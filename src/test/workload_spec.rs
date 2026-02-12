use crate::sim::{
    HostSpec, RankSpec, RankStepKind, RoutingMode, SendRecvDirection, TopologySpec,
    TransportProtocol, WorkloadDefaults, WorkloadSpec,
};

#[test]
fn workload_spec_parses_minimal_json_with_defaults() {
    let raw = r#"
    {
        "schema_version": 1,
        "topology": { "kind": "dumbbell" },
        "hosts": [ { "id": 0 }, { "id": 1 } ]
    }
    "#;
    let wl: WorkloadSpec = serde_json::from_str(raw).expect("parse workload");
    assert_eq!(wl.schema_version, 1);
    assert!(matches!(wl.topology, TopologySpec::Dumbbell { .. }));
    assert_eq!(wl.hosts.len(), 2);
    assert!(wl.steps.is_empty());
    assert!(wl.ranks.is_empty());
    assert!(wl.meta.is_none());
    assert!(wl.defaults.is_none());
}

#[test]
fn workload_spec_parses_fattree_topology_and_meta_defaults() {
    let raw = r#"
    {
        "schema_version": 1,
        "meta": { "source": "neusight" },
        "topology": { "kind": "fat_tree", "k": 4, "link_gbps": 100 },
        "hosts": [ { "id": 0, "name": "h0" } ],
        "steps": [ { "id": 1, "compute_ms": 0.5 } ]
    }
    "#;
    let wl: WorkloadSpec = serde_json::from_str(raw).expect("parse workload");
    assert_eq!(wl.schema_version, 1);
    match wl.topology {
        TopologySpec::FatTree { k, link_gbps, .. } => {
            assert_eq!(k, 4);
            assert_eq!(link_gbps, Some(100));
        }
        _ => panic!("expected fat_tree topology"),
    }
    assert_eq!(
        wl.meta.as_ref().and_then(|m| m.source.as_deref()),
        Some("neusight")
    );
    assert_eq!(wl.hosts.len(), 1);
    assert_eq!(wl.hosts[0].name.as_deref(), Some("h0"));
    assert_eq!(wl.steps.len(), 1);
    assert_eq!(wl.steps[0].id, Some(1));
}

#[test]
fn workload_defaults_roundtrip_for_transport_protocol() {
    let defaults = WorkloadDefaults {
        protocol: Some(TransportProtocol::Dctcp),
        routing: None,
        bytes_per_element: Some(2),
    };

    let raw = serde_json::to_string(&defaults).expect("serialize defaults");
    let decoded: WorkloadDefaults = serde_json::from_str(&raw).expect("deserialize defaults");
    assert_eq!(decoded.protocol, Some(TransportProtocol::Dctcp));
    assert_eq!(decoded.bytes_per_element, Some(2));
}

#[test]
fn workload_defaults_parses_routing_and_protocol_snake_case() {
    let raw = r#"
    {
        "protocol": "tcp",
        "routing": "per_packet",
        "bytes_per_element": 4
    }
    "#;
    let decoded: WorkloadDefaults = serde_json::from_str(raw).expect("parse defaults");
    assert_eq!(decoded.protocol, Some(TransportProtocol::Tcp));
    assert_eq!(decoded.routing, Some(RoutingMode::PerPacket));
    assert_eq!(decoded.bytes_per_element, Some(4));
}

#[test]
fn workload_rank_step_enums_parse_snake_case() {
    let kind: RankStepKind = serde_json::from_str("\"sendrecv\"").expect("parse kind");
    assert!(matches!(kind, RankStepKind::Sendrecv));
    let kind: RankStepKind = serde_json::from_str("\"collective_wait\"").expect("parse kind");
    assert!(matches!(kind, RankStepKind::CollectiveWait));
    let dir: SendRecvDirection = serde_json::from_str("\"recv\"").expect("parse dir");
    assert!(matches!(dir, SendRecvDirection::Recv));
}

#[test]
fn workload_spec_serializes_hosts_and_ranks() {
    let wl = WorkloadSpec {
        schema_version: 1,
        meta: None,
        topology: TopologySpec::Dumbbell {
            host_link_gbps: Some(10),
            bottleneck_gbps: None,
            link_latency_us: Some(5),
        },
        defaults: None,
        hosts: vec![HostSpec {
            id: 0,
            name: None,
            topo_index: None,
            gpu: None,
        }],
        steps: Vec::new(),
        ranks: vec![RankSpec {
            id: 0,
            steps: vec![],
        }],
    };

    let raw = serde_json::to_string(&wl).expect("serialize workload");
    let decoded: WorkloadSpec = serde_json::from_str(&raw).expect("deserialize workload");
    assert_eq!(decoded.schema_version, 1);
    assert_eq!(decoded.hosts.len(), 1);
    assert_eq!(decoded.hosts[0].id, 0);
    assert_eq!(decoded.ranks.len(), 1);
    assert_eq!(decoded.ranks[0].id, 0);
}

#[test]
fn workload_rank_step_parses_comm_stream() {
    let raw = r#"
    {
        "schema_version": 2,
        "topology": { "kind": "dumbbell" },
        "hosts": [ { "id": 0 }, { "id": 1 } ],
        "ranks": [
            {
                "id": 0,
                "steps": [
                    {
                        "kind": "collective",
                        "op": "allreduce_async",
                        "comm_bytes": 123,
                        "comm_id": "c0",
                        "hosts": [0, 1],
                        "comm_stream": 7
                    }
                ]
            }
        ]
    }
    "#;
    let wl: WorkloadSpec = serde_json::from_str(raw).expect("parse workload");
    assert_eq!(wl.schema_version, 2);
    assert_eq!(wl.ranks.len(), 1);
    assert_eq!(wl.ranks[0].steps.len(), 1);
    assert_eq!(wl.ranks[0].steps[0].comm_stream, Some(7));
}
