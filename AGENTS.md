# Repository Guidelines

## Project Structure & Module Organization
- `src/lib.rs` is the crate root; core modules live under `src/sim/`, `src/net/`, `src/proto/`, `src/queue/`, and `src/viz/`.
- `src/topo/` holds reusable topology builders (dumbbell/fat-tree).
- `src/bin/` contains runnable binaries: `dumbbell`, `dumbbell_tcp`, `dumbbell_dctcp`, `fat_tree`, `trace_single_packet`.
- `tools/viz/index.html` replays visualization JSON emitted by `--viz-json`.
- `tutorial.md` provides a deeper walkthrough; `parse_logs.py` formats tracing output.

## Build, Test, and Development Commands
- `cargo build` compiles the library and binaries.
- `cargo run --bin dumbbell -- --pkts 10000 --until-ms 50` runs the basic dumbbell sim.
- `cargo run --bin trace_single_packet` runs the single packet trace.
- `cargo run --bin dumbbell_tcp -- --viz-json out.json` runs TCP dumbbell and writes viz events for `tools/viz/index.html`.
- `cargo test` runs unit tests.

## Coding Style & Naming Conventions
- Rust 2024 edition; format with `cargo fmt` (default rustfmt settings).
- Indent with 4 spaces; keep modules focused and follow existing folder boundaries.
- Naming: `CamelCase` types, `snake_case` functions/vars, `SCREAMING_SNAKE_CASE` consts.
- Prefer `tracing` macros and keep `#[tracing::instrument]` on public simulation entry points.

## Testing Guidelines
- Tests live near the code under `#[cfg(test)]` (e.g., `src/net/packet.rs`).
- Use descriptive `snake_case` test names.
- Add tests for routing decisions, queue behavior, and protocol edge cases when changing those areas.

## Commit & Pull Request Guidelines
- Commit messages are short, lowercase, verb-first (e.g., `impl ecmp`).
- PRs should include a short rationale, commands run (e.g., `cargo test`, `cargo run --bin dumbbell ...`), and note any new CLI flags or output format changes.
- If logs or visualization output change, mention the expected output file or snippet.

## Logging & Debugging
- Enable detailed logs with `RUST_LOG=debug` or `RUST_LOG=trace`.
- Example: `RUST_LOG=trace cargo run --bin trace_single_packet 2>&1 | python3 parse_logs.py`.
