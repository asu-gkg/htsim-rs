use crate::net::{DeliverPacket, NetWorld, NodeId, Packet, TcpSegment, Transport};
use crate::sim::{Event, SimTime, Simulator, World};
use crate::viz::{VizEventKind, VizLogger};

fn expected_tx_time_ns(bytes: u32, bandwidth_bps: u64) -> u64 {
    if bandwidth_bps == 0 {
        return u64::MAX / 4;
    }
    let bits = (bytes as u128).saturating_mul(8);
    let nanos = (bits.saturating_mul(1_000_000_000u128) + (bandwidth_bps as u128 - 1))
        / bandwidth_bps as u128;
    nanos.min(u64::MAX as u128) as u64
}

fn tx_start_events(world: &NetWorld, from: NodeId, to: NodeId) -> Vec<(u64, u64, u64, u64)> {
    let Some(v) = &world.net.viz else {
        return Vec::new();
    };
    v.events
        .iter()
        .filter_map(|ev| match &ev.kind {
            VizEventKind::TxStart {
                link_from,
                link_to,
                depart_ns,
                arrive_ns,
            } if *link_from == from.0 && *link_to == to.0 => {
                Some((ev.t_ns, ev.pkt_id?, *depart_ns, *arrive_ns))
            }
            _ => None,
        })
        .collect()
}

fn drop_events(world: &NetWorld, from: NodeId, to: NodeId) -> Vec<(u64, u64, u64)> {
    let Some(v) = &world.net.viz else {
        return Vec::new();
    };
    v.events
        .iter()
        .filter_map(|ev| match &ev.kind {
            VizEventKind::Drop {
                link_from,
                link_to,
                q_cap_bytes,
                ..
            } if *link_from == from.0 && *link_to == to.0 => {
                Some((ev.t_ns, ev.pkt_id?, *q_cap_bytes))
            }
            _ => None,
        })
        .collect()
}

struct ScheduleDeliver {
    at: SimTime,
    to: NodeId,
    pkt: Packet,
}

impl Event for ScheduleDeliver {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        sim.schedule(
            self.at,
            DeliverPacket {
                to: self.to,
                pkt: self.pkt,
            },
        );
    }
}

fn build_two_host_link(latency: SimTime, bandwidth_bps: u64) -> (NetWorld, NodeId, NodeId) {
    let mut world = NetWorld::default();
    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    world.net.connect(h0, h1, latency, bandwidth_bps);
    world.net.viz = Some(VizLogger::default());
    (world, h0, h1)
}

#[test]
fn link_serializes_packets_and_spaces_tx_starts() {
    let latency = SimTime(1000); // 1us
    let bw = 1_000_000_000; // 1Gbps
    let bytes = 1000_u32;
    let tx_ns = expected_tx_time_ns(bytes, bw);

    let mut sim = Simulator::default();
    let (mut world, h0, h1) = build_two_host_link(latency, bw);

    let pkt0 = Packet::new_dynamic(10, 1, bytes, h0, h1);
    let pkt1 = Packet::new_dynamic(11, 1, bytes, h0, h1);
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt0 });
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt1 });
    sim.run(&mut world);

    assert_eq!(world.net.stats.dropped_pkts, 0);
    assert_eq!(world.net.stats.delivered_pkts, 2);
    assert_eq!(world.net.stats.delivered_bytes, (bytes as u64) * 2);

    let mut starts = tx_start_events(&world, h0, h1);
    starts.sort_by_key(|(t_ns, _, _, _)| *t_ns);
    assert_eq!(starts.len(), 2);

    // First packet starts at 0, finishes tx at tx_ns, arrives after latency.
    assert_eq!(starts[0].0, 0);
    assert_eq!(starts[0].1, 10);
    assert_eq!(starts[0].2, tx_ns);
    assert_eq!(starts[0].3, tx_ns.saturating_add(latency.0));

    // Second packet starts when link becomes free (depart of first).
    assert_eq!(starts[1].0, tx_ns);
    assert_eq!(starts[1].1, 11);
    assert_eq!(starts[1].2, tx_ns.saturating_mul(2));
    assert_eq!(
        starts[1].3,
        tx_ns.saturating_mul(2).saturating_add(latency.0)
    );
}

#[test]
fn queue_drop_updates_stats_and_emits_viz_drop() {
    let latency = SimTime(1000);
    let bw = 1_000_000_000;
    let (mut world, h0, h1) = build_two_host_link(latency, bw);

    // Force drop at host egress.
    world.net.set_host_egress_queue_capacity_bytes(100);

    let mut sim = Simulator::default();
    let pkt = Packet::new_dynamic(99, 1, 200, h0, h1);
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt });
    sim.run(&mut world);

    assert_eq!(world.net.stats.dropped_pkts, 1);
    assert_eq!(world.net.stats.delivered_pkts, 0);

    let drops = drop_events(&world, h0, h1);
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].0, 0);
    assert_eq!(drops[0].1, 99);
    assert_eq!(drops[0].2, 100);

    assert!(tx_start_events(&world, h0, h1).is_empty());
}

#[test]
fn link_ready_and_forward_from_same_time_transmits_once_regardless_of_order() {
    let latency = SimTime(1000);
    let bw = 1_000_000_000;
    let bytes = 1000_u32;
    let tx_ns = expected_tx_time_ns(bytes, bw);
    let depart1 = SimTime(tx_ns);

    // Case A: packet arrival at depart1 is scheduled before LinkReady (arrival executes first).
    {
        let mut sim = Simulator::default();
        let (mut world, h0, h1) = build_two_host_link(latency, bw);
        let pkt0 = Packet::new_dynamic(1, 1, bytes, h0, h1);
        let pkt1 = Packet::new_dynamic(2, 1, bytes, h0, h1);
        sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt0 });
        sim.schedule(depart1, DeliverPacket { to: h0, pkt: pkt1 });
        sim.run(&mut world);

        assert_eq!(world.net.stats.delivered_pkts, 2);
        let mut starts = tx_start_events(&world, h0, h1);
        starts.sort_by_key(|(t_ns, pkt_id, _, _)| (*t_ns, *pkt_id));
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].1, 1);
        assert_eq!(starts[1].1, 2);
        assert_eq!(starts[1].0, tx_ns);
    }

    // Case B: LinkReady at depart1 runs before packet arrival at depart1 (LinkReady executes first).
    {
        let mut sim = Simulator::default();
        let (mut world, h0, h1) = build_two_host_link(latency, bw);
        let pkt0 = Packet::new_dynamic(1, 1, bytes, h0, h1);
        let pkt1 = Packet::new_dynamic(2, 1, bytes, h0, h1);
        sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: pkt0 });
        sim.schedule(
            SimTime::ZERO,
            ScheduleDeliver {
                at: depart1,
                to: h0,
                pkt: pkt1,
            },
        );
        sim.run(&mut world);

        assert_eq!(world.net.stats.delivered_pkts, 2);
        let mut starts = tx_start_events(&world, h0, h1);
        starts.sort_by_key(|(t_ns, pkt_id, _, _)| (*t_ns, *pkt_id));
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].1, 1);
        assert_eq!(starts[1].1, 2);
        assert_eq!(starts[1].0, tx_ns);
    }
}

#[test]
fn link_priority_queue_sends_ack_before_data_when_both_queued() {
    let latency = SimTime::from_micros(1);
    let bw = 1_000_000_000; // 1Gbps

    let mut sim = Simulator::default();
    let (mut world, h0, h1) = build_two_host_link(latency, bw);

    // A "blocker" packet starts transmitting immediately at t=0, keeping the link busy.
    let mut blocker = Packet::new_dynamic(1, 1, 1000, h0, h1);
    blocker.transport = Transport::Tcp(TcpSegment::Data { seq: 0, len: 1000 });

    // Two packets arrive while the link is busy, so they are queued together.
    // Data is enqueued before ACK, but the PriorityQueue should dequeue the ACK first.
    let mut data = Packet::new_dynamic(2, 2, 900, h0, h1);
    data.transport = Transport::Tcp(TcpSegment::Data { seq: 0, len: 900 });

    let mut ack = Packet::new_dynamic(3, 3, 60, h0, h1);
    ack.transport = Transport::Tcp(TcpSegment::Ack { ack: 1 });

    sim.schedule(
        SimTime::ZERO,
        DeliverPacket {
            to: h0,
            pkt: blocker,
        },
    );
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: data });
    sim.schedule(SimTime::ZERO, DeliverPacket { to: h0, pkt: ack });

    sim.run(&mut world);

    let mut starts = tx_start_events(&world, h0, h1);
    starts.sort_by_key(|(t_ns, pkt_id, _, _)| (*t_ns, *pkt_id));
    assert_eq!(starts.len(), 3);

    // blocker transmits first, then ACK (high priority), then data.
    assert_eq!(starts[0].1, 1);
    assert_eq!(starts[1].1, 3);
    assert_eq!(starts[2].1, 2);
}
