from typing import List


def rank_for(dp_idx: int, pp_idx: int, tp_idx: int, dp_degree: int, pp_degree: int, tp_degree: int) -> int:
    return (dp_idx * pp_degree + pp_idx) * tp_degree + tp_idx


def build_rank_map(dp_degree: int, pp_degree: int, tp_degree: int) -> List[dict]:
    ranks = []
    for dp_idx in range(dp_degree):
        for pp_idx in range(pp_degree):
            for tp_idx in range(tp_degree):
                rank = rank_for(dp_idx, pp_idx, tp_idx, dp_degree, pp_degree, tp_degree)
                ranks.append({"id": rank, "dp": dp_idx, "pp": pp_idx, "tp": tp_idx})
    return ranks


def build_rank_steps(
    rank_info: dict,
    stage_stats: List[dict],
    dp_degree: int,
    pp_degree: int,
    tp_degree: int,
    microbatches: int,
    pipeline: str,
    mode: str,
) -> List[dict]:
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

    def pp_comm_id(direction: str, src_stage: int, microbatch: int) -> str:
        return f"pp-{direction}-s{src_stage}-mb{microbatch}-dp{dp_idx}-tp{tp_idx}"

    def tp_comm_id(direction: str, microbatch: int) -> str:
        return f"tp-{direction}-pp{pp_idx}-dp{dp_idx}-mb{microbatch}"

    def dp_comm_id(direction: str, microbatch: int) -> str:
        return f"dp-{direction}-pp{pp_idx}-tp{tp_idx}-mb{microbatch}"

    def add_compute(label: str, ms: float) -> None:
        if ms <= 0:
            return
        steps.append({"kind": "compute", "label": label, "compute_ms": round(ms, 6)})

    def add_collective(label: str, op: str, comm_bytes: int, hosts: List[int], comm_id: str) -> None:
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

    def add_sendrecv(label: str, comm_bytes: int, peer: int, direction: str, comm_id: str) -> None:
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

    def forward_step(microbatch: int) -> None:
        if prev_rank is not None:
            add_sendrecv(
                f"fwd_recv_mb{microbatch}",
                stats["pp_bytes"],
                prev_rank,
                "recv",
                pp_comm_id("fwd", pp_idx - 1, microbatch),
            )
        add_compute(f"fwd_mb{microbatch}", stats["fw_compute_ms"])
        add_collective(
            f"tp_fwd_mb{microbatch}",
            "allreduce",
            stats["tp_fw_bytes"],
            tp_group,
            tp_comm_id("fwd", microbatch),
        )
        if next_rank is not None:
            add_sendrecv(
                f"fwd_send_mb{microbatch}",
                stats["pp_bytes"],
                next_rank,
                "send",
                pp_comm_id("fwd", pp_idx, microbatch),
            )

    def backward_step(microbatch: int) -> None:
        if next_rank is not None:
            add_sendrecv(
                f"bwd_recv_mb{microbatch}",
                stats["pp_bytes"],
                next_rank,
                "recv",
                pp_comm_id("bwd", pp_idx + 1, microbatch),
            )
        add_compute(f"bwd_mb{microbatch}", stats["bw_compute_ms"])
        add_collective(
            f"tp_bwd_mb{microbatch}",
            "allreduce",
            stats["tp_bw_bytes"],
            tp_group,
            tp_comm_id("bwd", microbatch),
        )
        add_collective(
            f"dp_bwd_mb{microbatch}",
            "allreduce",
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

    if mode == "inf":
        for microbatch in range(microbatches):
            forward_step(microbatch)
    elif pipeline == "fwd_bwd":
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

    for idx, step in enumerate(steps):
        step["id"] = idx
    return steps
