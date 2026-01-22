//! Fat-tree ring allreduce with TCP flows.

use clap::{Parser, ValueEnum};
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::tcp::TcpConfig;
use htsim_rs::sim::{SimTime, Simulator};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};
use std::fs;
use std::path::PathBuf;

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

    /// Output visualization JSON (for viz/index.html)
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

struct TcpRingTransport {
    cfg: TcpConfig,
}

impl RingTransport for TcpRingTransport {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: CcRoutingMode,
        sim: &mut Simulator,
        world: &mut NetWorld,
        done: ring::RingDoneCallback,
    ) {
        let mut tcp = std::mem::take(&mut world.net.tcp);
        let conn = match routing {
            CcRoutingMode::PerFlow => {
                let route = world.net.route_ecmp_path(src, dst, flow_id);
                htsim_rs::proto::tcp::TcpConn::new(
                    flow_id,
                    src,
                    dst,
                    route,
                    chunk_bytes,
                    self.cfg.clone(),
                )
            }
            CcRoutingMode::PerPacket => {
                htsim_rs::proto::tcp::TcpConn::new_dynamic(
                    flow_id,
                    src,
                    dst,
                    chunk_bytes,
                    self.cfg.clone(),
                )
            }
        };
        let done_cb: htsim_rs::proto::tcp::TcpDoneCallback = Box::new(move |_, now, sim| {
            done(now, sim);
        });
        tcp.set_done_callback(flow_id, done_cb);
        tcp.start_conn(conn, sim, &mut world.net);
        world.net.tcp = tcp;
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

    let transport = TcpRingTransport { cfg: cfg.clone() };
    let handle = ring::start_ring_allreduce(
        &mut sim,
        RingAllreduceConfig {
            ranks,
            hosts: topo.hosts.iter().take(ranks).copied().collect(),
            chunk_bytes,
            routing: match args.routing {
                RoutingMode::PerFlow => CcRoutingMode::PerFlow,
                RoutingMode::PerPacket => CcRoutingMode::PerPacket,
            },
            start_flow_id: 1,
            transport: Box::new(transport),
        },
    );
    sim.run(&mut world);

    let stats = handle.stats();
    let start = stats.start_at.unwrap_or(sim.now());
    let fct_ns = stats
        .done_at
        .map(|d| d.0.saturating_sub(start.0));
    let reduce_ns = stats
        .reduce_done_at
        .map(|d| d.0.saturating_sub(start.0));
    let p99_ns = percentile_ns(&stats.flow_fct_ns, 0.99);
    let max_flow_ns = stats.flow_fct_ns.iter().copied().max();
    let slow_threshold_ns = SimTime::from_secs(1).0;
    let slow_count = stats
        .flow_fct_ns
        .iter()
        .filter(|&&ns| ns >= slow_threshold_ns)
        .count();
    let slow_ratio = if stats.flow_fct_ns.is_empty() {
        0.0
    } else {
        slow_count as f64 / stats.flow_fct_ns.len() as f64
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
            stats.total_steps,
            fct_ns.map(|ns| ns as f64 / 1_000_000.0),
            reduce_ns.map(|ns| ns as f64 / 1_000_000.0),
            p99_ms,
            max_flow_ms,
            slow_count,
            stats.flow_fct_ns.len(),
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
            stats.flow_fct_ns.len()
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
