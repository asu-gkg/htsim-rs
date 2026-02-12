# htsim-rs: Missing Pieces + Test Plan Notes

This file is a developer note: what is still *not* implemented (or only roughly modeled) in the
current repo, and what tests we should add once those features land.

## What We Have Tests For (Current)

Rust unit tests live under `src/test/` and are wired via `src/lib.rs`.

Covered areas include:
- `sim`: event ordering semantics (`schedule` seq ordering), `run_until` behavior, `SimTime` unit conversions.
- `cc`: collective op parsing + step/chunk sizing; ring collectives correctness properties (pairs, flow ids, step timing, callbacks).
- `net`: `Packet` routing helpers (preset/mixed/dynamic), ECN helpers.
- `queue`: DropTail and PriorityQueue behavior (capacity, ordering, accounting).
- `net::RoutingTable`: next-hop computation + deterministic ECMP selection.
- `sim::WorkloadSpec`: serde parsing/roundtrip for schema fields and snake_case enums.

## Things That Are Still Missing / Not Implemented

### 1) Compute/Comm Overlap (SimAI-style overlap)

Workload simulation now supports a basic async pattern:
- ops with `op` ending in `*_async` are launched non-blocking;
- ranks can continue issuing *compute* while async collectives run;
- `collective_wait` (rank step kind) blocks until all outstanding async collectives complete.

What is still missing vs “full SimAI overlap”:
- multiple independent comm streams (e.g., allow comm-vs-comm overlap with ordering constraints),
- partial overlap ratios (e.g., only expose `x%` of comm),
- pipelined ring algorithms (chunk-level pipeline overlap across steps).

Also note: we currently *serialize comm issuance* per rank while any async collective is in flight
(i.e., only compute overlaps; other comm steps wait).

### 2) Async Collective Semantics (`*_ASYNC`)

Implemented in `workload_sim`/`workloads_sim`:
- `*_async` collectives increment a per-rank “pending async” counter and return immediately.
- Completion decrements the counter and wakes ranks blocked on `collective_wait` (or end-of-rank).

Open questions:
- Should we allow ranks to issue further comm steps while async collectives are pending?
- Do we need per-rank comm-stream queues (issue order vs completion)?

### 3) Collective Modeling Fidelity (Ring-only, Step-barriered)

We currently model collectives as:
- `total_steps` steps,
- per-step: each rank starts exactly one flow of `chunk_bytes`,
- next step only starts after the slowest flow in the current step completes.

What is missing if we care about realism:
- pipelined ring (multiple chunks in flight across the ring, not strict step barriers),
- topology-aware algorithms (tree/hierarchical allreduce for fat-tree),
- link-level scheduling effects for collective-specific traffic patterns.

### 4) `alltoall` `comm_bytes` Semantics (Network bytes vs total buffer)

We currently interpret `alltoall.comm_bytes` as:
"per-rank total buffer size, including the self portion",
and compute `chunk_bytes = ceil(comm_bytes / ranks)`.

If `comm_bytes` is supposed to mean "pure network bytes (excluding self)",
then `chunk_bytes` and/or the accounting needs to change.

This needs a concrete decision based on the NeuSight output semantics you want.

### 5) End-to-End Tests for CLI Binaries / Output Formats

We do not have stable integration tests for:
- `src/bin/workload_sim.rs` / `src/bin/workloads_sim.rs` outputs (CSV/JSON, field names, invariants),
- viz JSON event correctness (meta + event sequences),
- regression tests for output renames (e.g., `collective_fct_*` naming).

Most current tests are unit/property tests, not golden-file or CLI-level tests.

### 6) Protocol/Network Model Completeness (By Design Simplifications)

These are not "bugs", but gaps vs real stacks:
- TCP/DCTCP are simplified (no SACK, simplified RTO/fast-recovery behavior, etc.).
- Queue disciplines are limited (DropTail + strict priority only).
- Routing is shortest-hop with ECMP next-hop sets; no weighted costs or failure models.

If we need correctness against a reference simulator, these need validation/calibration.

## Tests To Add Once We Implement Overlap

### A) Unit tests for overlap scheduler

When overlap exists, tests should assert:
- non-blocking collectives allow rank progress without waiting for comm completion;
- blocking collectives still behave as barriers;
- overlap policy is explicit and deterministic (e.g., `max(compute, comm)` vs partial overlap rules).

Suggested minimal tests:
- single-rank: async comm should be a no-op and not deadlock;
- 2 ranks: async allreduce starts, next compute step begins immediately, final barrier waits.

### B) Workload-level tests (small JSON fixtures)

Add small `WorkloadSpec` fixtures and test invariants:
- completion time expectations under different overlap modes,
- per-step ordering constraints (e.g., comm_id groups must synchronize),
- `comm_id` mismatches should error loudly (not silently skip).

These may require refactoring `workload_sim` logic into library functions so unit tests
can run without spawning binaries.

### C) Viz snapshot tests (if we want UI regression protection)

If viz JSON is part of the contract:
- snapshot the `VizEventKind::Meta` + a short event trace for a tiny topology,
- assert required fields exist and ordering invariants hold.

## Open Decisions

- Define overlap rules (what can overlap with what, and how dependencies are expressed).
- Confirm NeuSight `comm_bytes` semantics for `alltoall` (include self or not).
- Decide whether `ranks=0` should be treated as invalid input (error) or as a no-op.
