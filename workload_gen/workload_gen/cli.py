import argparse
import json
from pathlib import Path

from .generator import generate_workload


def main() -> int:
    parser = argparse.ArgumentParser(description="workload_gen CLI")
    parser.add_argument("--model", required=True, help="Model config name")
    parser.add_argument("--gpu", required=True, help="GPU config name")
    parser.add_argument("--mode", default="train", choices=["train", "inf"])
    parser.add_argument("--seq", type=int, required=True)
    parser.add_argument("--batch", type=int, required=True)
    parser.add_argument("--dp", type=int, default=1)
    parser.add_argument("--tp", type=int, default=1)
    parser.add_argument("--pp", type=int, default=1)
    parser.add_argument("--pp-microbatch", type=int, default=1)
    parser.add_argument("--pipeline", default="1f1b", choices=["1f1b", "fwd_bwd"])
    parser.add_argument("--dtype", default="fp16", choices=["fp16", "bf16", "fp32"])
    parser.add_argument("--device", default="cuda", choices=["cuda", "cpu"])
    parser.add_argument("--warmup-steps", type=int, default=1)
    parser.add_argument("--measure-steps", type=int, default=1)
    parser.add_argument("--tp-comm-factor", type=float, default=2.0)
    parser.add_argument(
        "--device-scale-mode",
        default="max",
        choices=["max", "mean", "compute", "memory", "none"],
    )
    parser.add_argument("--model-backend", default="transformers", choices=["transformers", "minimal"])
    parser.add_argument("--repo-root", default=None)
    parser.add_argument("--model-dir", default=None)
    parser.add_argument("--device-dir", default=None)
    parser.add_argument("--out", default="workload.json")
    args = parser.parse_args()

    payload = {
        "model": args.model,
        "gpu": args.gpu,
        "mode": args.mode,
        "seq": args.seq,
        "batch": args.batch,
        "dp": args.dp,
        "tp": args.tp,
        "pp": args.pp,
        "pp_microbatch": args.pp_microbatch,
        "pipeline": args.pipeline,
        "dtype": args.dtype,
        "device": args.device,
        "warmup_steps": args.warmup_steps,
        "measure_steps": args.measure_steps,
        "tp_comm_factor": args.tp_comm_factor,
        "device_scale_mode": args.device_scale_mode,
        "model_backend": args.model_backend,
        "model_dir": args.model_dir,
        "device_dir": args.device_dir,
    }
    workload = generate_workload(payload, repo_root=args.repo_root)
    out_path = Path(args.out)
    out_path.write_text(json.dumps(workload, indent=2))
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
