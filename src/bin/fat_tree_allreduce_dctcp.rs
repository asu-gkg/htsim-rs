//! Fat-tree ring allreduce with DCTCP flows.

use clap::{Parser, ValueEnum};
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::dctcp::DctcpConfig;
use htsim_rs::sim::{SimTime, Simulator};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};
use std::fs;
use std::path::PathBuf;

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

    /// Output visualization JSON (for viz/index.html)
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

struct DctcpRingTransport {
    cfg: DctcpConfig,
    probe_flow_id: Option<u64>,
}

impl RingTransport for DctcpRingTransport {
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
        let mut dctcp = std::mem::take(&mut world.net.dctcp);
        let mut conn = match routing {
            CcRoutingMode::PerFlow => {
                let route = world.net.route_ecmp_path(src, dst, flow_id);
                htsim_rs::proto::dctcp::DctcpConn::new(
                    flow_id,
                    src,
                    dst,
                    route,
                    chunk_bytes,
                    self.cfg.clone(),
                )
            }
            CcRoutingMode::PerPacket => {
                htsim_rs::proto::dctcp::DctcpConn::new_dynamic(
                    flow_id,
                    src,
                    dst,
                    chunk_bytes,
                    self.cfg.clone(),
                )
            }
        };
        if Some(flow_id) == self.probe_flow_id {
            conn.enable_cwnd_log();
        }
        let done_cb: htsim_rs::proto::dctcp::DctcpDoneCallback = Box::new(move |_, now, sim| {
            done(now, sim);
        });
        dctcp.set_done_callback(flow_id, done_cb);
        dctcp.start_conn(conn, sim, &mut world.net);
        world.net.dctcp = dctcp;
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
    if args.cwnd_csv.is_some() {
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

    let probe_flow_id = args.cwnd_csv.as_ref().map(|_| {
        let step_offset = (args.probe_step as u64).saturating_mul(ranks as u64);
        1_u64
            .saturating_add(step_offset)
            .saturating_add(args.probe_rank as u64)
    });

    let transport = DctcpRingTransport {
        cfg: cfg.clone(),
        probe_flow_id,
    };
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
            done_cb: None,
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

    if !args.quiet {
        println!(
            "done @ {:?}\n  ranks={}, msg_bytes={}, chunk_bytes={}, steps={}\n  makespan_ms={:?}, reduce_scatter_ms={:?}\n  net: delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
            sim.now(),
            ranks,
            args.msg_bytes,
            chunk_bytes,
            stats.total_steps,
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
        if let Some(conn_id) = probe_flow_id {
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
