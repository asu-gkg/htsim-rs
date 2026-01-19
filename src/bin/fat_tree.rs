use htsim_rs::net::{NetWorld, NodeId};
use htsim_rs::sim::{Event, SimTime, Simulator, World};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};

#[derive(Debug)]
struct InjectFlowDynamic {
    flow_id: u64,
    src: NodeId,
    dst: NodeId,
    pkt_bytes: u32,
    remaining: u64,
    gap: SimTime,
}

impl Event for InjectFlowDynamic {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let mut me = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        if me.remaining == 0 {
            return;
        }

        let pkt = w
            .net
            .make_packet_dynamic(me.flow_id, me.pkt_bytes, me.src, me.dst);
        w.net.forward_from(me.src, pkt, sim);

        me.remaining -= 1;
        if me.remaining > 0 {
            let next_at = SimTime(sim.now().0.saturating_add(me.gap.0));
            sim.schedule(next_at, InjectFlowDynamic { ..me });
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .init();

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let topo_opts = FatTreeOpts {
        k: 4,
        link_gbps: 100,
        link_latency: SimTime::from_micros(2),
    };
    let topo = build_fat_tree(&mut world, &topo_opts);

    let pkt_bytes = 1500;
    let pkts = 2000;
    let gap = SimTime::from_micros(5);
    let until = SimTime::from_millis(200);

    for pod in 0..topo.k {
        let src = topo.host(pod, 0, 0);
        let dst = topo.host((pod + 1) % topo.k, 0, 0);
        let flow_id = pod as u64 + 1;
        sim.schedule(
            SimTime::ZERO,
            InjectFlowDynamic {
                flow_id,
                src,
                dst,
                pkt_bytes,
                remaining: pkts,
                gap,
            },
        );
    }

    sim.run_until(until, &mut world);

    println!(
        "done @ {:?}, delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
        sim.now(),
        world.net.stats.delivered_pkts,
        world.net.stats.delivered_bytes,
        world.net.stats.dropped_pkts,
        world.net.stats.dropped_bytes
    );
}
