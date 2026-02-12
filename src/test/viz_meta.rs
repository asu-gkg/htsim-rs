use crate::net::NetWorld;
use crate::sim::SimTime;
use crate::viz::{VizEventKind, VizLogger, VizNodeKind};
use std::collections::HashMap;

#[test]
fn viz_meta_includes_nodes_links_and_queue_caps() {
    let mut world = NetWorld::default();
    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");

    let latency = SimTime::from_micros(2);
    let bw = 10_u64 * 1_000_000_000;

    world.net.connect(h0, h1, latency, bw);
    world.net.connect(h1, h0, latency, bw);
    world.net.set_link_queue_capacity_bytes(h0, h1, 111);
    world.net.set_link_queue_capacity_bytes(h1, h0, 222);

    world.net.viz = Some(VizLogger::default());
    world.net.emit_viz_meta();

    let events = &world.net.viz.as_ref().expect("viz enabled").events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].t_ns, 0);

    let (nodes, links) = match &events[0].kind {
        VizEventKind::Meta { nodes, links } => (nodes, links),
        _ => panic!("expected Meta event"),
    };

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].id, h0.0);
    assert_eq!(nodes[0].name, "h0");
    assert!(matches!(nodes[0].kind, VizNodeKind::Host));
    assert_eq!(nodes[1].id, h1.0);
    assert_eq!(nodes[1].name, "h1");
    assert!(matches!(nodes[1].kind, VizNodeKind::Host));

    let by_pair = links
        .iter()
        .map(|l| ((l.from, l.to), l))
        .collect::<HashMap<_, _>>();
    let l01 = by_pair.get(&(h0.0, h1.0)).expect("missing h0->h1");
    assert_eq!(l01.bandwidth_bps, bw);
    assert_eq!(l01.latency_ns, latency.0);
    assert_eq!(l01.q_cap_bytes, 111);

    let l10 = by_pair.get(&(h1.0, h0.0)).expect("missing h1->h0");
    assert_eq!(l10.bandwidth_bps, bw);
    assert_eq!(l10.latency_ns, latency.0);
    assert_eq!(l10.q_cap_bytes, 222);
}

#[test]
fn viz_meta_reflects_host_vs_switch_egress_queue_overrides() {
    let mut world = NetWorld::default();
    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    let s0 = world.net.add_switch("s0");

    let latency = SimTime::from_micros(1);
    let bw = 100_u64 * 1_000_000_000;

    // h0 <-> s0 <-> h1
    world.net.connect(h0, s0, latency, bw);
    world.net.connect(s0, h0, latency, bw);
    world.net.connect(s0, h1, latency, bw);
    world.net.connect(h1, s0, latency, bw);

    world.net.set_host_egress_queue_capacity_bytes(111);
    world.net.set_switch_egress_queue_capacity_bytes(222);

    world.net.viz = Some(VizLogger::default());
    world.net.emit_viz_meta();

    let events = &world.net.viz.as_ref().expect("viz enabled").events;
    let links = match &events[0].kind {
        VizEventKind::Meta { links, .. } => links,
        _ => panic!("expected Meta event"),
    };

    let by_pair = links
        .iter()
        .map(|l| ((l.from, l.to), l.q_cap_bytes))
        .collect::<HashMap<_, _>>();

    // Host egress links use host capacity.
    assert_eq!(*by_pair.get(&(h0.0, s0.0)).unwrap(), 111);
    assert_eq!(*by_pair.get(&(h1.0, s0.0)).unwrap(), 111);

    // Switch egress links use switch capacity.
    assert_eq!(*by_pair.get(&(s0.0, h0.0)).unwrap(), 222);
    assert_eq!(*by_pair.get(&(s0.0, h1.0)).unwrap(), 222);
}
