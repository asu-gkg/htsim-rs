from pathlib import Path
from typing import Dict, Optional

import torch

from .config import (
    default_device_dir,
    default_model_dir,
    load_device_config,
    load_model_config,
    resolve_repo_root,
)
from .model import build_minimal_model
from .hf_model import build_transformers_model
from .profiler import ProfileResult, profile_model
from .workload import build_rank_map, build_rank_steps


def _parse_int(value, name: str, min_value: int = 1) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        raise ValueError(f"{name} must be an integer")
    if parsed < min_value:
        raise ValueError(f"{name} must be >= {min_value}")
    return parsed


def _parse_float(value, name: str, min_value: float = 0.0) -> float:
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        raise ValueError(f"{name} must be a number")
    if parsed < min_value:
        raise ValueError(f"{name} must be >= {min_value}")
    return parsed


def _dtype_from_name(name: str) -> torch.dtype:
    name = str(name or "").lower()
    if name in ("fp16", "float16"):
        return torch.float16
    if name in ("bf16", "bfloat16"):
        return torch.bfloat16
    return torch.float32


def _module_param_bytes(module: torch.nn.Module) -> int:
    total = 0
    for param in module.parameters(recurse=True):
        total += param.numel() * param.element_size()
    return int(total)


def _build_stage_stats(
    layer_stats,
    prologue_stats,
    epilogue_stats,
    pp_degree,
):
    if pp_degree <= 0:
        raise ValueError("pp must be >= 1")
    if len(layer_stats) % pp_degree != 0:
        raise ValueError("num_layers must be divisible by pp")
    per_stage = len(layer_stats) // pp_degree
    stage_stats = []
    for stage in range(pp_degree):
        start = stage * per_stage
        end = start + per_stage
        chunk = layer_stats[start:end]
        stage_stat = {
            "fw_compute_ms": sum(item["fw_compute_ms"] for item in chunk),
            "bw_compute_ms": sum(item["bw_compute_ms"] for item in chunk),
            "tp_fw_bytes": sum(item["tp_fw_bytes"] for item in chunk),
            "tp_bw_bytes": sum(item["tp_bw_bytes"] for item in chunk),
            "dp_bw_bytes": sum(item["dp_bw_bytes"] for item in chunk),
            "pp_bytes": chunk[-1]["pp_bytes"] if chunk else 0,
        }
        if stage == 0 and prologue_stats:
            stage_stat["fw_compute_ms"] += prologue_stats["fw_compute_ms"]
            stage_stat["bw_compute_ms"] += prologue_stats["bw_compute_ms"]
            stage_stat["tp_fw_bytes"] += prologue_stats["tp_fw_bytes"]
            stage_stat["tp_bw_bytes"] += prologue_stats["tp_bw_bytes"]
            stage_stat["dp_bw_bytes"] += prologue_stats["dp_bw_bytes"]
        if stage == pp_degree - 1 and epilogue_stats:
            stage_stat["fw_compute_ms"] += epilogue_stats["fw_compute_ms"]
            stage_stat["bw_compute_ms"] += epilogue_stats["bw_compute_ms"]
            stage_stat["tp_fw_bytes"] += epilogue_stats["tp_fw_bytes"]
            stage_stat["tp_bw_bytes"] += epilogue_stats["tp_bw_bytes"]
            stage_stat["dp_bw_bytes"] += epilogue_stats["dp_bw_bytes"]
        stage_stats.append(stage_stat)
    return stage_stats


def _build_layer_stats(
    profile: ProfileResult,
    layer_modules,
    mode: str,
    dp_degree: int,
    tp_degree: int,
    bytes_per_element: int,
    tp_comm_factor: float,
    time_scale: float,
):
    layer_stats = []
    for layer_profile, (_name, module) in zip(profile.layers, layer_modules):
        shape = layer_profile.output_shape
        if not shape:
            raise ValueError(f"missing output shape for {layer_profile.name}")
        elems = 1
        for dim in shape:
            elems *= int(dim)
        activation_bytes = int(elems * bytes_per_element)
        param_bytes = _module_param_bytes(module)
        tp_fw_bytes = int(activation_bytes * tp_comm_factor) if tp_degree > 1 else 0
        tp_bw_bytes = int(activation_bytes * tp_comm_factor) if tp_degree > 1 and mode == "train" else 0
        dp_bw_bytes = int(param_bytes) if dp_degree > 1 and mode == "train" else 0
        scaled_fw = layer_profile.fw_ms * time_scale
        scaled_bw = layer_profile.bw_ms * time_scale
        layer_stats.append(
            {
                "fw_compute_ms": scaled_fw,
                "bw_compute_ms": scaled_bw if mode == "train" else 0.0,
                "tp_fw_bytes": tp_fw_bytes,
                "tp_bw_bytes": tp_bw_bytes,
                "dp_bw_bytes": dp_bw_bytes,
                "pp_bytes": activation_bytes,
            }
        )
    return layer_stats


def _build_extra_stats(
    extra,
    module,
    mode: str,
    dp_degree: int,
    time_scale: float,
):
    if extra is None or module is None:
        return None
    param_bytes = _module_param_bytes(module)
    return {
        "fw_compute_ms": extra.fw_ms * time_scale,
        "bw_compute_ms": extra.bw_ms * time_scale if mode == "train" else 0.0,
        "tp_fw_bytes": 0,
        "tp_bw_bytes": 0,
        "dp_bw_bytes": int(param_bytes) if dp_degree > 1 and mode == "train" else 0,
        "pp_bytes": 0,
    }


def _ratio(numerator: float, denominator: float) -> float:
    if denominator <= 0:
        return 1.0
    if numerator <= 0:
        return 1.0
    return float(numerator) / float(denominator)


def _compute_time_scale(profile_cfg: Optional[Dict], target_cfg: Optional[Dict], mode: str) -> float:
    if not profile_cfg or not target_cfg:
        return 1.0
    comp_scale = _ratio(profile_cfg.get("SingleFLOPs", 0), target_cfg.get("SingleFLOPs", 0))
    mem_scale = _ratio(profile_cfg.get("Mem_Bw", 0), target_cfg.get("Mem_Bw", 0))
    if mode == "compute":
        return comp_scale
    if mode == "memory":
        return mem_scale
    if mode == "mean":
        return 0.5 * (comp_scale + mem_scale)
    return max(comp_scale, mem_scale)


def generate_workload(payload: Dict, repo_root: Optional[str] = None) -> Dict:
    model = str(payload.get("model") or "").strip()
    gpu = str(payload.get("gpu") or "").strip()
    mode = str(payload.get("mode") or "train").strip()
    seq = _parse_int(payload.get("seq"), "seq")
    batch = _parse_int(payload.get("batch"), "batch")
    dp_degree = _parse_int(payload.get("dp", 1), "dp")
    tp_degree = _parse_int(payload.get("tp", 1), "tp")
    pp_degree = _parse_int(payload.get("pp", 1), "pp")
    pp_microbatch = _parse_int(payload.get("pp_microbatch", 1), "pp_microbatch")
    pipeline = str(payload.get("pipeline") or "1f1b").strip()
    dtype_name = str(payload.get("dtype") or "fp16").strip()
    device_name = str(payload.get("device") or "cuda").strip()
    device_scale_mode = str(payload.get("device_scale_mode") or "max").strip().lower()
    model_backend = str(payload.get("model_backend") or "transformers").strip().lower()
    warmup_steps = _parse_int(payload.get("warmup_steps", 1), "warmup_steps", min_value=0)
    measure_steps = _parse_int(payload.get("measure_steps", 1), "measure_steps", min_value=1)
    tp_comm_factor = _parse_float(payload.get("tp_comm_factor", 2.0), "tp_comm_factor")

    if not model:
        raise ValueError("model is required")
    if not gpu:
        raise ValueError("gpu is required")
    if mode not in ("train", "inf"):
        raise ValueError("mode must be train or inf")

    repo_root_path = resolve_repo_root(repo_root)
    model_dir = Path(payload.get("model_dir") or default_model_dir(repo_root_path))
    device_dir = Path(payload.get("device_dir") or default_device_dir(repo_root_path))

    spec = load_model_config(model, model_dir)
    target_cfg = load_device_config(gpu, device_dir)

    if batch % dp_degree != 0:
        raise ValueError("batch must be divisible by dp")
    micro_batch = batch // dp_degree
    if micro_batch % pp_microbatch != 0:
        raise ValueError("batch/dp must be divisible by pp_microbatch")
    profile_batch = micro_batch // pp_microbatch
    if profile_batch <= 0:
        raise ValueError("invalid microbatch size")

    if spec.num_layers % pp_degree != 0:
        raise ValueError("num_layers must be divisible by pp")

    dtype = _dtype_from_name(dtype_name)
    device = torch.device(device_name)
    if device.type == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("cuda is not available on this host")
    if device.type == "cpu" and dtype in (torch.float16, torch.bfloat16):
        raise RuntimeError("fp16/bf16 profiling on cpu is not supported")

    if model_backend == "transformers":
        model_path = model_dir / f"{model}.json"
        model_torch, layer_modules, prologue_modules, epilogue_modules = build_transformers_model(
            str(model_path)
        )
    elif model_backend == "minimal":
        model_torch = build_minimal_model(spec)
        layer_modules = [(f"layer_{idx}", layer) for idx, layer in enumerate(model_torch.layers)]
        prologue_modules = [("prologue", model_torch.prologue)]
        epilogue_modules = [("epilogue", model_torch.epilogue)]
    else:
        raise ValueError(f"unknown model_backend: {model_backend}")

    model_torch = model_torch.to(device=device, dtype=dtype)
    model_torch.train(mode == "train")

    if not layer_modules:
        raise RuntimeError("no transformer layers found for profiling")
    prologue_module = prologue_modules[0][1] if prologue_modules else None
    epilogue_module = epilogue_modules[0][1] if epilogue_modules else None

    input_ids = torch.randint(
        0,
        spec.vocab_size,
        (profile_batch, seq),
        device=device,
        dtype=torch.long,
    )

    profile = profile_model(
        model=model_torch,
        input_ids=input_ids,
        layer_modules=layer_modules,
        prologue_modules=prologue_modules,
        epilogue_modules=epilogue_modules,
        mode=mode,
        warmup_steps=warmup_steps,
        measure_steps=measure_steps,
    )

    profile_gpu = payload.get("profile_gpu")
    profile_cfg = None
    if device.type == "cuda" and torch.cuda.is_available():
        try:
            index = device.index if device.index is not None else 0
            detected = torch.cuda.get_device_name(index)
            if detected:
                profile_gpu = detected.replace(" ", "_")
        except Exception:
            profile_gpu = profile_gpu
    if not profile_gpu:
        profile_gpu = gpu
    if profile_gpu:
        try:
            profile_cfg = load_device_config(str(profile_gpu), device_dir)
        except FileNotFoundError:
            profile_cfg = None
    if device.type != "cuda":
        device_scale_mode = "none"

    time_scale = 1.0
    if device_scale_mode not in ("none", "off"):
        time_scale = _compute_time_scale(profile_cfg, target_cfg, device_scale_mode)

    bytes_per_element = torch.tensor([], dtype=dtype).element_size()
    layer_stats = _build_layer_stats(
        profile=profile,
        layer_modules=layer_modules,
        mode=mode,
        dp_degree=dp_degree,
        tp_degree=tp_degree,
        bytes_per_element=bytes_per_element,
        tp_comm_factor=tp_comm_factor,
        time_scale=time_scale,
    )
    prologue_stats = _build_extra_stats(profile.prologue, prologue_module, mode, dp_degree, time_scale)
    epilogue_stats = _build_extra_stats(profile.epilogue, epilogue_module, mode, dp_degree, time_scale)

    stage_stats = _build_stage_stats(layer_stats, prologue_stats, epilogue_stats, pp_degree)

    microbatches = max(1, pp_microbatch)
    ranks = []
    for rank_info in build_rank_map(dp_degree, pp_degree, tp_degree):
        steps = build_rank_steps(
            rank_info,
            stage_stats,
            dp_degree,
            pp_degree,
            tp_degree,
            microbatches,
            pipeline,
            mode,
        )
        ranks.append({"id": rank_info["id"], "steps": steps})

    host_count = dp_degree * tp_degree * pp_degree
    hosts = []
    for hid in range(host_count):
        entry = {"id": hid, "topo_index": hid}
        if gpu:
            entry["gpu"] = {"model": gpu}
        hosts.append(entry)

    topology = payload.get("topology")
    if not topology:
        topo_kind = "dumbbell" if host_count == 2 else "fat_tree"
        if topo_kind == "fat_tree":
            topology = {
                "kind": "fat_tree",
                "k": 4,
                "link_gbps": 100,
                "link_latency_us": 2,
            }
        else:
            topology = {
                "kind": "dumbbell",
                "host_link_gbps": 100,
                "bottleneck_gbps": 10,
                "link_latency_us": 2,
            }

    defaults = payload.get("defaults") or {"protocol": "tcp", "routing": "per_flow"}

    workload = {
        "schema_version": 2,
        "meta": {
            "source": "workload_gen",
            "model": model,
            "num_layers": spec.num_layers,
            "device": gpu,
            "profile": {
                "mode": mode,
                "seq": seq,
                "batch": batch,
                "micro_batch": profile_batch,
                "dtype": dtype_name,
                "device": device_name,
                "warmup_steps": warmup_steps,
                "measure_steps": measure_steps,
                "tp_comm_factor": tp_comm_factor,
                "device_scale_mode": device_scale_mode,
                "device_scale": time_scale,
                "profile_gpu": profile_gpu,
                "target_gpu": gpu,
                "model_backend": model_backend,
            },
            "parallel": {
                "dp": dp_degree,
                "tp": tp_degree,
                "pp": pp_degree,
                "pp_microbatch": microbatches,
                "layout": "dp-pp-tp",
                "pipeline": pipeline,
            },
        },
        "topology": topology,
        "defaults": {
            "protocol": defaults.get("protocol", "tcp"),
            "routing": defaults.get("routing", "per_flow"),
            "bytes_per_element": bytes_per_element,
        },
        "hosts": hosts,
        "ranks": ranks,
    }

    return workload
