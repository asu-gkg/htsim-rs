from typing import List, Tuple

try:
    from transformers import AutoConfig, AutoModel
except Exception:  # pragma: no cover - optional dependency
    AutoConfig = None
    AutoModel = None

import torch


def _get_attr_path(obj, path: str):
    current = obj
    for part in path.split("."):
        if not hasattr(current, part):
            return None
        current = getattr(current, part)
    return current


def _resolve_layers(model) -> List[Tuple[str, torch.nn.Module]]:
    candidates = [
        "encoder.layer",
        "h",
        "decoder.layers",
        "transformer.h",
        "model.layers",
        "layers",
    ]
    for path in candidates:
        layers = _get_attr_path(model, path)
        if isinstance(layers, torch.nn.ModuleList):
            return [(f"{path}.{idx}", layer) for idx, layer in enumerate(layers)]
    raise ValueError("unable to locate transformer layers in model")


def build_transformers_model(config_path: str):
    if AutoConfig is None or AutoModel is None:
        raise ImportError("transformers is not installed")

    cfg = AutoConfig.from_pretrained(config_path)
    if hasattr(cfg, "use_cache"):
        cfg.use_cache = False
    model = AutoModel.from_config(cfg)

    layer_modules = _resolve_layers(model)
    prologue_modules = []
    epilogue_modules = []
    if hasattr(model, "embeddings"):
        prologue_modules = [("embeddings", model.embeddings)]
    if hasattr(model, "layernorm"):
        epilogue_modules = [("layernorm", model.layernorm)]
    if hasattr(model, "ln_f"):
        epilogue_modules = [("ln_f", model.ln_f)]

    return model, layer_modules, prologue_modules, epilogue_modules
