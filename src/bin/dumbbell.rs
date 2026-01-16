//! Dumbbell 拓扑仿真
//!
//! 运行 dumbbell 拓扑的单流发包示例

use clap::Parser;
use htsim_rs::demo::{build_dumbbell, DumbbellOpts, InjectFlow};
use htsim_rs::net::NetWorld;
use htsim_rs::sim::{SimTime, Simulator};

#[derive(Debug, Parser)]
#[command(name = "dumbbell", about = "Dumbbell 拓扑仿真：h0->h1 单流发包")]
struct Args {
    #[arg(long, default_value_t = 1500)]
    pkt_bytes: u32,
    #[arg(long, default_value_t = 10_000)]
    pkts: u64,
    /// 两个 packet 注入间隔（微秒）
    #[arg(long, default_value_t = 10)]
    gap_us: u64,
    #[arg(long, default_value_t = 100)]
    host_link_gbps: u64,
    #[arg(long, default_value_t = 10)]
    bottleneck_gbps: u64,
    /// 单向链路传播时延（微秒）
    #[arg(long, default_value_t = 2)]
    link_latency_us: u64,
    /// 仿真运行到多少毫秒
    #[arg(long, default_value_t = 50)]
    until_ms: u64,
}

fn main() {
    // 初始化 tracing
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
        pkt_bytes: args.pkt_bytes,
        pkts: args.pkts,
        gap: SimTime::from_micros(args.gap_us),
        host_link_gbps: args.host_link_gbps,
        bottleneck_gbps: args.bottleneck_gbps,
        link_latency: SimTime::from_micros(args.link_latency_us),
        until: SimTime::from_millis(args.until_ms),
    };

    let (src, _dst, route) = build_dumbbell(&mut world, &opts);

    // 注入一个 flow
    sim.schedule(
        SimTime::ZERO,
        InjectFlow {
            flow_id: 1,
            src,
            route,
            pkt_bytes: opts.pkt_bytes,
            remaining: opts.pkts,
            gap: opts.gap,
        },
    );

    sim.run_until(opts.until, &mut world);

    println!(
        "done @ {:?}, delivered_pkts={}, delivered_bytes={}",
        sim.now(),
        world.net.stats.delivered_pkts,
        world.net.stats.delivered_bytes
    );
}
