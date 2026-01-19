//! 单包追踪模式
//!
//! 只发送一个数据包，打印详细的执行流程和调试信息

use clap::Parser;
use htsim_rs::net::{NetWorld, NodeId};
use htsim_rs::sim::{Event, SimTime, Simulator, World};
use htsim_rs::topo::dumbbell::{build_dumbbell, DumbbellOpts};
use tracing::{debug, info, trace};

#[derive(Debug, Parser)]
#[command(name = "trace-single-packet", about = "单包追踪模式：只发送一个数据包，打印详细的执行流程")]
struct Args {
    #[arg(long, default_value_t = 1500)]
    pkt_bytes: u32,
    #[arg(long, default_value_t = 100)]
    host_link_gbps: u64,
    #[arg(long, default_value_t = 10)]
    bottleneck_gbps: u64,
    /// 单向链路传播时延（微秒）
    #[arg(long, default_value_t = 2)]
    link_latency_us: u64,
}

/// 单包追踪事件：只发送一个数据包并打印详细调试信息
#[derive(Debug)]
struct TraceSinglePacket {
    flow_id: u64,
    src: NodeId,
    route: Vec<NodeId>,
    pkt_bytes: u32,
}

impl Event for TraceSinglePacket {
    #[tracing::instrument(skip(self, sim, world), fields(flow_id = self.flow_id, src = ?self.src, pkt_bytes = self.pkt_bytes))]
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let TraceSinglePacket {
            flow_id,
            src,
            route,
            pkt_bytes,
        } = *self;
        
        info!("📦 创建并发送单个数据包");
        debug!(
            now = ?sim.now(),
            route = ?route,
            "事件参数"
        );
        
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let pkt = w.net.make_packet(flow_id, pkt_bytes, route.clone());
        trace!(
            pkt_id = pkt.id,
            dst = ?pkt.dst,
            hops_taken = pkt.hops_taken,
            "创建数据包"
        );
        
        // 从 src 直接发送到下一跳（forward 会 schedule DeliverPacket）
        w.net.forward_from(src, pkt, sim);
        
        debug!("数据包已从源节点发出，等待链路传输");
    }
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
        pkts: 1,
        gap: SimTime::ZERO,
        host_link_gbps: args.host_link_gbps,
        bottleneck_gbps: args.bottleneck_gbps,
        link_latency: SimTime::from_micros(args.link_latency_us),
        until: SimTime::from_millis(100),
    };

    info!("╔════════════════════════════════════════════════════════════════════════════════╗");
    info!("║                    单包追踪模式启动                                            ║");
    info!("╚════════════════════════════════════════════════════════════════════════════════╝");
    
    let (src, _dst, route) = build_dumbbell(&mut world, &opts);
    
    info!("构建 dumbbell 拓扑: h0 (src) -> s0 -> s1 -> h1 (dst)");
    debug!(route = ?route, "路径信息");
    
    // 注入单个数据包
    info!("在 t=0 调度 TraceSinglePacket 事件");
    sim.schedule(
        SimTime::ZERO,
        TraceSinglePacket {
            flow_id: 1,
            src,
            route,
            pkt_bytes: args.pkt_bytes,
        },
    );

    info!("开始运行仿真直到所有事件完成");
    sim.run(&mut world);
    
    info!("╔════════════════════════════════════════════════════════════════════════════════╗");
    info!("║                    仿真完成                                                    ║");
    info!("╚════════════════════════════════════════════════════════════════════════════════╝");

    println!(
        "done @ {:?}, delivered_pkts={}, delivered_bytes={}",
        sim.now(),
        world.net.stats.delivered_pkts,
        world.net.stats.delivered_bytes
    );
}
