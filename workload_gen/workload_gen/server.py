import argparse
import json
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from .config import default_device_dir, default_model_dir, list_gpu_names, list_model_names, resolve_repo_root
from .generator import generate_workload


def _json_response(handler: BaseHTTPRequestHandler, code: int, payload: dict) -> None:
    data = json.dumps(payload).encode("utf-8")
    handler.send_response(code)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(data)))
    handler.send_header("Access-Control-Allow-Origin", "*")
    handler.end_headers()
    handler.wfile.write(data)


def _slugify(value: str) -> str:
    value = re.sub(r"\s+", "_", value.strip())
    return re.sub(r"[^A-Za-z0-9._-]", "_", value)


def _save_workload(workload: dict, repo_root) -> str:
    if repo_root:
        base_dir = Path(repo_root) / "viz" / "workloads"
    else:
        base_dir = Path(__file__).resolve().parents[1] / "outputs"
    base_dir.mkdir(parents=True, exist_ok=True)
    meta = workload.get("meta") or {}
    model = _slugify(str(meta.get("model") or "model"))
    mode = _slugify(str(meta.get("profile", {}).get("mode") or "mode"))
    seq = meta.get("profile", {}).get("seq") or 0
    batch = meta.get("profile", {}).get("batch") or 0
    parallel = meta.get("parallel") or {}
    dp = parallel.get("dp") or 1
    tp = parallel.get("tp") or 1
    pp = parallel.get("pp") or 1
    stamp = int(time.time() * 1000)
    filename = f"{model}-{mode}-seq{seq}-bs{batch}-dp{dp}-tp{tp}-pp{pp}-{stamp}.json"
    path = base_dir / filename
    path.write_text(json.dumps(workload, indent=2))
    return str(path)


class Handler(BaseHTTPRequestHandler):
    config = {
        "repo_root": None,
        "model_dir": None,
        "device_dir": None,
    }

    def do_GET(self) -> None:
        if self.path == "/api/health":
            _json_response(self, 200, {"ok": True})
            return
        if self.path == "/api/models":
            _json_response(self, 200, {"ok": True, "models": list_model_names(self.config["model_dir"])})
            return
        if self.path == "/api/gpus":
            _json_response(self, 200, {"ok": True, "gpus": list_gpu_names(self.config["device_dir"])})
            return
        _json_response(self, 404, {"ok": False, "error": "not found"})

    def do_OPTIONS(self) -> None:
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_POST(self) -> None:
        if self.path != "/api/workload":
            _json_response(self, 404, {"ok": False, "error": "not found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body.decode("utf-8"))
        except json.JSONDecodeError:
            _json_response(self, 400, {"ok": False, "error": "invalid json"})
            return
        start = time.time()
        try:
            workload = generate_workload(payload, repo_root=self.config["repo_root"])
        except Exception as exc:
            _json_response(self, 400, {"ok": False, "error": str(exc)})
            return
        path = _save_workload(workload, self.config["repo_root"])
        if "meta" not in workload or workload["meta"] is None:
            workload["meta"] = {}
        workload["meta"]["source"] = path
        elapsed_ms = int((time.time() - start) * 1000)
        _json_response(
            self,
            200,
            {"ok": True, "elapsed_ms": elapsed_ms, "path": path, "workload": workload},
        )

    def log_message(self, fmt: str, *args) -> None:
        sys.stderr.write("%s - - [%s] %s\n" % (self.client_address[0], self.log_date_time_string(), fmt % args))


def main() -> int:
    parser = argparse.ArgumentParser(description="workload_gen API server")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=3100)
    parser.add_argument("--repo-root", default=None)
    parser.add_argument("--model-dir", default=None)
    parser.add_argument("--device-dir", default=None)
    args = parser.parse_args()

    repo_root = resolve_repo_root(args.repo_root)
    model_dir = Path(args.model_dir) if args.model_dir else default_model_dir(repo_root)
    device_dir = Path(args.device_dir) if args.device_dir else default_device_dir(repo_root)

    Handler.config = {
        "repo_root": str(repo_root),
        "model_dir": model_dir,
        "device_dir": device_dir,
    }

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    sys.stderr.write(f"listening on http://{args.host}:{args.port}\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("shutdown\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
