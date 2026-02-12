use crate::net::{DeliverPacket, EcmpHashMode, NetWorld, NodeId, Packet, RoutingTable};
use crate::sim::{SimTime, Simulator};
use crate::viz::{VizEventKind, VizLogger};

fn build_diamond(world: &mut NetWorld) -> (NodeId, NodeId, NodeId, NodeId, NodeId) {
    // Topology:
    // h0 -> s0 -> {s1, s2} -> s3 -> h1
    // Two equal-cost paths exist from s0 to h1 via s1 or s2.
    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    let s0 = world.net.add_switch("s0");
    let s1 = world.net.add_switch("s1");
    let s2 = world.net.add_switch("s2");
    let s3 = world.net.add_switch("s3");

    let latency = SimTime::from_micros(1);
    let bw = 100_u64 * 1_000_000_000; // 100Gbps

    world.net.connect(h0, s0, latency, bw);
    world.net.connect(s0, s1, latency, bw);
    world.net.connect(s0, s2, latency, bw);
    world.net.connect(s1, s3, latency, bw);
    world.net.connect(s2, s3, latency, bw);
    world.net.connect(s3, h1, latency, bw);

    (h0, h1, s0, s1, s2)
}

fn s0_forwards(world: &NetWorld, s0: NodeId) -> Vec<(u64, usize)> {
    let Some(v) = &world.net.viz else {
        return Vec::new();
    };
    v.events
        .iter()
        .filter_map(|ev| match &ev.kind {
            VizEventKind::NodeForward { node, next } if *node == s0.0 => Some((ev.pkt_id?, *next)),
            _ => None,
        })
        .collect()
}

#[test]
fn ecmp_hash_mode_flow_pins_packets_to_same_next_hop() {
    let mut sim = Simulator::default();
    let mut world = NetWorld::default();
    world.net.viz = Some(VizLogger::default());
    let (h0, h1, s0, s1, s2) = build_diamond(&mut world);

    world.net.set_ecmp_hash_mode(EcmpHashMode::Flow);

    let flow_id = 12345;
    let pkt0 = Packet::new_dynamic(10, flow_id, 100, h0, h1);
    let pkt1 = Packet::new_dynamic(11, flow_id, 100, h0, h1);
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt0 });
    sim.schedule(SimTime::from_micros(1), DeliverPacket { to: h0, pkt: pkt1 });
    sim.run(&mut world);

    let forwards = s0_forwards(&world, s0);
    assert_eq!(forwards.len(), 2);

    // In flow mode, both packets should pick the same ECMP next hop for a given flow_id.
    let rt = RoutingTable::new(0xC5A1_DA7A_5EED_1234);
    let cands = vec![s1, s2];
    let expected = rt.pick_ecmp_with_key(s0, h1, flow_id, &cands).0;

    for (pkt_id, next) in forwards {
        assert!(matches!(pkt_id, 10 | 11));
        assert_eq!(next, expected);
    }
}

#[test]
fn ecmp_hash_mode_packet_can_spread_packets_across_next_hops() {
    let mut sim = Simulator::default();
    let mut world = NetWorld::default();
    world.net.viz = Some(VizLogger::default());
    let (h0, h1, s0, s1, s2) = build_diamond(&mut world);

    world.net.set_ecmp_hash_mode(EcmpHashMode::Packet);

    let flow_id = 777;
    let cands = vec![s1, s2];

    // Find two packet ids that deterministically map to different next hops.
    let rt = RoutingTable::new(0xC5A1_DA7A_5EED_1234);
    let mut chosen: Option<(u64, NodeId)> = None;
    let mut ids: Option<(u64, u64, NodeId, NodeId)> = None;
    for pkt_id in 0..2048_u64 {
        let key = flow_id ^ pkt_id;
        let nh = rt.pick_ecmp_with_key(s0, h1, key, &cands);
        if let Some((first_id, first_nh)) = chosen {
            if first_nh != nh {
                ids = Some((first_id, pkt_id, first_nh, nh));
                break;
            }
        } else {
            chosen = Some((pkt_id, nh));
        }
    }
    let (pkt_id0, pkt_id1, nh0, nh1) =
        ids.expect("failed to find two packet ids with diff ECMP hop");

    let pkt0 = Packet::new_dynamic(pkt_id0, flow_id, 100, h0, h1);
    let pkt1 = Packet::new_dynamic(pkt_id1, flow_id, 100, h0, h1);
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt0 });
    sim.schedule(SimTime::from_micros(1), DeliverPacket { to: h0, pkt: pkt1 });
    sim.run(&mut world);

    let mut forwards = s0_forwards(&world, s0);
    forwards.sort_by_key(|(pkt_id, _)| *pkt_id);
    assert_eq!(forwards.len(), 2);

    assert_eq!(forwards[0].0, pkt_id0);
    assert_eq!(forwards[0].1, nh0.0);
    assert_eq!(forwards[1].0, pkt_id1);
    assert_eq!(forwards[1].1, nh1.0);
}
