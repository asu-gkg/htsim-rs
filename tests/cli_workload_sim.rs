use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "htsim-rs-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(dir: &PathBuf, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write temp file");
    path
}

fn count_collective_fct_lines(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| line.starts_with("collective_fct "))
        .count()
}

#[test]
fn workload_sim_writes_viz_json_with_meta_first() {
    let dir = unique_temp_dir("workload-sim-viz");
    let workload = write_file(
        &dir,
        "workload.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "dumbbell" },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "compute", "compute_ms": 0.001 } ] },
        { "id": 1, "steps": [ { "kind": "compute", "compute_ms": 0.001 } ] }
    ]
}
        "#,
    );
    let out_json = dir.join("viz.json");

    let output = Command::new(env!("CARGO_BIN_EXE_workload_sim"))
        .args([
            "--workload",
            workload.to_str().unwrap(),
            "--viz-json",
            out_json.to_str().unwrap(),
            "--until-ms",
            "0",
        ])
        .output()
        .expect("run workload_sim");
    assert!(
        output.status.success(),
        "workload_sim failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = fs::read_to_string(&out_json).expect("read viz.json");
    let v: Value = serde_json::from_str(&raw).expect("parse viz.json");
    let arr = v.as_array().expect("viz.json must be a JSON array");
    assert!(!arr.is_empty(), "viz.json should contain at least meta event");
    assert_eq!(
        arr[0].get("kind").and_then(|k| k.as_str()),
        Some("meta"),
        "expected first viz event to be meta"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn workload_sim_allows_comm_overlap_across_comm_streams() {
    let dir = unique_temp_dir("workload-sim-stream-overlap");
    let workload = write_file(
        &dir,
        "workload.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "dumbbell" },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        {
            "id": 0,
            "steps": [
                { "kind": "collective", "op": "allreduce_async", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1], "comm_stream": 0 },
                { "kind": "collective", "op": "allgather", "comm_bytes": 1, "comm_id": "c1", "hosts": [0, 1], "comm_stream": 1 }
            ]
        },
        {
            "id": 1,
            "steps": [
                { "kind": "collective", "op": "allreduce_async", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1], "comm_stream": 0 },
                { "kind": "collective", "op": "allgather", "comm_bytes": 1, "comm_id": "c1", "hosts": [0, 1], "comm_stream": 1 }
            ]
        }
    ]
}
        "#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_workload_sim"))
        .args([
            "--workload",
            workload.to_str().unwrap(),
            "--until-ms",
            "0",
            "--fct-stats",
        ])
        .output()
        .expect("run workload_sim");
    assert!(
        output.status.success(),
        "workload_sim failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#"comm_id=Some("c0")"#));
    assert!(stdout.contains(r#"comm_id=Some("c1")"#));
    assert_eq!(
        count_collective_fct_lines(&stdout),
        2,
        "expected 2 started collectives (async + overlapped comm)"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn workload_sim_serializes_comm_on_same_comm_stream() {
    let dir = unique_temp_dir("workload-sim-stream-serialize");
    let workload = write_file(
        &dir,
        "workload.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "dumbbell" },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        {
            "id": 0,
            "steps": [
                { "kind": "collective", "op": "allreduce_async", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1], "comm_stream": 0 },
                { "kind": "collective", "op": "allgather", "comm_bytes": 1, "comm_id": "c1", "hosts": [0, 1], "comm_stream": 0 }
            ]
        },
        {
            "id": 1,
            "steps": [
                { "kind": "collective", "op": "allreduce_async", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1], "comm_stream": 0 },
                { "kind": "collective", "op": "allgather", "comm_bytes": 1, "comm_id": "c1", "hosts": [0, 1], "comm_stream": 0 }
            ]
        }
    ]
}
        "#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_workload_sim"))
        .args([
            "--workload",
            workload.to_str().unwrap(),
            "--until-ms",
            "0",
            "--fct-stats",
        ])
        .output()
        .expect("run workload_sim");
    assert!(
        output.status.success(),
        "workload_sim failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#"comm_id=Some("c0")"#));
    assert!(
        !stdout.contains(r#"comm_id=Some("c1")"#),
        "expected c1 not to start while stream is busy"
    );
    assert_eq!(
        count_collective_fct_lines(&stdout),
        1,
        "expected only the async collective to start at t=0"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn workload_sim_exits_nonzero_on_comm_id_op_mismatch() {
    let dir = unique_temp_dir("workload-sim-mismatch");
    let workload = write_file(
        &dir,
        "workload.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "dumbbell" },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "collective", "op": "allreduce", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1] } ] },
        { "id": 1, "steps": [ { "kind": "collective", "op": "allgather", "comm_bytes": 1, "comm_id": "c0", "hosts": [0, 1] } ] }
    ]
}
        "#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_workload_sim"))
        .args(["--workload", workload.to_str().unwrap(), "--until-ms", "0"])
        .output()
        .expect("run workload_sim");
    assert!(
        !output.status.success(),
        "expected non-zero exit, got success"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("collective op mismatch"),
        "stderr did not contain expected message: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

