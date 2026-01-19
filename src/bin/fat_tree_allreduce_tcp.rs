//! Fat-tree ring allreduce with TCP flows.

use clap::{Parser, ValueEnum};
use htsim_rs::net::{EcmpHashMode, NetWorld};
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpDoneCallback};
use htsim_rs::sim::{Event, SimTime, Simulator, World};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Parser)]
#[command(name = "fat-tree-allreduce-tcp", about = "Fat-tree ring allreduce with TCP")]
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

    /// Min RTO (microseconds)
    #[arg(long, default_value_t = 200)]
    min_rto_us: u64,

    /// Max RTO (milliseconds)
    #[arg(long, default_value_t = 200)]
    max_rto_ms: u64,

    #[arg(long, default_value_t = 100)]
    link_gbps: u64,

    /// Link latency (microseconds)
    #[arg(long, default_value_t = 2)]
    link_latency_us: u64,

    /// Queue capacity per link in MSS packets; 0 keeps default
    #[arg(long, default_value_t = 0)]
    queue_pkts: u64,

    /// Output visualization JSON (for tools/viz/index.html)
    #[arg(long)]
    viz_json: Option<PathBuf>,

    /// Disable tracing and summary output
    #[arg(long)]
    quiet: bool,

    /// Print a single-line stats summary
    #[arg(long)]
    stats: bool,

    /// Enable three-way handshake
    #[arg(long, default_value_t = false)]
    handshake: bool,

    /// Application limit (packets per second)
    #[arg(long)]
    app_limited_pps: Option<u64>,

    /// ECMP routing mode
    #[arg(long, value_enum, default_value_t = RoutingMode::PerFlow)]
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
    cfg: TcpConfig,
    routing: RoutingMode,
    step: usize,
    inflight: usize,
    next_flow_id: u64,
    start_at: Option<SimTime>,
    reduce_done_at: Option<SimTime>,
    done_at: Option<SimTime>,
    flow_start_at: HashMap<u64, SimTime>,
    flow_fct_ns: Vec<u64>,
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
    flow_id: u64,
    done_at: SimTime,
}

struct StepContext {
    ranks: usize,
    hosts: Vec<htsim_rs::net::NodeId>,
    chunk_bytes: u64,
    cfg: TcpConfig,
    start_flow_id: u64,
    routing: RoutingMode,
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
            let step_start = sim.now();
            for rank in 0..st.ranks {
                let flow_id = start_flow_id + rank as u64;
                st.flow_start_at.insert(flow_id, step_start);
            }
            StepContext {
                ranks: st.ranks,
                hosts: st.hosts.clone(),
                chunk_bytes: st.chunk_bytes,
                cfg: st.cfg.clone(),
                start_flow_id,
                routing: st.routing,
            }
        };

        let mut tcp = std::mem::take(&mut w.net.tcp);

        for rank in 0..ctx.ranks {
            let flow_id = ctx.start_flow_id + rank as u64;
            let src = ctx.hosts[rank];
            let dst = ctx.hosts[(rank + 1) % ctx.ranks];
            let conn = match ctx.routing {
                RoutingMode::PerFlow => {
                    let route = w.net.route_ecmp_path(src, dst, flow_id);
                    TcpConn::new(flow_id, src, dst, route, ctx.chunk_bytes, ctx.cfg.clone())
                }
                RoutingMode::PerPacket => {
                    TcpConn::new_dynamic(flow_id, src, dst, ctx.chunk_bytes, ctx.cfg.clone())
                }
            };
            let done_state = Arc::clone(&state);
            let done_cb: TcpDoneCallback = Box::new(move |_, now, sim| {
                sim.schedule(
                    now,
                    FlowDone {
                        state: Arc::clone(&done_state),
                        flow_id,
                        done_at: now,
                    },
                );
            });
            tcp.set_done_callback(flow_id, done_cb);
            tcp.start_conn(conn, sim, &mut w.net);
        }
        w.net.tcp = tcp;
    }
}

impl Event for FlowDone {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        let FlowDone {
            state,
            flow_id,
            done_at,
        } = *self;
        let mut start_next = false;
        {
            let mut st = state.lock().expect("collective state lock");
            if st.inflight == 0 || st.done_at.is_some() {
                return;
            }
            if let Some(start_at) = st.flow_start_at.remove(&flow_id) {
                let fct_ns = done_at.0.saturating_sub(start_at.0);
                st.flow_fct_ns.push(fct_ns);
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

fn percentile_ns(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let p = if p <= 0.0 {
        0.0
    } else if p >= 1.0 {
        1.0
    } else {
        p
    };
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = (p * sorted.len() as f64).ceil() as usize;
    let idx = idx.saturating_sub(1).min(sorted.len().saturating_sub(1));
    sorted.get(idx).copied()
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

    if args.queue_pkts > 0 {
        let cap_bytes = args.queue_pkts.saturating_mul(args.mss as u64);
        world.net.set_all_link_queue_capacity_bytes(cap_bytes);
    }

    world.net.set_ecmp_hash_mode(match args.routing {
        RoutingMode::PerFlow => EcmpHashMode::Flow,
        RoutingMode::PerPacket => EcmpHashMode::Packet,
    });

    if args.viz_json.is_some() {
        world.net.viz = Some(htsim_rs::viz::VizLogger::default());
        world.net.emit_viz_meta();
    }

    let cfg = TcpConfig {
        mss: args.mss,
        ack_bytes: 64,
        init_cwnd_bytes: args.init_cwnd_pkts.saturating_mul(args.mss as u64),
        init_ssthresh_bytes: args.init_ssthresh_pkts.saturating_mul(args.mss as u64),
        init_rto: SimTime::from_micros(args.rto_us),
        min_rto: SimTime::from_micros(args.min_rto_us),
        max_rto: SimTime::from_millis(args.max_rto_ms),
        handshake: args.handshake,
        app_limited_pps: args.app_limited_pps,
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
        flow_start_at: HashMap::new(),
        flow_fct_ns: Vec::new(),
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
    let p99_ns = percentile_ns(&st.flow_fct_ns, 0.99);
    let max_flow_ns = st.flow_fct_ns.iter().copied().max();
    let slow_threshold_ns = SimTime::from_secs(1).0;
    let slow_count = st
        .flow_fct_ns
        .iter()
        .filter(|&&ns| ns >= slow_threshold_ns)
        .count();
    let slow_ratio = if st.flow_fct_ns.is_empty() {
        0.0
    } else {
        slow_count as f64 / st.flow_fct_ns.len() as f64
    };
    let makespan_ms = fct_ns.map(|ns| ns as f64 / 1_000_000.0).unwrap_or(0.0);
    let p99_ms = p99_ns.map(|ns| ns as f64 / 1_000_000.0).unwrap_or(0.0);
    let max_flow_ms = max_flow_ns
        .map(|ns| ns as f64 / 1_000_000.0)
        .unwrap_or(0.0);

    if !args.quiet {
        println!(
            "done @ {:?}\n  ranks={}, msg_bytes={}, chunk_bytes={}, steps={}\n  makespan_ms={:?}, reduce_scatter_ms={:?}, p99_fct_ms={:.6}, max_flow_fct_ms={:.6}, slow_flow_ge_1s={}/{} ({:.3})\n  net: delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
            sim.now(),
            ranks,
            args.msg_bytes,
            chunk_bytes,
            st.total_steps(),
            fct_ns.map(|ns| ns as f64 / 1_000_000.0),
            reduce_ns.map(|ns| ns as f64 / 1_000_000.0),
            p99_ms,
            max_flow_ms,
            slow_count,
            st.flow_fct_ns.len(),
            slow_ratio,
            world.net.stats.delivered_pkts,
            world.net.stats.delivered_bytes,
            world.net.stats.dropped_pkts,
            world.net.stats.dropped_bytes
        );
    }

    if args.stats {
        println!(
            "stats msg_bytes={} p99_fct_ms={:.6} makespan_ms={:.6} max_flow_fct_ms={:.6} slow_flow_ge_1s={} slow_flow_ge_1s_ratio={:.6} flows={}",
            args.msg_bytes,
            p99_ms,
            makespan_ms,
            max_flow_ms,
            slow_count,
            slow_ratio,
            st.flow_fct_ns.len()
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
}
