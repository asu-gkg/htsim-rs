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
fn workloads_sim_prefixes_comm_id_and_label_with_tenant() {
    let dir = unique_temp_dir("workloads-sim-prefix");
    let w0 = write_file(
        &dir,
        "w0.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "fat_tree", "k": 4 },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "collective", "label": "foo", "op": "allreduce", "comm_bytes": 1, "comm_id": "c0" } ] },
        { "id": 1, "steps": [ { "kind": "collective", "label": "foo", "op": "allreduce", "comm_bytes": 1, "comm_id": "c0" } ] }
    ]
}
        "#,
    );
    let w1 = write_file(
        &dir,
        "w1.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "fat_tree", "k": 4 },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "collective", "label": "foo", "op": "allreduce", "comm_bytes": 1, "comm_id": "c0" } ] },
        { "id": 1, "steps": [ { "kind": "collective", "label": "foo", "op": "allreduce", "comm_bytes": 1, "comm_id": "c0" } ] }
    ]
}
        "#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_workloads_sim"))
        .args([
            "--workload",
            w0.to_str().unwrap(),
            "--workload",
            w1.to_str().unwrap(),
            "--until-ms",
            "0",
            "--fct-stats",
        ])
        .output()
        .expect("run workloads_sim");
    assert!(
        output.status.success(),
        "workloads_sim failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r#"comm_id=Some("t0:c0")"#));
    assert!(stdout.contains(r#"comm_id=Some("t1:c0")"#));
    assert!(stdout.contains(r#"label=Some("t0:foo")"#));
    assert!(stdout.contains(r#"label=Some("t1:foo")"#));
    assert_eq!(
        count_collective_fct_lines(&stdout),
        2,
        "expected one started collective per tenant"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn workloads_sim_exits_nonzero_on_topology_mismatch() {
    let dir = unique_temp_dir("workloads-sim-topo-mismatch");
    let w0 = write_file(
        &dir,
        "w0.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "dumbbell" },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "compute", "compute_ms": 0.001 } ] }
    ]
}
        "#,
    );
    let w1 = write_file(
        &dir,
        "w1.json",
        r#"
{
    "schema_version": 2,
    "topology": { "kind": "fat_tree", "k": 4 },
    "hosts": [ { "id": 0 }, { "id": 1 } ],
    "ranks": [
        { "id": 0, "steps": [ { "kind": "compute", "compute_ms": 0.001 } ] }
    ]
}
        "#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_workloads_sim"))
        .args(["--workload", w0.to_str().unwrap(), "--workload", w1.to_str().unwrap()])
        .output()
        .expect("run workloads_sim");
    assert!(
        !output.status.success(),
        "expected non-zero exit, got success"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("all workloads must share the same topology"),
        "stderr did not contain expected message: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

