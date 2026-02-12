use crate::net::NetWorld;
use crate::proto::dctcp::{DctcpConfig, DctcpConn};
use crate::sim::{SimTime, Simulator};
use crate::viz::{VizCwndReason, VizEventKind, VizLogger};

#[test]
fn dctcp_emits_ecn_window_cwnd_event_when_link_marks_ce() {
    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    let latency = SimTime::from_micros(1);
    let bw = 100_u64 * 1_000_000_000; // 100Gbps

    // Bidirectional connectivity for data + ACKs.
    world.net.connect(h0, h1, latency, bw);
    world.net.connect(h1, h0, latency, bw);

    // Mark every ECT packet as CE on the forward link.
    world.net.set_link_ecn_threshold_bytes(h0, h1, 1);

    world.net.viz = Some(VizLogger::default());

    let cfg = DctcpConfig::default();
    let init_cwnd = cfg.init_cwnd_bytes.max(cfg.mss as u64);
    let total_bytes = init_cwnd.saturating_mul(2);
    let mut conn = DctcpConn::new_dynamic(1, h0, h1, total_bytes, cfg);
    conn.enable_cwnd_log();

    let mut stack = std::mem::take(&mut world.net.dctcp);
    stack.start_conn(conn, &mut sim, &mut world.net);
    world.net.dctcp = stack;

    sim.run(&mut world);

    let v = world.net.viz.as_ref().expect("viz enabled");
    let mut saw_ecn_window = false;
    for ev in &v.events {
        if let VizEventKind::DctcpCwnd {
            reason,
            alpha,
            ecn_frac,
            ..
        } = &ev.kind
        {
            if matches!(reason, VizCwndReason::DctcpEcnWindow) {
                saw_ecn_window = true;
                assert!(*alpha > 0.0);
                assert!(ecn_frac.is_some());
            }
        }
    }
    assert!(
        saw_ecn_window,
        "expected at least one DctcpEcnWindow cwnd event"
    );
}
