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


def extract_comm_elems(ops):
    total = 0
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
                total += int(size)
    return total


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


def rows_to_steps(rows, hosts, bytes_per_element):
    steps = []
    compute_ms = 0.0
    for row in rows:
        fw_ops = parse_ops(row.get("FwOps", ""))
        bw_ops = parse_ops(row.get("BwOps", ""))
        comm_elems = extract_comm_elems(fw_ops) + extract_comm_elems(bw_ops)
        if comm_elems > 0:
            comm_bytes = comm_elems * bytes_per_element
            steps.append(
                {
                    "id": len(steps),
                    "label": row.get("Name") or None,
                    "hosts": hosts,
                    "compute_ms": round(compute_ms, 6),
                    "comm_bytes": comm_bytes,
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
    parser.add_argument("--hosts", type=int, required=True, help="Number of hosts / ranks")
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
            rows.append(row)

    model_name = find_model_from_name(pred_path.name)
    gpu_name = args.gpu or find_device_from_path(pred_path)

    num_layers = args.num_layers
    if num_layers is None and args.summary_json:
        with Path(args.summary_json).open() as f:
            summary = json.load(f)
        num_layers = int(summary.get("num_layer") or 0) or None

    if num_layers:
        rows = replicate_layers(rows, model_name, num_layers)

    host_ids = list(range(args.hosts))
    steps = rows_to_steps(rows, host_ids, args.bytes_per_element)

    topo_kind = args.topo_kind
    if topo_kind is None:
        topo_kind = "dumbbell" if args.hosts == 2 else "fat_tree"

    if topo_kind == "fat_tree":
        k = args.k or infer_fat_tree_k(args.hosts)
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

    out_path = Path(args.out)
    out_path.write_text(json.dumps(workload, indent=2))
    print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
