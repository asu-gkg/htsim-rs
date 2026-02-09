#!/usr/bin/env python3
import argparse
import json
import os
import shlex
import subprocess
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ASPLOS_DIR = REPO_ROOT / "NeuSight" / "scripts" / "asplos"
PRED_SCRIPT = ASPLOS_DIR.parent / "pred.py"

PREDICTOR_CONFIG = {
    "neusight": {
        "predictor_path": "./data/predictor/MLP_WAVE",
        "tile_dataset_dir": "./data/dataset/train",
    },
    "micro": {
        "predictor_path": "./data/predictor/MICRO",
        "tile_dataset_dir": "",
    },
    "roofline": {
        "predictor_path": "./data/predictor/ROOFLINE",
        "tile_dataset_dir": "",
    },
    "habitat": {
        "predictor_path": "./data/predictor/HABITAT",
        "tile_dataset_dir": "./data/dataset/test",
    },
}


def log_line(message: str) -> None:
    sys.stderr.write(f"{message}\n")
    sys.stderr.flush()


def json_response(handler: BaseHTTPRequestHandler, code: int, payload: dict) -> None:
    data = json.dumps(payload).encode("utf-8")
    handler.send_response(code)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(data)))
    handler.end_headers()
    handler.wfile.write(data)


def run_prediction(payload: dict) -> dict:
    for key in ("model", "gpu", "predictor", "mode", "seq", "batch"):
        if key not in payload:
            return {"ok": False, "error": f"missing field: {key}"}

    model = str(payload["model"]).strip()
    gpu = str(payload["gpu"]).strip()
    predictor = str(payload["predictor"]).strip()
    mode = str(payload["mode"]).strip()
    try:
        seq = int(payload["seq"])
        batch = int(payload["batch"])
    except (TypeError, ValueError):
        return {"ok": False, "error": "seq/batch must be integers"}

    if not model or not gpu or not predictor or not mode:
        return {"ok": False, "error": "model/gpu/predictor/mode cannot be empty"}
    if predictor not in PREDICTOR_CONFIG:
        return {"ok": False, "error": f"unknown predictor: {predictor}"}
    options = str(payload.get("options", "")).strip()
    log_line(
        "predict request: "
        + json.dumps(
            {
                "model": model,
                "gpu": gpu,
                "predictor": predictor,
                "mode": mode,
                "seq": seq,
                "batch": batch,
                "options": options,
            },
            ensure_ascii=True,
        )
    )

    device_config = ASPLOS_DIR / "data" / "device_configs" / f"{gpu}.json"
    model_config = ASPLOS_DIR / "data" / "DLmodel_configs" / f"{model}.json"
    if not device_config.exists():
        return {"ok": False, "error": f"device config not found: {device_config}"}
    if not model_config.exists():
        return {"ok": False, "error": f"model config not found: {model_config}"}

    cfg = PREDICTOR_CONFIG[predictor]
    cmd = [
        sys.executable,
        str(PRED_SCRIPT),
        "--predictor_name",
        predictor,
        "--predictor_path",
        cfg["predictor_path"],
        "--device_config_path",
        str(device_config),
        "--model_config_path",
        str(model_config),
        "--sequence_length",
        str(seq),
        "--batch_size",
        str(batch),
        "--execution_type",
        mode,
        "--tile_dataset_dir",
        cfg["tile_dataset_dir"],
        "--result_dir",
        "./results",
        "--options",
        options,
    ]

    env = os.environ.copy()
    env.setdefault("OPENBLAS_NUM_THREADS", "1")
    local_neusight = str(REPO_ROOT / "NeuSight")
    pythonpath = env.get("PYTHONPATH", "")
    if pythonpath:
        env["PYTHONPATH"] = f"{local_neusight}:{pythonpath}"
    else:
        env["PYTHONPATH"] = local_neusight
    cuda_visible = payload.get("cuda_visible_devices")
    if cuda_visible is not None:
        env["CUDA_VISIBLE_DEVICES"] = str(cuda_visible)

    log_line(f"predict cmd: {shlex.join(cmd)}")
    log_line(f"predict cwd: {ASPLOS_DIR}")

    start = time.time()
    proc = subprocess.run(
        cmd,
        cwd=str(ASPLOS_DIR),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    elapsed_ms = int((time.time() - start) * 1000)
    log_line(f"predict exit={proc.returncode} elapsed_ms={elapsed_ms}")

    if proc.returncode != 0:
        if proc.stdout:
            sys.stderr.write(proc.stdout[-4000:] + "\n")
            sys.stderr.flush()
        return {
            "ok": False,
            "error": "prediction failed",
            "detail": proc.stdout[-4000:],
        }

    csv_name = f"{model}-{mode}-{seq}-{batch}"
    if options:
        csv_name = f"{csv_name}-{options}"
    csv_path = ASPLOS_DIR / "results" / "prediction" / gpu / predictor / f"{csv_name}.csv"
    if not csv_path.exists():
        return {"ok": False, "error": f"prediction CSV not found: {csv_path}"}

    csv_text = csv_path.read_text()
    csv_header = csv_text.splitlines()[0] if csv_text else ""
    log_line(f"predict csv: {csv_path} header={csv_header}")
    return {"ok": True, "csv": csv_text, "path": str(csv_path), "elapsed_ms": elapsed_ms}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/api/health":
            json_response(self, 200, {"ok": True})
            return
        json_response(self, 404, {"ok": False, "error": "not found"})

    def do_POST(self) -> None:
        if self.path != "/api/predict":
            json_response(self, 404, {"ok": False, "error": "not found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body.decode("utf-8"))
        except json.JSONDecodeError:
            json_response(self, 400, {"ok": False, "error": "invalid json"})
            return
        result = run_prediction(payload)
        code = 200 if result.get("ok") else 400
        json_response(self, code, result)

    def log_message(self, fmt: str, *args) -> None:
        sys.stderr.write("%s - - [%s] %s\n" % (self.client_address[0], self.log_date_time_string(), fmt % args))


def main() -> int:
    parser = argparse.ArgumentParser(description="NeuSight prediction backend")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=3100)
    args = parser.parse_args()

    if not ASPLOS_DIR.exists() or not PRED_SCRIPT.exists():
        sys.stderr.write("NeuSight scripts not found. Run from repo root.\n")
        return 1

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    sys.stderr.write(f"listening on http://{args.host}:{args.port}\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("shutdown\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
