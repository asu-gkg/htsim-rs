#!/usr/bin/env python3
import argparse
import ast
import csv
import json
import re
from pathlib import Path

COMM_OPS = {
    "ALLREDUCE",
    "ALLREDUCE_ASYNC",
    "ALLGATHER",
    "REDUCESCATTER",
    "ALLTOALL",
    "ALLTOALL_EP",
    "ALLGATHER_DP_EP",
    "REDUCESCATTER_DP_EP",
    "SENDRECV",
}


def parse_ops(cell):
    if not cell:
        return []
    try:
        return ast.literal_eval(cell)
    except (ValueError, SyntaxError):
        return []


def parse_literal(cell, default):
    if not cell:
        return default
    try:
        return ast.literal_eval(cell)
    except (ValueError, SyntaxError):
        return default


def extract_comm_ops(ops):
    totals = {}
    for op in ops:
        if not op or len(op) < 2:
            continue
        name = op[0]
        if name not in COMM_OPS:
            continue
        args = op[1]
        if isinstance(args, (list, tuple)) and args:
            size = args[0]
            if isinstance(size, (int, float)) and size > 0:
                totals[name] = totals.get(name, 0) + int(size)
    return totals


def find_device_from_path(path: Path):
    parts = list(path.parts)
    for part in parts:
        if part.startswith("NVIDIA_") or part.startswith("AMD_") or part.startswith("Tesla_"):
            return part.replace("_", " ")
    return None


def find_model_from_name(name: str):
    m = re.match(r"([a-zA-Z0-9_]+)-", name)
    return m.group(1) if m else name


def infer_fat_tree_k(hosts: int):
    if hosts <= 0:
        return None
    k = round((4 * hosts) ** (1.0 / 3.0))
    if k > 0 and (k**3) // 4 == hosts:
        return k
    return None


def replicate_layers(rows, model_name, num_layers):
    if not num_layers or num_layers <= 1:
        return rows

    model_name = model_name.lower()
    if "switch" in model_name:
        return rows

    def find_index(target):
        for i, row in enumerate(rows):
            if row.get("Name") == target:
                return i
        return None

    if "bert" in model_name:
        start = find_index("bert_encoder_layer_0_attention_self_query")
        end = find_index("bert_encoder_layer_0_output_layer_norm")
    elif "gpt" in model_name:
        start = find_index("transformer_h_0_ln_1_grad")
        if start is None:
            start = find_index("transformer_h_0_ln_1")
        end = find_index("add_15")
    elif "opt" in model_name:
        start = find_index("model_decoder_layers_0_self_attn_layer_norm")
        end = find_index("view_11")
    else:
        return rows

    if start is None or end is None:
        return rows

    end = end + 1
    prologue = rows[:start]
    layer = rows[start:end]
    epilogue = rows[end:]
    return prologue + (layer * num_layers) + epilogue


def parse_parallel_options(options: str):
    dp_degree = 1
    tp_degree = 1
    pp_degree = 1
    pp_num_microbatch = 1
    tokens = [t for t in str(options or "").split(",") if t]
    for token in tokens:
        token = token.strip()
        if not token or token == "fusion":
            continue
        if re.match(r"dp\d+$", token):
            dp_degree = int(token[2:])
            continue
        if re.match(r"tp\d+$", token):
            tp_degree = int(token[2:])
            continue
        if re.match(r"pp\d+_\d+$", token):
            parts = token[2:].split("_")
            pp_degree = int(parts[0])
            pp_num_microbatch = int(parts[1])
            continue
        raise ValueError(f"unknown option token: {token}")
    return dp_degree, tp_degree, pp_degree, pp_num_microbatch


def split_layers(rows, model_name, num_layers):
    if not num_layers or num_layers <= 1:
        return [], [rows], []

    model_name = model_name.lower()
    if "switch" in model_name:
        return [], [rows], []

    def find_index(target):
        for i, row in enumerate(rows):
            if row.get("Name") == target:
                return i
        return None

    if "bert" in model_name:
        start = find_index("bert_encoder_layer_0_attention_self_query")
        end = find_index("bert_encoder_layer_0_output_layer_norm")
    elif "gpt" in model_name:
        start = find_index("transformer_h_0_ln_1_grad")
        if start is None:
            start = find_index("transformer_h_0_ln_1")
        end = find_index("add_15")
    elif "opt" in model_name:
        start = find_index("model_decoder_layers_0_self_attn_layer_norm")
        end = find_index("view_11")
    else:
        return [], [rows], []

    if start is None or end is None:
        return [], [rows], []

    end = end + 1
    prologue = rows[:start]
    layer = rows[start:end]
    epilogue = rows[end:]
    layers = [list(layer) for _ in range(num_layers)]
    return prologue, layers, epilogue


def comm_bytes_from_ops(ops, bytes_per_element):
    return comm_stats_from_ops(ops, bytes_per_element)[0]


def normalize_comm_op_name(raw):
    name = str(raw or "").upper()
    if name == "ALLREDUCE":
        return "allreduce"
    if name == "ALLREDUCE_ASYNC":
        return "allreduce_async"
    if name in ("ALLGATHER", "ALLGATHER_DP_EP"):
        return "allgather"
    if name in ("REDUCESCATTER", "REDUCESCATTER_DP_EP"):
        return "reducescatter"
    if name in ("ALLTOALL", "ALLTOALL_EP"):
        return "alltoall"
    if name == "SENDRECV":
        return "sendrecv"
    return name.lower()


def merge_comm_by_op(into, from_map):
    if not from_map:
        return
    for op, bytes_val in from_map.items():
        try:
            value = int(bytes_val)
        except (TypeError, ValueError):
            continue
        if value <= 0:
            continue
        into[op] = into.get(op, 0) + value


def pick_primary_op(by_op):
    if not by_op:
        return ""
    best = ""
    best_bytes = 0
    for op, bytes_val in by_op.items():
        try:
            value = int(bytes_val)
        except (TypeError, ValueError):
            continue
        if value <= 0:
            continue
        if value > best_bytes:
            best = op
            best_bytes = value
    return best


def comm_stats_from_ops(ops, bytes_per_element):
    by_op = {}
    total_bytes = 0
    for op in ops or []:
        if not op or len(op) < 2:
            continue
        name = op[0]
        if name not in COMM_OPS:
            continue
        args = op[1]
        if isinstance(args, (list, tuple)) and args:
            size = args[0]
            if isinstance(size, (int, float)) and size > 0:
                bytes_val = int(size) * bytes_per_element
                total_bytes += bytes_val
                norm = normalize_comm_op_name(name)
                by_op[norm] = by_op.get(norm, 0) + bytes_val
    return total_bytes, by_op


def infer_comm_group(row):
    name = str(row.get("Name", "")).lower()
    opname = str(row.get("OpName", "")).lower()
    if "sendrecv" in name or opname == "sendrecv":
        return "pp"
    if name.endswith("_grad") and opname == "allreduce":
        return "dp"
    if (
        "tensor_model_parallel" in name
        or "reduce_from_tensor_model_parallel_region" in name
        or "gather_from_tensor_model_parallel_region" in name
        or "reduce_scatter_to_tensor_model_parallel_region" in name
        or "scatter_to_tensor_model_parallel_region" in name
    ):
        return "tp"
    return ""


def collect_layer_stats(rows, bytes_per_element):
    fw_compute_ms = 0.0
    bw_compute_ms = 0.0
    tp_fw_bytes = 0
    tp_bw_bytes = 0
    dp_bw_bytes = 0
    pp_bytes = 0
    tp_fw_by_op = {}
    tp_bw_by_op = {}
    dp_bw_by_op = {}
    unknown_fw_by_op = {}
    unknown_bw_by_op = {}
    for row in rows:
        fw_ops = row.get("FwOps", [])
        bw_ops = row.get("BwOps", [])
        fw_comm, fw_by_op = comm_stats_from_ops(fw_ops, bytes_per_element)
        bw_comm, bw_by_op = comm_stats_from_ops(bw_ops, bytes_per_element)
        comm_group = row.get("CommGroup") or infer_comm_group(row)

        if comm_group:
            if comm_group == "tp":
                tp_fw_bytes += fw_comm
                tp_bw_bytes += bw_comm
                merge_comm_by_op(tp_fw_by_op, fw_by_op)
                merge_comm_by_op(tp_bw_by_op, bw_by_op)
            elif comm_group == "dp":
                dp_bw_bytes += fw_comm + bw_comm
                merge_comm_by_op(dp_bw_by_op, fw_by_op)
                merge_comm_by_op(dp_bw_by_op, bw_by_op)
            elif comm_group == "pp":
                pp_bytes = max(pp_bytes, fw_comm, bw_comm)
            continue

        fw_compute_ms += float(row.get("fw_latency") or 0.0)
        bw_compute_ms += float(row.get("bw_latency") or 0.0) + float(row.get("acc_latency") or 0.0)

        if fw_comm or bw_comm:
            inferred = infer_comm_group(row)
            if inferred == "tp":
                tp_fw_bytes += fw_comm
                tp_bw_bytes += bw_comm
                merge_comm_by_op(tp_fw_by_op, fw_by_op)
                merge_comm_by_op(tp_bw_by_op, bw_by_op)
            elif inferred == "dp":
                dp_bw_bytes += fw_comm + bw_comm
                merge_comm_by_op(dp_bw_by_op, fw_by_op)
                merge_comm_by_op(dp_bw_by_op, bw_by_op)
            elif inferred == "pp":
                pp_bytes = max(pp_bytes, fw_comm, bw_comm)
            else:
                merge_comm_by_op(unknown_fw_by_op, fw_by_op)
                merge_comm_by_op(unknown_bw_by_op, bw_by_op)

    if pp_bytes <= 0 and rows:
        shape = rows[-1].get("OutputShape")
        if isinstance(shape, (list, tuple)) and shape:
            elems = 1
            for dim in shape:
                if isinstance(dim, (int, float)) and dim > 0:
                    elems *= int(dim)
            if elems > 0:
                pp_bytes = elems * bytes_per_element

    return {
        "fw_compute_ms": fw_compute_ms,
        "bw_compute_ms": bw_compute_ms,
        "tp_fw_bytes": tp_fw_bytes,
        "tp_bw_bytes": tp_bw_bytes,
        "dp_bw_bytes": dp_bw_bytes,
        "pp_bytes": pp_bytes,
        "tp_fw_by_op": tp_fw_by_op,
        "tp_bw_by_op": tp_bw_by_op,
        "dp_bw_by_op": dp_bw_by_op,
        "unknown_fw_by_op": unknown_fw_by_op,
        "unknown_bw_by_op": unknown_bw_by_op,
    }


def rank_for(dp_idx, pp_idx, tp_idx, dp_degree, pp_degree, tp_degree):
    return (dp_idx * pp_degree + pp_idx) * tp_degree + tp_idx


def build_rank_map(dp_degree, pp_degree, tp_degree):
    ranks = []
    for dp_idx in range(dp_degree):
        for pp_idx in range(pp_degree):
            for tp_idx in range(tp_degree):
                rank = rank_for(dp_idx, pp_idx, tp_idx, dp_degree, pp_degree, tp_degree)
                ranks.append({"id": rank, "dp": dp_idx, "pp": pp_idx, "tp": tp_idx})
    return ranks


def build_rank_steps(
    rank_info,
    stage_stats,
    dp_degree,
    pp_degree,
    tp_degree,
    microbatches,
    pipeline,
    collective_wait,
):
    dp_idx = rank_info["dp"]
    pp_idx = rank_info["pp"]
    tp_idx = rank_info["tp"]

    tp_group = [rank_for(dp_idx, pp_idx, t, dp_degree, pp_degree, tp_degree) for t in range(tp_degree)]
    dp_group = [rank_for(d, pp_idx, tp_idx, dp_degree, pp_degree, tp_degree) for d in range(dp_degree)]

    prev_rank = None
    next_rank = None
    if pp_idx > 0:
        prev_rank = rank_for(dp_idx, pp_idx - 1, tp_idx, dp_degree, pp_degree, tp_degree)
    if pp_idx + 1 < pp_degree:
        next_rank = rank_for(dp_idx, pp_idx + 1, tp_idx, dp_degree, pp_degree, tp_degree)

    stats = stage_stats[pp_idx]

    steps = []

    def pp_comm_id(direction, src_stage, microbatch):
        return f"pp-{direction}-s{src_stage}-mb{microbatch}-dp{dp_idx}-tp{tp_idx}"

    def tp_comm_id(direction, microbatch):
        return f"tp-{direction}-pp{pp_idx}-dp{dp_idx}-mb{microbatch}"

    def dp_comm_id(direction, microbatch):
        return f"dp-{direction}-pp{pp_idx}-tp{tp_idx}-mb{microbatch}"

    def add_compute(label, ms):
        if ms <= 0:
            return
        steps.append({"kind": "compute", "label": label, "compute_ms": round(ms, 6)})

    def add_collective(label, op, comm_bytes, hosts, comm_id):
        if comm_bytes <= 0:
            return
        steps.append(
            {
                "kind": "collective",
                "label": label,
                "op": op,
                "comm_bytes": int(comm_bytes),
                "hosts": hosts,
                "comm_id": comm_id,
            }
        )

    def add_collectives_by_op(label, by_op, total_bytes, hosts, comm_id):
        items = []
        for op, bytes_val in (by_op or {}).items():
            try:
                value = int(bytes_val)
            except (TypeError, ValueError):
                continue
            if value <= 0:
                continue
            items.append((str(op), value))
        if not items:
            add_collective(label, "allreduce", total_bytes, hosts, comm_id)
            return
        if len(items) == 1:
            op, bytes_val = items[0]
            add_collective(label, op, bytes_val, hosts, comm_id)
            return
        for op, bytes_val in sorted(items, key=lambda item: item[0]):
            add_collective(f"{label}_{op}", op, bytes_val, hosts, f"{comm_id}-{op}")

    def add_sendrecv(label, comm_bytes, peer, direction, comm_id):
        if comm_bytes <= 0 or peer is None:
            return
        steps.append(
            {
                "kind": "sendrecv",
                "label": label,
                "comm_bytes": int(comm_bytes),
                "peer": peer,
                "direction": direction,
                "comm_id": comm_id,
            }
        )

    def forward_step(microbatch):
        tp_fw_by_op = dict(stats.get("tp_fw_by_op") or {})
        dp_fw_by_op = {}
        if tp_degree > 1:
            merge_comm_by_op(tp_fw_by_op, stats.get("unknown_fw_by_op"))
        else:
            merge_comm_by_op(dp_fw_by_op, stats.get("unknown_fw_by_op"))

        if prev_rank is not None:
            add_sendrecv(
                f"fwd_recv_mb{microbatch}",
                stats["pp_bytes"],
                prev_rank,
                "recv",
                pp_comm_id("fwd", pp_idx - 1, microbatch),
            )
        add_compute(f"fwd_mb{microbatch}", stats["fw_compute_ms"])
        add_collectives_by_op(
            f"tp_fwd_mb{microbatch}",
            tp_fw_by_op,
            stats["tp_fw_bytes"],
            tp_group,
            tp_comm_id("fwd", microbatch),
        )
        # If tensor-parallel is disabled, still model forward comm on data-parallel ranks.
        if tp_degree <= 1:
            add_collectives_by_op(
                f"dp_fwd_mb{microbatch}",
                dp_fw_by_op,
                0,
                dp_group,
                dp_comm_id("fwd", microbatch),
            )
        if next_rank is not None:
            add_sendrecv(
                f"fwd_send_mb{microbatch}",
                stats["pp_bytes"],
                next_rank,
                "send",
                pp_comm_id("fwd", pp_idx, microbatch),
            )

    def backward_step(microbatch):
        tp_bw_by_op = dict(stats.get("tp_bw_by_op") or {})
        dp_bw_by_op = dict(stats.get("dp_bw_by_op") or {})
        if tp_degree > 1:
            merge_comm_by_op(tp_bw_by_op, stats.get("unknown_bw_by_op"))
        else:
            merge_comm_by_op(dp_bw_by_op, stats.get("unknown_bw_by_op"))

        if next_rank is not None:
            add_sendrecv(
                f"bwd_recv_mb{microbatch}",
                stats["pp_bytes"],
                next_rank,
                "recv",
                pp_comm_id("bwd", pp_idx + 1, microbatch),
            )
        add_compute(f"bwd_mb{microbatch}", stats["bw_compute_ms"])
        add_collectives_by_op(
            f"tp_bwd_mb{microbatch}",
            tp_bw_by_op,
            stats["tp_bw_bytes"],
            tp_group,
            tp_comm_id("bwd", microbatch),
        )
        add_collectives_by_op(
            f"dp_bwd_mb{microbatch}",
            dp_bw_by_op,
            stats["dp_bw_bytes"],
            dp_group,
            dp_comm_id("bwd", microbatch),
        )
        if prev_rank is not None:
            add_sendrecv(
                f"bwd_send_mb{microbatch}",
                stats["pp_bytes"],
                prev_rank,
                "send",
                pp_comm_id("bwd", pp_idx, microbatch),
            )

    if pipeline == "fwd_bwd":
        for microbatch in range(microbatches):
            forward_step(microbatch)
        for microbatch in range(microbatches):
            backward_step(microbatch)
    else:
        num_warmup = min(microbatches, pp_degree - pp_idx - 1)
        num_remaining = microbatches - num_warmup
        fwd_idx = 0
        bwd_idx = 0

        for _ in range(num_warmup):
            forward_step(fwd_idx)
            fwd_idx += 1

        for _ in range(num_remaining):
            forward_step(fwd_idx)
            fwd_idx += 1
            backward_step(bwd_idx)
            bwd_idx += 1

        while bwd_idx < microbatches:
            backward_step(bwd_idx)
            bwd_idx += 1

    # If any async collective was launched, the simulator needs an explicit wait
    # step to model the dependency point (e.g., end of iteration).
    if str(collective_wait or "").lower() == "end":
        def is_async_collective(step):
            if step.get("kind") != "collective":
                return False
            op = str(step.get("op") or "").strip().lower()
            compact = "".join(ch for ch in op if ch not in ("_", "-"))
            return compact.endswith("async")

        if any(is_async_collective(step) for step in steps):
            steps.append({"kind": "collective_wait", "label": "collective_wait"})

    for idx, step in enumerate(steps):
        step["id"] = idx
    return steps


def rows_to_steps(rows, hosts, bytes_per_element):
    steps = []
    compute_ms = 0.0
    for row in rows:
        fw_raw = row.get("FwOps", "")
        bw_raw = row.get("BwOps", "")
        fw_ops = fw_raw if isinstance(fw_raw, list) else parse_ops(fw_raw)
        bw_ops = bw_raw if isinstance(bw_raw, list) else parse_ops(bw_raw)
        comm_totals = {}
        for name, size in extract_comm_ops(fw_ops).items():
            comm_totals[name] = comm_totals.get(name, 0) + size
        for name, size in extract_comm_ops(bw_ops).items():
            comm_totals[name] = comm_totals.get(name, 0) + size
        comm_elems = sum(comm_totals.values())
        if comm_elems > 0:
            comm_bytes = comm_elems * bytes_per_element
            comm_ops = [
                {
                    "op": name,
                    "comm_elems": elems,
                    "comm_bytes": elems * bytes_per_element,
                }
                for name, elems in comm_totals.items()
            ]
            steps.append(
                {
                    "id": len(steps),
                    "label": row.get("Name") or None,
                    "hosts": hosts,
                    "compute_ms": round(compute_ms, 6),
                    "comm_bytes": comm_bytes,
                    "comm_ops": comm_ops,
                }
            )
            compute_ms = 0.0
            continue

        fw = float(row.get("fw_latency") or 0.0)
        bw = float(row.get("bw_latency") or 0.0)
        acc = float(row.get("acc_latency") or 0.0)
        compute_ms += fw + bw + acc

    if compute_ms > 0:
        steps.append(
            {
                "id": len(steps),
                "label": "compute_tail",
                "hosts": hosts,
                "compute_ms": round(compute_ms, 6),
                "comm_bytes": 0,
            }
        )
    return steps


def main():
    parser = argparse.ArgumentParser(description="Convert NeuSight prediction CSV into workload.json")
    parser.add_argument("--pred-csv", required=True, help="Path to NeuSight prediction CSV (with *_latency columns)")
    parser.add_argument("--out", default="workload.json", help="Output workload.json path")
    parser.add_argument("--summary-json", help="Optional NeuSight summary JSON to read num_layer")
    parser.add_argument("--num-layers", type=int, help="Override number of layers to replicate")
    parser.add_argument("--hosts", type=int, help="Number of hosts / ranks")
    parser.add_argument("--schema-version", type=int, default=1, choices=[1, 2], help="Workload schema version")
    parser.add_argument("--options", default="", help="Parallel options (dpX,tpY,ppZ_M)")
    parser.add_argument("--dp", type=int, default=1, help="Data-parallel degree")
    parser.add_argument("--tp", type=int, default=1, help="Tensor-parallel degree")
    parser.add_argument("--pp", type=int, default=1, help="Pipeline-parallel degree")
    parser.add_argument("--pp-microbatch", type=int, default=1, help="Pipeline microbatch count")
    parser.add_argument("--layout", default="dp-pp-tp", choices=["dp-pp-tp"], help="Rank layout order")
    parser.add_argument("--pipeline", default="1f1b", choices=["1f1b", "fwd_bwd"], help="Pipeline schedule")
    parser.add_argument(
        "--collective-wait",
        default="end",
        choices=["none", "end"],
        help="Insert `collective_wait` for `*_async` collectives (schema_version=2)",
    )
    parser.add_argument("--gpu", help="GPU model (e.g. NVIDIA H100)")
    parser.add_argument("--protocol", default="tcp", choices=["tcp", "dctcp"], help="Default transport protocol")
    parser.add_argument("--routing", default="per_flow", choices=["per_flow", "per_packet"], help="ECMP routing mode")
    parser.add_argument("--bytes-per-element", type=int, default=4, help="Element size for comm bytes")
    parser.add_argument("--topo-kind", choices=["dumbbell", "fat_tree"], help="Topology kind")
    parser.add_argument("--k", type=int, help="Fat-tree k (required if topo-kind=fat_tree)")
    parser.add_argument("--link-gbps", type=int, default=100, help="Fat-tree link bandwidth in Gbps")
    parser.add_argument("--link-latency-us", type=int, default=2, help="Link latency in microseconds")
    parser.add_argument("--host-link-gbps", type=int, default=100, help="Dumbbell host link bandwidth in Gbps")
    parser.add_argument("--bottleneck-gbps", type=int, default=10, help="Dumbbell bottleneck bandwidth in Gbps")
    args = parser.parse_args()

    pred_path = Path(args.pred_csv)
    if not pred_path.exists():
        raise SystemExit(f"prediction CSV not found: {pred_path}")

    rows = []
    with pred_path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            row["FwOps"] = parse_literal(row.get("FwOps"), [])
            row["BwOps"] = parse_literal(row.get("BwOps"), [])
            row["AccOps"] = parse_literal(row.get("AccOps"), [])
            row["InputShapes"] = parse_literal(row.get("InputShapes"), [])
            row["OutputShape"] = parse_literal(row.get("OutputShape"), [])
            for key in ("fw_latency", "bw_latency", "acc_latency", "bwall_latency", "e2e_latency"):
                row[key] = float(row.get(key) or 0.0)
            rows.append(row)

    model_name = find_model_from_name(pred_path.name)
    gpu_name = args.gpu or find_device_from_path(pred_path)

    num_layers = args.num_layers
    if num_layers is None and args.summary_json:
        with Path(args.summary_json).open() as f:
            summary = json.load(f)
        num_layers = int(summary.get("num_layer") or 0) or None

    dp_degree = args.dp
    tp_degree = args.tp
    pp_degree = args.pp
    pp_microbatch = args.pp_microbatch
    if args.options:
        dp_degree, tp_degree, pp_degree, pp_microbatch = parse_parallel_options(args.options)

    host_count = args.hosts
    if host_count is None:
        host_count = dp_degree * tp_degree * pp_degree if args.schema_version == 2 else 1

    if args.schema_version == 2:
        expected = dp_degree * tp_degree * pp_degree
        if host_count != expected:
            raise SystemExit(f"hosts must equal dp*tp*pp ({expected}), got {host_count}")

    host_ids = list(range(host_count))

    topo_kind = args.topo_kind
    if topo_kind is None:
        topo_kind = "dumbbell" if host_count == 2 else "fat_tree"

    if topo_kind == "fat_tree":
        k = args.k or infer_fat_tree_k(host_count)
        if not k:
            raise SystemExit("fat_tree requires --k (unable to infer from hosts)")
        topo = {
            "kind": "fat_tree",
            "k": k,
            "link_gbps": args.link_gbps,
            "link_latency_us": args.link_latency_us,
        }
    else:
        topo = {
            "kind": "dumbbell",
            "host_link_gbps": args.host_link_gbps,
            "bottleneck_gbps": args.bottleneck_gbps,
            "link_latency_us": args.link_latency_us,
        }

    hosts = []
    for hid in host_ids:
        entry = {"id": hid, "topo_index": hid}
        if gpu_name:
            entry["gpu"] = {"model": gpu_name}
        hosts.append(entry)

    if args.schema_version == 1:
        rows_full = replicate_layers(rows, model_name, num_layers) if num_layers else rows
        steps = rows_to_steps(rows_full, host_ids, args.bytes_per_element)
        workload = {
            "schema_version": 1,
            "meta": {
                "source": str(pred_path),
                "model": model_name,
                "num_layers": num_layers,
                "device": gpu_name,
            },
            "topology": topo,
            "defaults": {
                "protocol": args.protocol,
                "routing": args.routing,
                "bytes_per_element": args.bytes_per_element,
            },
            "hosts": hosts,
            "steps": steps,
        }
    else:
        if pp_degree > 1 and not num_layers:
            raise SystemExit("pp requires --num-layers or --summary-json")
        prologue, layers, epilogue = split_layers(rows, model_name, num_layers or 1)
        layer_stats = [collect_layer_stats(layer, args.bytes_per_element) for layer in layers]
        prologue_stats = collect_layer_stats(prologue, args.bytes_per_element) if prologue else None
        epilogue_stats = collect_layer_stats(epilogue, args.bytes_per_element) if epilogue else None

        if pp_degree <= 0:
            raise SystemExit("pp must be >= 1")
        if len(layer_stats) % pp_degree != 0:
            raise SystemExit("num_layers must be divisible by pp")
        per_stage_layer = len(layer_stats) // pp_degree

        stage_stats = []
        for stage in range(pp_degree):
            start = stage * per_stage_layer
            end = start + per_stage_layer
            chunk = layer_stats[start:end]
            tp_fw_by_op = {}
            tp_bw_by_op = {}
            dp_bw_by_op = {}
            unknown_fw_by_op = {}
            unknown_bw_by_op = {}
            for item in chunk:
                merge_comm_by_op(tp_fw_by_op, item.get("tp_fw_by_op"))
                merge_comm_by_op(tp_bw_by_op, item.get("tp_bw_by_op"))
                merge_comm_by_op(dp_bw_by_op, item.get("dp_bw_by_op"))
                merge_comm_by_op(unknown_fw_by_op, item.get("unknown_fw_by_op"))
                merge_comm_by_op(unknown_bw_by_op, item.get("unknown_bw_by_op"))
            stage_stat = {
                "fw_compute_ms": sum(item["fw_compute_ms"] for item in chunk),
                "bw_compute_ms": sum(item["bw_compute_ms"] for item in chunk),
                "tp_fw_bytes": sum(item["tp_fw_bytes"] for item in chunk),
                "tp_bw_bytes": sum(item["tp_bw_bytes"] for item in chunk),
                "dp_bw_bytes": sum(item["dp_bw_bytes"] for item in chunk),
                "pp_bytes": chunk[-1]["pp_bytes"] if chunk else 0,
                "tp_fw_by_op": tp_fw_by_op,
                "tp_bw_by_op": tp_bw_by_op,
                "dp_bw_by_op": dp_bw_by_op,
                "unknown_fw_by_op": unknown_fw_by_op,
                "unknown_bw_by_op": unknown_bw_by_op,
            }
            if stage == 0 and prologue_stats:
                stage_stat["fw_compute_ms"] += prologue_stats["fw_compute_ms"]
                stage_stat["bw_compute_ms"] += prologue_stats["bw_compute_ms"]
                stage_stat["tp_fw_bytes"] += prologue_stats["tp_fw_bytes"]
                stage_stat["tp_bw_bytes"] += prologue_stats["tp_bw_bytes"]
                stage_stat["dp_bw_bytes"] += prologue_stats["dp_bw_bytes"]
                merge_comm_by_op(stage_stat["tp_fw_by_op"], prologue_stats.get("tp_fw_by_op"))
                merge_comm_by_op(stage_stat["tp_bw_by_op"], prologue_stats.get("tp_bw_by_op"))
                merge_comm_by_op(stage_stat["dp_bw_by_op"], prologue_stats.get("dp_bw_by_op"))
                merge_comm_by_op(
                    stage_stat["unknown_fw_by_op"],
                    prologue_stats.get("unknown_fw_by_op"),
                )
                merge_comm_by_op(
                    stage_stat["unknown_bw_by_op"],
                    prologue_stats.get("unknown_bw_by_op"),
                )
            if stage == pp_degree - 1 and epilogue_stats:
                stage_stat["fw_compute_ms"] += epilogue_stats["fw_compute_ms"]
                stage_stat["bw_compute_ms"] += epilogue_stats["bw_compute_ms"]
                stage_stat["tp_fw_bytes"] += epilogue_stats["tp_fw_bytes"]
                stage_stat["tp_bw_bytes"] += epilogue_stats["tp_bw_bytes"]
                stage_stat["dp_bw_bytes"] += epilogue_stats["dp_bw_bytes"]
                merge_comm_by_op(stage_stat["tp_fw_by_op"], epilogue_stats.get("tp_fw_by_op"))
                merge_comm_by_op(stage_stat["tp_bw_by_op"], epilogue_stats.get("tp_bw_by_op"))
                merge_comm_by_op(stage_stat["dp_bw_by_op"], epilogue_stats.get("dp_bw_by_op"))
                merge_comm_by_op(
                    stage_stat["unknown_fw_by_op"],
                    epilogue_stats.get("unknown_fw_by_op"),
                )
                merge_comm_by_op(
                    stage_stat["unknown_bw_by_op"],
                    epilogue_stats.get("unknown_bw_by_op"),
                )
            stage_stats.append(stage_stat)

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
                args.pipeline,
                args.collective_wait,
            )
            ranks.append({"id": rank_info["id"], "steps": steps})

        workload = {
            "schema_version": 2,
            "meta": {
                "source": str(pred_path),
                "model": model_name,
                "num_layers": num_layers,
                "device": gpu_name,
                "parallel": {
                    "dp": dp_degree,
                    "tp": tp_degree,
                    "pp": pp_degree,
                    "pp_microbatch": microbatches,
                    "layout": args.layout,
                    "pipeline": args.pipeline,
                },
            },
            "topology": topo,
            "defaults": {
                "protocol": args.protocol,
                "routing": args.routing,
                "bytes_per_element": args.bytes_per_element,
            },
            "hosts": hosts,
            "ranks": ranks,
        }

    out_path = Path(args.out)
    out_path.write_text(json.dumps(workload, indent=2))
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
