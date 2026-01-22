//! Dumbbell 拓扑 DCTCP 实验
//!
//! 运行一个简化 DCTCP 在 dumbbell 拓扑上的单流发送。

use clap::Parser;
use htsim_rs::net::NetWorld;
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpStart};
use htsim_rs::sim::{SimTime, Simulator};
use htsim_rs::topo::dumbbell::{build_dumbbell, DumbbellOpts};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "dumbbell-dctcp", about = "Dumbbell 拓扑仿真：h0->h1 单流 DCTCP（简化）")]
struct Args {
    /// 要发送的应用数据量（字节）
    #[arg(long, default_value_t = 10_000_000)]
    data_bytes: u64,

    /// MSS（每个 DCTCP 数据段载荷大小，字节）
    #[arg(long, default_value_t = 1460)]
    mss: u32,

    /// 初始 cwnd（单位：MSS 个数）
    #[arg(long, default_value_t = 10)]
    init_cwnd_pkts: u64,

    /// 初始 ssthresh（单位：MSS 个数）
    #[arg(long, default_value_t = 1_000)]
    init_ssthresh_pkts: u64,

    /// 初始 RTO（微秒）
    #[arg(long, default_value_t = 200)]
    rto_us: u64,

    /// 最大 RTO（毫秒）
    #[arg(long, default_value_t = 200)]
    max_rto_ms: u64,

    /// DCTCP alpha 的增益 g
    #[arg(long, default_value_t = 0.0625)]
    dctcp_g: f64,

    #[arg(long, default_value_t = 100)]
    host_link_gbps: u64,

    #[arg(long, default_value_t = 10)]
    bottleneck_gbps: u64,

    /// 单向链路传播时延（微秒）
    #[arg(long, default_value_t = 2)]
    link_latency_us: u64,

    /// 仿真运行到多少毫秒
    #[arg(long, default_value_t = 200)]
    until_ms: u64,

    /// 瓶颈链路队列大小（单位：MSS 个数）；0 表示保持默认（几乎无限，不丢包）
    #[arg(long, default_value_t = 0)]
    queue_pkts: u64,

    /// ECN 标记阈值（单位：MSS 个数）；0 表示不开启 ECN
    #[arg(long, default_value_t = 20)]
    ecn_k_pkts: u64,

    /// 输出可视化 JSON 事件文件（供 `viz/index.html` 加载）；不填则不生成
    #[arg(long)]
    viz_json: Option<PathBuf>,

    /// 输出 cwnd/alpha 采样 CSV（每个 ACK 采样一次）
    #[arg(long)]
    cwnd_csv: Option<PathBuf>,

    /// 不打印日志或统计信息（仅输出到文件）
    #[arg(long)]
    quiet: bool,
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            if args.quiet {
                tracing_subscriber::EnvFilter::new("off")
            } else {
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
            },
        )
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .init();

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let opts = DumbbellOpts {
        pkt_bytes: args.mss, // 仅用于拓扑构建/默认参数占位
        pkts: 0,
        gap: SimTime::ZERO,
        host_link_gbps: args.host_link_gbps,
        bottleneck_gbps: args.bottleneck_gbps,
        link_latency: SimTime::from_micros(args.link_latency_us),
        until: SimTime::from_millis(args.until_ms),
    };

    let (src, dst, route) = build_dumbbell(&mut world, &opts);

    if args.queue_pkts > 0 {
        let cap_bytes = args.queue_pkts.saturating_mul(args.mss as u64);
        if route.len() >= 3 {
            let s0 = route[1];
            let s1 = route[2];
            world.net.set_link_queue_capacity_bytes(s0, s1, cap_bytes);
            world.net.set_link_queue_capacity_bytes(s1, s0, cap_bytes);
        }
    }

    if args.ecn_k_pkts > 0 {
        let k_bytes = args.ecn_k_pkts.saturating_mul(args.mss as u64);
        if route.len() >= 3 {
            let s0 = route[1];
            let s1 = route[2];
            world.net.set_link_ecn_threshold_bytes(s0, s1, k_bytes);
            world.net.set_link_ecn_threshold_bytes(s1, s0, k_bytes);
        }
    }

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

    let conn_id = 1;
    let mut conn = DctcpConn::new(conn_id, src, dst, route, args.data_bytes, cfg);
    if args.cwnd_csv.is_some() {
        conn.enable_cwnd_log();
    }
    sim.schedule(SimTime::ZERO, DctcpStart { conn });

    sim.run_until(opts.until, &mut world);

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

    let c = world.net.dctcp.get(conn_id).expect("dctcp conn exists");
    let acked = c.bytes_acked();
    let done = c.is_done();
    let start = c.start_time();
    let end = c.done_time();

    let dur_ns = match (start, end) {
        (Some(s), Some(e)) if e.0 >= s.0 => Some(e.0 - s.0),
        _ => None,
    };
    let gbps = dur_ns.map(|ns| {
        if ns == 0 {
            0.0
        } else {
            (acked as f64 * 8.0) / (ns as f64)
        }
    });

    if !args.quiet {
        println!(
            "done @ {:?}\n  dctcp: acked_bytes={}, finished={}, start={:?}, end={:?}, goodput_gbps={:?}\n  net: delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
            sim.now(),
            acked,
            done,
            start,
            end,
            gbps,
            world.net.stats.delivered_pkts,
            world.net.stats.delivered_bytes,
            world.net.stats.dropped_pkts,
            world.net.stats.dropped_bytes
        );
    }
}
