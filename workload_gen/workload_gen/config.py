import json
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional


@dataclass
class ModelSpec:
    name: str
    model_type: str
    num_layers: int
    hidden_size: int
    num_heads: int
    ffn_hidden_size: int
    vocab_size: int
    max_position: int

    @staticmethod
    def from_config(name: str, cfg: Dict) -> "ModelSpec":
        def pick_int(keys, default=0):
            for key in keys:
                value = cfg.get(key)
                if isinstance(value, (int, float)) and value > 0:
                    return int(value)
            return int(default)

        model_type = str(cfg.get("model_type") or name).lower()
        num_layers = pick_int(["n_layer", "num_hidden_layers", "num_layers"])
        hidden_size = pick_int(["hidden_size", "n_embd"])
        num_heads = pick_int(["num_attention_heads", "n_head"])
        ffn_hidden_size = pick_int(["intermediate_size", "ffn_dim"], default=hidden_size * 4)
        vocab_size = pick_int(["vocab_size"], default=50257)
        max_position = pick_int(["n_ctx", "n_positions", "max_position_embeddings"], default=2048)
        if not num_layers or not hidden_size or not num_heads:
            raise ValueError(f"incomplete model config for {name}")
        return ModelSpec(
            name=name,
            model_type=model_type,
            num_layers=num_layers,
            hidden_size=hidden_size,
            num_heads=num_heads,
            ffn_hidden_size=ffn_hidden_size,
            vocab_size=vocab_size,
            max_position=max_position,
        )


def resolve_repo_root(repo_root: Optional[str]) -> Path:
    if repo_root:
        return Path(repo_root).resolve()
    return Path(__file__).resolve().parents[2]


def default_model_dir(repo_root: Path) -> Path:
    return repo_root / "NeuSight" / "scripts" / "asplos" / "data" / "DLmodel_configs"


def default_device_dir(repo_root: Path) -> Path:
    return repo_root / "NeuSight" / "scripts" / "asplos" / "data" / "device_configs"


def load_json(path: Path) -> Dict:
    with path.open() as f:
        return json.load(f)


def list_model_names(model_dir: Path) -> List[str]:
    if not model_dir.exists():
        return []
    return sorted([p.stem for p in model_dir.glob("*.json")])


def list_gpu_names(device_dir: Path) -> List[str]:
    if not device_dir.exists():
        return []
    return sorted([p.stem for p in device_dir.glob("*.json")])


def load_model_config(model: str, model_dir: Path) -> ModelSpec:
    path = model_dir / f"{model}.json"
    if not path.exists():
        raise FileNotFoundError(f"model config not found: {path}")
    cfg = load_json(path)
    return ModelSpec.from_config(model, cfg)


def load_device_config(gpu: str, device_dir: Path) -> Dict:
    path = device_dir / f"{gpu}.json"
    if not path.exists():
        raise FileNotFoundError(f"device config not found: {path}")
    return load_json(path)
