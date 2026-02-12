use crate::net::{DeliverPacket, NetWorld, Packet};
use crate::sim::{SimTime, Simulator};
use crate::topo::dumbbell::{DumbbellOpts, build_dumbbell};
use crate::topo::fat_tree::{FatTreeOpts, build_fat_tree};
use std::collections::HashSet;

#[test]
fn dumbbell_route_delivers_packet_end_to_end() {
    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let opts = DumbbellOpts::default();
    let (h0, _h1, route) = build_dumbbell(&mut world, &opts);

    let pkt = world.net.make_packet(1, 100, route);
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt });
    sim.run(&mut world);

    assert_eq!(world.net.stats.dropped_pkts, 0);
    assert_eq!(world.net.stats.delivered_pkts, 1);
}

#[test]
fn fat_tree_counts_indexing_and_ecmp_variation() {
    let mut world = NetWorld::default();
    let opts = FatTreeOpts {
        k: 4,
        link_gbps: 100,
        link_latency: SimTime::from_micros(1),
    };
    let topo = build_fat_tree(&mut world, &opts);

    let half = opts.k / 2;
    assert_eq!(topo.hosts.len(), opts.k * half * half);
    assert_eq!(topo.edge_switches.len(), opts.k * half);
    assert_eq!(topo.agg_switches.len(), opts.k * half);
    assert_eq!(topo.core_switches.len(), half * half);

    let mut seen_hosts = HashSet::new();
    for pod in 0..opts.k {
        for edge in 0..half {
            for host in 0..half {
                let hid = topo.host(pod, edge, host);
                assert!(seen_hosts.insert(hid), "duplicate host id {hid:?}");
            }
        }
    }
    assert_eq!(seen_hosts.len(), topo.hosts.len());

    // Pick an inter-pod pair so shortest paths have ECMP choices.
    let src = topo.host(0, 0, 0);
    let dst = topo.host(1, 0, 0);

    let mut seen_src_aggs = HashSet::new();
    for flow_id in 0..512_u64 {
        let path = world.net.route_ecmp_path(src, dst, flow_id);
        assert_eq!(path.first().copied(), Some(src));
        assert_eq!(path.last().copied(), Some(dst));
        assert!(
            path.len() >= 3,
            "unexpectedly short path for inter-pod traffic: {path:?}"
        );
        // src_host -> src_edge -> (ECMP) src_agg -> ...
        seen_src_aggs.insert(path[2]);
    }
    assert!(
        seen_src_aggs.len() > 1,
        "expected ECMP to choose multiple src_agg next-hops for inter-pod traffic"
    );

    // Sanity: the chosen route should be traversable end-to-end.
    let path = world.net.route_ecmp_path(src, dst, 0);
    let pkt = Packet::new_preset(1, 1, 100, path);
    let mut sim = Simulator::default();
    sim.schedule(SimTime::ZERO, DeliverPacket { to: src, pkt });
    sim.run(&mut world);
    assert_eq!(world.net.stats.dropped_pkts, 0);
    assert!(world.net.stats.delivered_pkts > 0);
}

#[test]
fn fat_tree_shortest_paths_have_expected_lengths_and_core_usage() {
    let mut world = NetWorld::default();
    let opts = FatTreeOpts {
        k: 4,
        link_gbps: 100,
        link_latency: SimTime::from_micros(1),
    };
    let topo = build_fat_tree(&mut world, &opts);

    let src = topo.host(0, 0, 0);
    let dst_same_edge = topo.host(0, 0, 1);
    let dst_same_pod = topo.host(0, 1, 0);
    let dst_diff_pod = topo.host(1, 0, 0);

    let core = topo.core_switches.iter().copied().collect::<HashSet<_>>();

    let p_same_edge = world.net.route_ecmp_path(src, dst_same_edge, 1);
    assert_eq!(p_same_edge.len(), 3, "same-edge path: {p_same_edge:?}");
    assert!(
        p_same_edge.iter().all(|n| !core.contains(n)),
        "same-edge path should not traverse core: {p_same_edge:?}"
    );

    let p_same_pod = world.net.route_ecmp_path(src, dst_same_pod, 2);
    assert_eq!(p_same_pod.len(), 5, "same-pod path: {p_same_pod:?}");
    assert!(
        p_same_pod.iter().all(|n| !core.contains(n)),
        "same-pod path should not traverse core: {p_same_pod:?}"
    );

    let p_diff_pod = world.net.route_ecmp_path(src, dst_diff_pod, 3);
    assert_eq!(p_diff_pod.len(), 7, "diff-pod path: {p_diff_pod:?}");
    assert!(
        p_diff_pod.iter().any(|n| core.contains(n)),
        "diff-pod path should traverse core: {p_diff_pod:?}"
    );
}
