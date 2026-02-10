//! Dumbbell 拓扑构建

use crate::net::{NetWorld, NodeId};
use crate::sim::SimTime;

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
