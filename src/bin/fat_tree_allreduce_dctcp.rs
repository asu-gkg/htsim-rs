//! Fat-tree ring allreduce with DCTCP flows.

use clap::{Parser, ValueEnum};
use htsim_rs::net::{EcmpHashMode, NetWorld};
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpDoneCallback};
use htsim_rs::sim::{Event, SimTime, Simulator, World};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Parser)]
#[command(name = "fat-tree-allreduce-dctcp", about = "Fat-tree ring allreduce with DCTCP")]
struct Args {
    #[arg(long, default_value_t = 4)]
    k: usize,

    /// Number of ranks (defaults to all hosts in the fat-tree)
    #[arg(long)]
    ranks: Option<usize>,

    /// Message size per rank (bytes)
    #[arg(long, default_value_t = 10_000_000)]
    msg_bytes: u64,

    /// Chunk size per step (bytes); defaults to ceil(msg_bytes / ranks)
    #[arg(long)]
    chunk_bytes: Option<u64>,

    #[arg(long, default_value_t = 1460)]
    mss: u32,

    /// Initial cwnd in MSS packets
    #[arg(long, default_value_t = 10)]
    init_cwnd_pkts: u64,

    /// Initial ssthresh in MSS packets
    #[arg(long, default_value_t = 1_000)]
    init_ssthresh_pkts: u64,

    /// Initial RTO (microseconds)
    #[arg(long, default_value_t = 200)]
    rto_us: u64,

    /// Max RTO (milliseconds)
    #[arg(long, default_value_t = 200)]
    max_rto_ms: u64,

    /// DCTCP g gain
    #[arg(long, default_value_t = 0.0625)]
    dctcp_g: f64,

    #[arg(long, default_value_t = 100)]
    link_gbps: u64,

    /// Link latency (microseconds)
    #[arg(long, default_value_t = 2)]
    link_latency_us: u64,

    /// Queue capacity per link in MSS packets; 0 keeps default
    #[arg(long, default_value_t = 0)]
    queue_pkts: u64,

    /// ECN marking threshold per link in MSS packets; 0 disables ECN
    #[arg(long, default_value_t = 0)]
    ecn_k_pkts: u64,

    /// Output visualization JSON (for tools/viz/index.html)
    #[arg(long)]
    viz_json: Option<PathBuf>,

    /// Output cwnd CSV for a probe flow
    #[arg(long)]
    cwnd_csv: Option<PathBuf>,

    /// Probe rank for cwnd logging (requires --cwnd-csv)
    #[arg(long, default_value_t = 0)]
    probe_rank: usize,

    /// Probe step index for cwnd logging (requires --cwnd-csv)
    #[arg(long, default_value_t = 0)]
    probe_step: usize,

    /// Disable tracing and summary output
    #[arg(long)]
    quiet: bool,

    /// ECMP routing mode
    #[arg(long, value_enum, default_value_t = RoutingMode::PerPacket)]
    routing: RoutingMode,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RoutingMode {
    PerFlow,
    PerPacket,
}

#[derive(Clone)]
struct CollectiveState {
    ranks: usize,
    hosts: Vec<htsim_rs::net::NodeId>,
    chunk_bytes: u64,
    cfg: DctcpConfig,
    routing: RoutingMode,
    step: usize,
    inflight: usize,
    next_flow_id: u64,
    start_at: Option<SimTime>,
    reduce_done_at: Option<SimTime>,
    done_at: Option<SimTime>,
    probe: Option<(usize, usize)>,
    probe_conn_id: Option<u64>,
}

impl CollectiveState {
    fn total_steps(&self) -> usize {
        self.ranks.saturating_sub(1) * 2
    }
}

struct StartStep {
    state: Arc<Mutex<CollectiveState>>,
}

struct FlowDone {
    state: Arc<Mutex<CollectiveState>>,
}

struct StepContext {
    step: usize,
    ranks: usize,
    hosts: Vec<htsim_rs::net::NodeId>,
    chunk_bytes: u64,
    cfg: DctcpConfig,
    start_flow_id: u64,
    routing: RoutingMode,
    probe: Option<(usize, usize)>,
}

impl Event for StartStep {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let StartStep { state } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let ctx = {
            let mut st = state.lock().expect("collective state lock");
            let total_steps = st.total_steps();
            if total_steps == 0 {
                if st.start_at.is_none() {
                    st.start_at = Some(sim.now());
                }
                st.done_at = Some(sim.now());
                return;
            }
            if st.step >= total_steps {
                st.done_at = Some(sim.now());
                return;
            }
            if st.start_at.is_none() {
                st.start_at = Some(sim.now());
            }
            st.inflight = st.ranks;
            let start_flow_id = st.next_flow_id;
            st.next_flow_id = st.next_flow_id.saturating_add(st.ranks as u64);
            StepContext {
                step: st.step,
                ranks: st.ranks,
                hosts: st.hosts.clone(),
                chunk_bytes: st.chunk_bytes,
                cfg: st.cfg.clone(),
                start_flow_id,
                routing: st.routing,
                probe: st.probe,
            }
        };

        let probe_flow_id = ctx.probe.and_then(|(rank, step)| {
            if step == ctx.step && rank < ctx.ranks {
                Some(ctx.start_flow_id + rank as u64)
            } else {
                None
            }
        });

        let mut dctcp = std::mem::take(&mut w.net.dctcp);

        for rank in 0..ctx.ranks {
            let flow_id = ctx.start_flow_id + rank as u64;
            let src = ctx.hosts[rank];
            let dst = ctx.hosts[(rank + 1) % ctx.ranks];
            let mut conn = match ctx.routing {
                RoutingMode::PerFlow => {
                    let route = w.net.route_ecmp_path(src, dst, flow_id);
                    DctcpConn::new(flow_id, src, dst, route, ctx.chunk_bytes, ctx.cfg.clone())
                }
                RoutingMode::PerPacket => {
                    DctcpConn::new_dynamic(flow_id, src, dst, ctx.chunk_bytes, ctx.cfg.clone())
                }
            };
            if Some(flow_id) == probe_flow_id {
                conn.enable_cwnd_log();
            }
            let done_state = Arc::clone(&state);
            let done_cb: DctcpDoneCallback = Box::new(move |_, now, sim| {
                sim.schedule(now, FlowDone { state: Arc::clone(&done_state) });
            });
            dctcp.set_done_callback(flow_id, done_cb);
            dctcp.start_conn(conn, sim, &mut w.net);
        }
        w.net.dctcp = dctcp;

        if let Some(fid) = probe_flow_id {
            let mut st = state.lock().expect("collective state lock");
            st.probe_conn_id = Some(fid);
        }
    }
}

impl Event for FlowDone {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        let FlowDone { state } = *self;
        let mut start_next = false;
        {
            let mut st = state.lock().expect("collective state lock");
            if st.inflight == 0 || st.done_at.is_some() {
                return;
            }
            st.inflight = st.inflight.saturating_sub(1);
            if st.inflight == 0 {
                if st.step + 1 == st.ranks.saturating_sub(1) {
                    st.reduce_done_at = Some(sim.now());
                }
                st.step = st.step.saturating_add(1);
                if st.step >= st.total_steps() {
                    st.done_at = Some(sim.now());
                } else {
                    start_next = true;
                }
            }
        }

        if start_next {
            sim.schedule(sim.now(), StartStep { state });
        }
    }
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(if args.quiet {
            tracing_subscriber::EnvFilter::new("off")
        } else {
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        })
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .init();

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let topo_opts = FatTreeOpts {
        k: args.k,
        link_gbps: args.link_gbps,
        link_latency: SimTime::from_micros(args.link_latency_us),
    };
    let topo = build_fat_tree(&mut world, &topo_opts);

    let ranks = args.ranks.unwrap_or(topo.hosts.len());
    if ranks == 0 || ranks > topo.hosts.len() {
        eprintln!("invalid ranks: {} (hosts available: {})", ranks, topo.hosts.len());
        return;
    }

    let chunk_bytes = args.chunk_bytes.unwrap_or_else(|| {
        let denom = ranks as u64;
        (args.msg_bytes + denom - 1) / denom
    });
    if chunk_bytes == 0 {
        eprintln!("chunk_bytes must be > 0");
        return;
    }

    let total_steps = ranks.saturating_sub(1) * 2;
    if let Some(_) = args.cwnd_csv {
        if args.probe_rank >= ranks || args.probe_step >= total_steps {
            eprintln!(
                "probe out of range: rank={} step={} (ranks={}, steps={})",
                args.probe_rank, args.probe_step, ranks, total_steps
            );
            return;
        }
    }

    if args.queue_pkts > 0 {
        let cap_bytes = args.queue_pkts.saturating_mul(args.mss as u64);
        world.net.set_all_link_queue_capacity_bytes(cap_bytes);
    }
    if args.ecn_k_pkts > 0 {
        let th_bytes = args.ecn_k_pkts.saturating_mul(args.mss as u64);
        world.net.set_all_link_ecn_threshold_bytes(th_bytes);
    }

    world.net.set_ecmp_hash_mode(match args.routing {
        RoutingMode::PerFlow => EcmpHashMode::Flow,
        RoutingMode::PerPacket => EcmpHashMode::Packet,
    });

    if args.viz_json.is_some() {
        world.net.viz = Some(htsim_rs::viz::VizLogger::default());
        world.net.emit_viz_meta();
    }

    let cfg = DctcpConfig {
        mss: args.mss,
        ack_bytes: 64,
        init_cwnd_bytes: args.init_cwnd_pkts.saturating_mul(args.mss as u64),
        init_ssthresh_bytes: args.init_ssthresh_pkts.saturating_mul(args.mss as u64),
        init_rto: SimTime::from_micros(args.rto_us),
        max_rto: SimTime::from_millis(args.max_rto_ms),
        g: args.dctcp_g,
    };

    let state = Arc::new(Mutex::new(CollectiveState {
        ranks,
        hosts: topo.hosts.iter().take(ranks).copied().collect(),
        chunk_bytes,
        cfg,
        routing: args.routing,
        step: 0,
        inflight: 0,
        next_flow_id: 1,
        start_at: None,
        reduce_done_at: None,
        done_at: None,
        probe: args.cwnd_csv.as_ref().map(|_| (args.probe_rank, args.probe_step)),
        probe_conn_id: None,
    }));

    sim.schedule(SimTime::ZERO, StartStep { state: Arc::clone(&state) });
    sim.run(&mut world);

    let st = state.lock().expect("collective state lock");
    let start = st.start_at.unwrap_or(sim.now());
    let fct_ns = st
        .done_at
        .map(|d| d.0.saturating_sub(start.0));
    let reduce_ns = st
        .reduce_done_at
        .map(|d| d.0.saturating_sub(start.0));

    if !args.quiet {
        println!(
            "done @ {:?}\n  ranks={}, msg_bytes={}, chunk_bytes={}, steps={}\n  fct_ms={:?}, reduce_scatter_ms={:?}\n  net: delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
            sim.now(),
            ranks,
            args.msg_bytes,
            chunk_bytes,
            total_steps,
            fct_ns.map(|ns| ns as f64 / 1_000_000.0),
            reduce_ns.map(|ns| ns as f64 / 1_000_000.0),
            world.net.stats.delivered_pkts,
            world.net.stats.delivered_bytes,
            world.net.stats.dropped_pkts,
            world.net.stats.dropped_bytes
        );
    }

    if let Some(path) = args.viz_json {
        if let Some(v) = world.net.viz.take() {
            let json = serde_json::to_string_pretty(&v.events).expect("serialize viz events");
            fs::write(&path, json).expect("write viz json");
            if !args.quiet {
                eprintln!("wrote viz events to {}", path.display());
            }
        }
    }

    if let Some(path) = args.cwnd_csv {
        if let Some(conn_id) = st.probe_conn_id {
            if let Some(c) = world.net.dctcp.get(conn_id) {
                if let Some(samples) = c.cwnd_samples() {
                    let mut out = String::from("t_ns,cwnd_bytes,ssthresh_bytes,alpha,acked_bytes\n");
                    for s in samples {
                        out.push_str(&format!(
                            "{},{},{},{:.6},{}\n",
                            s.t_ns, s.cwnd_bytes, s.ssthresh_bytes, s.alpha, s.acked_bytes
                        ));
                    }
                    fs::write(&path, out).expect("write cwnd csv");
                    if !args.quiet {
                        eprintln!("wrote cwnd samples to {}", path.display());
                    }
                }
            }
        }
    }
}
