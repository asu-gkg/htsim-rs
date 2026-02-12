use crate::net::NetWorld;
use crate::proto::tcp::{TcpConfig, TcpConn};
use crate::sim::{SimTime, Simulator};
use crate::viz::{VizEventKind, VizLogger};

#[test]
fn tcp_rto_retransmits_after_drop_and_completes() {
    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    let latency = SimTime(1000); // 1us
    let bw = 1_000_000_000; // 1Gbps

    world.net.connect(h0, h1, latency, bw);
    world.net.connect(h1, h0, latency, bw);

    // Small host egress buffers: enough for a single MSS-sized segment to sit in the queue.
    world.net.set_host_egress_queue_capacity_bytes(100);

    world.net.viz = Some(VizLogger::default());

    let mut cfg = TcpConfig::default();
    cfg.mss = 100;
    cfg.ack_bytes = 64;
    cfg.init_cwnd_bytes = (cfg.mss as u64).saturating_mul(10);
    cfg.init_ssthresh_bytes = (cfg.mss as u64).saturating_mul(1_000_000);
    cfg.init_rto = SimTime::from_micros(10);
    cfg.min_rto = SimTime::from_micros(10);
    cfg.max_rto = SimTime::from_millis(1);
    cfg.handshake = false;

    // Send 3 segments: 2 can be in-flight (1 transmitting + 1 queued), the 3rd is dropped.
    //
    // This creates a "tail loss": there are no later packets to generate dupACKs,
    // so recovery should happen via RTO + retransmission.
    let conn_id = 1;
    let total_bytes = 300_u64;
    let conn = TcpConn::new_dynamic(conn_id, h0, h1, total_bytes, cfg);

    let mut tcp = std::mem::take(&mut world.net.tcp);
    tcp.start_conn(conn, &mut sim, &mut world.net);
    world.net.tcp = tcp;

    sim.run(&mut world);

    assert!(
        world.net.stats.dropped_pkts > 0,
        "expected at least one drop"
    );

    let conn = world.net.tcp.get(conn_id).expect("tcp conn missing");
    assert!(conn.is_done(), "tcp conn did not complete");

    let events = &world.net.viz.as_ref().expect("viz enabled").events;
    let mut saw_rto = false;
    let mut saw_retrans = false;
    for ev in events {
        match &ev.kind {
            VizEventKind::TcpRto(v) => {
                if v.conn_id == conn_id {
                    saw_rto = true;
                }
            }
            VizEventKind::TcpSendData(v) => {
                if v.conn_id == conn_id && v.retrans == Some(true) {
                    saw_retrans = true;
                }
            }
            _ => {}
        }
    }

    assert!(saw_rto, "expected at least one TCP RTO event");
    assert!(
        saw_retrans,
        "expected at least one retransmitted data segment"
    );
}
