#!/usr/bin/env python3
import argparse
import csv
import math
import shutil
import subprocess
import sys
from pathlib import Path


def default_sizes():
    return [
        10_000,
        30_000,
        100_000,
        300_000,
        1_000_000,
        3_000_000,
        10_000_000,
        30_000_000,
        100_000_000,
    ]


def run_once(msg_bytes, args):
    cmd = [
        "cargo",
        "run",
        "--quiet",
        "--bin",
        "fat_tree_allreduce_tcp",
        "--",
        "--msg-bytes",
        str(msg_bytes),
        "--stats",
        "--quiet",
        "--k",
        str(args.k),
    ]
    if args.ranks is not None:
        cmd += ["--ranks", str(args.ranks)]
    if args.routing is not None:
        cmd += ["--routing", args.routing]
    if args.link_gbps is not None:
        cmd += ["--link-gbps", str(args.link_gbps)]
    if args.link_latency_us is not None:
        cmd += ["--link-latency-us", str(args.link_latency_us)]
    if args.mss is not None:
        cmd += ["--mss", str(args.mss)]
    if args.init_cwnd_pkts is not None:
        cmd += ["--init-cwnd-pkts", str(args.init_cwnd_pkts)]
    if args.init_ssthresh_pkts is not None:
        cmd += ["--init-ssthresh-pkts", str(args.init_ssthresh_pkts)]
    if args.rto_us is not None:
        cmd += ["--rto-us", str(args.rto_us)]
    if args.max_rto_ms is not None:
        cmd += ["--max-rto-ms", str(args.max_rto_ms)]
    if args.queue_pkts is not None:
        cmd += ["--queue-pkts", str(args.queue_pkts)]

    proc = subprocess.run(cmd, check=False, text=True, capture_output=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise RuntimeError(f"command failed: {' '.join(cmd)}")
    for line in proc.stdout.splitlines():
        if line.startswith("stats "):
            parts = dict(token.split("=", 1) for token in line.split()[1:])
            makespan_ms = parts.get("makespan_ms", parts.get("fct_ms", "0"))
            return {
                "msg_bytes": int(parts["msg_bytes"]),
                "p99_fct_ms": float(parts["p99_fct_ms"]),
                "makespan_ms": float(makespan_ms),
                "max_flow_fct_ms": float(parts.get("max_flow_fct_ms", "0")),
                "slow_flow_ge_1s": int(parts.get("slow_flow_ge_1s", "0")),
                "slow_flow_ge_1s_ratio": float(parts.get("slow_flow_ge_1s_ratio", "0")),
                "flows": int(parts["flows"]),
            }
    raise RuntimeError("stats line not found in output")


def format_size_label(bytes_val):
    if bytes_val >= 1_000_000 and bytes_val % 1_000_000 == 0:
        return f"{bytes_val // 1_000_000} MB"
    if bytes_val >= 1_000 and bytes_val % 1_000 == 0:
        return f"{bytes_val // 1_000} KB"
    return str(bytes_val)


def nice_number(value):
    if value == 0:
        return "0"
    if abs(value) >= 10:
        return f"{value:.1f}".rstrip("0").rstrip(".")
    return f"{value:.2f}".rstrip("0").rstrip(".")


def write_svg(xs, ys, svg_path):
    width = 720
    height = 432
    margin_left = 80
    margin_right = 20
    margin_top = 30
    margin_bottom = 60

    plot_w = width - margin_left - margin_right
    plot_h = height - margin_top - margin_bottom

    x_min = min(xs)
    x_max = max(xs)
    log_min = math.log10(x_min)
    log_max = math.log10(x_max)

    y_min = 0.0
    y_max = max(ys) * 1.1 if max(ys) > 0 else 1.0

    def x_pos(x):
        return margin_left + (math.log10(x) - log_min) / (log_max - log_min) * plot_w

    def y_pos(y):
        return margin_top + (1.0 - (y - y_min) / (y_max - y_min)) * plot_h

    x_ticks = xs
    y_ticks = [y_min + i * (y_max - y_min) / 5 for i in range(6)]

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">',
        '<rect width="100%" height="100%" fill="white"/>',
    ]

    # Grid lines and ticks
    for xt in x_ticks:
        xp = x_pos(xt)
        lines.append(
            f'<line x1="{xp:.2f}" y1="{margin_top}" x2="{xp:.2f}" y2="{margin_top + plot_h}" '
            'stroke="#e0e0e0" stroke-width="1"/>'
        )
        lines.append(
            f'<line x1="{xp:.2f}" y1="{margin_top + plot_h}" x2="{xp:.2f}" y2="{margin_top + plot_h + 6}" '
            'stroke="#000" stroke-width="1"/>'
        )
        label = format_size_label(xt)
        lines.append(
            f'<text x="{xp:.2f}" y="{margin_top + plot_h + 22}" text-anchor="middle" '
            'font-size="12" font-family="Times New Roman">{label}</text>'
        )

    for yt in y_ticks:
        yp = y_pos(yt)
        lines.append(
            f'<line x1="{margin_left}" y1="{yp:.2f}" x2="{margin_left + plot_w}" y2="{yp:.2f}" '
            'stroke="#e0e0e0" stroke-width="1"/>'
        )
        lines.append(
            f'<line x1="{margin_left - 6}" y1="{yp:.2f}" x2="{margin_left}" y2="{yp:.2f}" '
            'stroke="#000" stroke-width="1"/>'
        )
        label = nice_number(yt)
        lines.append(
            f'<text x="{margin_left - 10}" y="{yp + 4:.2f}" text-anchor="end" '
            'font-size="12" font-family="Times New Roman">{label}</text>'
        )

    # Axes
    lines.append(
        f'<line x1="{margin_left}" y1="{margin_top}" x2="{margin_left}" y2="{margin_top + plot_h}" '
        'stroke="#000" stroke-width="1.2"/>'
    )
    lines.append(
        f'<line x1="{margin_left}" y1="{margin_top + plot_h}" x2="{margin_left + plot_w}" '
        f'y2="{margin_top + plot_h}" stroke="#000" stroke-width="1.2"/>'
    )

    # Data line
    points = " ".join(f"{x_pos(x):.2f},{y_pos(y):.2f}" for x, y in zip(xs, ys))
    lines.append(
        f'<polyline fill="none" stroke="#1f77b4" stroke-width="2" points="{points}"/>'
    )
    for x, y in zip(xs, ys):
        lines.append(
            f'<circle cx="{x_pos(x):.2f}" cy="{y_pos(y):.2f}" r="3" fill="#1f77b4"/>'
        )

    # Labels
    lines.append(
        f'<text x="{margin_left + plot_w / 2:.2f}" y="{height - 18}" text-anchor="middle" '
        'font-size="14" font-family="Times New Roman">Data size (bytes)</text>'
    )
    lines.append(
        f'<text x="18" y="{margin_top + plot_h / 2:.2f}" text-anchor="middle" '
        'font-size="14" font-family="Times New Roman" transform="rotate(-90 18 '
        f'{margin_top + plot_h / 2:.2f})">P99 FCT (ms)</text>'
    )

    lines.append("</svg>")
    svg_path.write_text("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description="Sweep TCP allreduce sizes and plot p99 FCT.")
    parser.add_argument("--out", default="allreduce_tcp_p99_fct.pdf", help="output PDF path")
    parser.add_argument("--csv", default="allreduce_tcp_p99_fct.csv", help="output CSV path")
    parser.add_argument("--k", type=int, default=4)
    parser.add_argument("--ranks", type=int)
    parser.add_argument("--routing", choices=["per-flow", "per-packet"])
    parser.add_argument("--link-gbps", type=int)
    parser.add_argument("--link-latency-us", type=int)
    parser.add_argument("--mss", type=int)
    parser.add_argument("--init-cwnd-pkts", type=int)
    parser.add_argument("--init-ssthresh-pkts", type=int)
    parser.add_argument("--rto-us", type=int)
    parser.add_argument("--max-rto-ms", type=int)
    parser.add_argument("--queue-pkts", type=int)
    parser.add_argument("--sizes", nargs="*", type=int, help="message sizes in bytes")
    args = parser.parse_args()

    sizes = args.sizes if args.sizes else default_sizes()
    results = []
    for size in sizes:
        res = run_once(size, args)
        results.append(res)
        print(f"size={size}B p99_fct_ms={res['p99_fct_ms']:.6f}")

    with open(args.csv, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(
            [
                "msg_bytes",
                "p99_fct_ms",
                "makespan_ms",
                "max_flow_fct_ms",
                "slow_flow_ge_1s",
                "slow_flow_ge_1s_ratio",
                "flows",
            ]
        )
        for row in results:
            writer.writerow(
                [
                    row["msg_bytes"],
                    row["p99_fct_ms"],
                    row["makespan_ms"],
                    row["max_flow_fct_ms"],
                    row["slow_flow_ge_1s"],
                    row["slow_flow_ge_1s_ratio"],
                    row["flows"],
                ]
            )

    xs = [row["msg_bytes"] for row in results]
    ys = [row["p99_fct_ms"] for row in results]

    svg_path = Path(args.out).with_suffix(".svg")
    write_svg(xs, ys, svg_path)

    if shutil.which("rsvg-convert") is None:
        raise RuntimeError("rsvg-convert not found; cannot render PDF")
    subprocess.run(
        ["rsvg-convert", "-f", "pdf", "-o", args.out, str(svg_path)],
        check=True,
        text=True,
    )

    print(f"wrote {args.csv}")
    print(f"wrote {svg_path}")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
