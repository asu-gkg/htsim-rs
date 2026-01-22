//! Dumbbell 拓扑 TCP 实验
//!
//! 运行一个简化 TCP（Reno 风格）在 dumbbell 拓扑上的单流发送。

use clap::Parser;
use htsim_rs::net::NetWorld;
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpStart};
use htsim_rs::sim::{SimTime, Simulator};
use htsim_rs::topo::dumbbell::{build_dumbbell, DumbbellOpts};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "dumbbell-tcp", about = "Dumbbell 拓扑仿真：h0->h1 单流 TCP（简化 Reno）")]
struct Args {
    /// 要发送的应用数据量（字节）
    #[arg(long, default_value_t = 10_000_000)]
    data_bytes: u64,

    /// MSS（每个 TCP 数据段载荷大小，字节）
    #[arg(long, default_value_t = 1460)]
    mss: u32,

    /// 初始 cwnd（单位：MSS 个数）
    #[arg(long, default_value_t = 10)]
    init_cwnd_pkts: u64,

    /// 初始 ssthresh（单位：MSS 个数）
    #[arg(long, default_value_t = 1_000)]
    init_ssthresh_pkts: u64,

    /// 初始 RTO（毫秒）
    #[arg(long, default_value_t = 200)]
    rto_ms: u64,

    /// 最小 RTO（毫秒）
    #[arg(long, default_value_t = 1)]
    min_rto_ms: u64,

    /// 最大 RTO（毫秒）
    #[arg(long, default_value_t = 60000)]
    max_rto_ms: u64,

    /// 启用三次握手
    #[arg(long, default_value_t = false)]
    handshake: bool,

    /// 应用层限速（包/秒）
    #[arg(long)]
    app_limited_pps: Option<u64>,

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

    /// 输出可视化 JSON 事件文件（供 `viz/index.html` 加载）；不填则不生成
    #[arg(long)]
    viz_json: Option<PathBuf>,
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

    let args = Args::parse();

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

    // 按 C++ 的 -qs 逻辑：把瓶颈链路（s0->s1 及 s1->s0）队列设为有限缓冲
    // dumbbell 路径固定为 [h0, s0, s1, h1]
    if args.queue_pkts > 0 {
        let cap_bytes = args.queue_pkts.saturating_mul(args.mss as u64);
        if route.len() >= 3 {
            let s0 = route[1];
            let s1 = route[2];
            world.net.set_link_queue_capacity_bytes(s0, s1, cap_bytes);
            world.net.set_link_queue_capacity_bytes(s1, s0, cap_bytes);
        }
    }

    // 启用可视化：在拓扑与队列容量设置完成后，发出 meta（含带宽/时延/队列容量）
    if args.viz_json.is_some() {
        world.net.viz = Some(htsim_rs::viz::VizLogger::default());
        world.net.emit_viz_meta();
    }

    let cfg = TcpConfig {
        mss: args.mss,
        ack_bytes: 64,
        init_cwnd_bytes: args.init_cwnd_pkts.saturating_mul(args.mss as u64),
        init_ssthresh_bytes: args.init_ssthresh_pkts.saturating_mul(args.mss as u64),
        init_rto: SimTime::from_millis(args.rto_ms),
        min_rto: SimTime::from_millis(args.min_rto_ms),
        max_rto: SimTime::from_millis(args.max_rto_ms),
        handshake: args.handshake,
        app_limited_pps: args.app_limited_pps,
    };

    let conn_id = 1;
    let conn = TcpConn::new(conn_id, src, dst, route, args.data_bytes, cfg);
    sim.schedule(SimTime::ZERO, TcpStart { conn });

    sim.run_until(opts.until, &mut world);

    if let Some(path) = args.viz_json {
        if let Some(v) = world.net.viz.take() {
            let json = serde_json::to_string_pretty(&v.events).expect("serialize viz events");
            fs::write(&path, json).expect("write viz json");
            eprintln!("wrote viz events to {}", path.display());
        }
    }

    let c = world.net.tcp.get(conn_id).expect("tcp conn exists");
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
            (acked as f64 * 8.0) / (ns as f64) // Gbps：bit/ns
        }
    });

    println!(
        "done @ {:?}\n  tcp: acked_bytes={}, finished={}, start={:?}, end={:?}, goodput_gbps={:?}\n  net: delivered_pkts={}, delivered_bytes={}, dropped_pkts={}, dropped_bytes={}",
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
