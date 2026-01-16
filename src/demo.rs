//! 演示和示例代码
//!
//! 包含各种拓扑构建函数和共享类型

use crate::net::{NetWorld, NodeId};
use crate::sim::{Event, SimTime, Simulator, World};

/// Dumbbell 拓扑配置选项
#[derive(Debug, Clone)]
pub struct DumbbellOpts {
    pub pkt_bytes: u32,
    pub pkts: u64,
    pub gap: SimTime,
    pub host_link_gbps: u64,
    pub bottleneck_gbps: u64,
    pub link_latency: SimTime,
    pub until: SimTime,
}

impl Default for DumbbellOpts {
    fn default() -> Self {
        Self {
            pkt_bytes: 1500,
            pkts: 1000,
            gap: SimTime::from_micros(10),
            host_link_gbps: 100,
            bottleneck_gbps: 10,
            link_latency: SimTime::from_micros(2),
            until: SimTime::from_millis(50),
        }
    }
}

/// 构建 dumbbell 拓扑
/// 
/// 拓扑结构：h0 <-> s0 <-> s1 <-> h1
/// 返回：(源节点, 目标节点, 路由路径)
pub fn build_dumbbell(world: &mut NetWorld, opts: &DumbbellOpts) -> (NodeId, NodeId, Vec<NodeId>) {
    let h0 = world.net.add_host("h0");
    let h1 = world.net.add_host("h1");
    let s0 = world.net.add_switch("s0");
    let s1 = world.net.add_switch("s1");

    let gbps_to_bps = |g: u64| g.saturating_mul(1_000_000_000);
    let host_bps = gbps_to_bps(opts.host_link_gbps);
    let bottleneck_bps = gbps_to_bps(opts.bottleneck_gbps);

    // h0 <-> s0
    world.net.connect(h0, s0, opts.link_latency, host_bps);
    world.net.connect(s0, h0, opts.link_latency, host_bps);
    // s0 <-> s1 (bottleneck)
    world.net.connect(s0, s1, opts.link_latency, bottleneck_bps);
    world.net.connect(s1, s0, opts.link_latency, bottleneck_bps);
    // s1 <-> h1
    world.net.connect(s1, h1, opts.link_latency, host_bps);
    world.net.connect(h1, s1, opts.link_latency, host_bps);

    let route = vec![h0, s0, s1, h1];
    (h0, h1, route)
}

/// 流量注入事件
/// 
/// 用于周期性注入数据包
#[derive(Debug)]
pub struct InjectFlow {
    pub flow_id: u64,
    pub src: NodeId,
    pub route: Vec<NodeId>,
    pub pkt_bytes: u32,
    pub remaining: u64,
    pub gap: SimTime,
}

impl Event for InjectFlow {
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
            .make_packet(me.flow_id, me.pkt_bytes, me.route.clone());
        // 从 src 直接发送到下一跳（forward 会 schedule DeliverPacket）
        w.net.forward_from(me.src, pkt, sim);

        me.remaining -= 1;
        if me.remaining > 0 {
            let next_at = SimTime(sim.now().0.saturating_add(me.gap.0));
            sim.schedule(next_at, InjectFlow { ..me });
        }
    }
}
