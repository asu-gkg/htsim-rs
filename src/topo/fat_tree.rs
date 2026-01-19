//! Fat-tree 拓扑构建

use crate::net::{NetWorld, NodeId};
use crate::sim::SimTime;

#[derive(Debug, Clone)]
pub struct FatTreeOpts {
    pub k: usize,
    pub link_gbps: u64,
    pub link_latency: SimTime,
}

impl Default for FatTreeOpts {
    fn default() -> Self {
        Self {
            k: 4,
            link_gbps: 100,
            link_latency: SimTime::from_micros(2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FatTreeTopology {
    pub k: usize,
    pub hosts: Vec<NodeId>,
    pub edge_switches: Vec<NodeId>,
    pub agg_switches: Vec<NodeId>,
    pub core_switches: Vec<NodeId>,
}

impl FatTreeTopology {
    fn half(&self) -> usize {
        self.k / 2
    }

    pub fn host(&self, pod: usize, edge: usize, host: usize) -> NodeId {
        let half = self.half();
        let idx = (pod * half + edge) * half + host;
        self.hosts[idx]
    }

    pub fn edge(&self, pod: usize, edge: usize) -> NodeId {
        let half = self.half();
        let idx = pod * half + edge;
        self.edge_switches[idx]
    }

    pub fn agg(&self, pod: usize, agg: usize) -> NodeId {
        let half = self.half();
        let idx = pod * half + agg;
        self.agg_switches[idx]
    }

    pub fn core(&self, group: usize, index: usize) -> NodeId {
        let half = self.half();
        let idx = group * half + index;
        self.core_switches[idx]
    }
}

pub fn build_fat_tree(world: &mut NetWorld, opts: &FatTreeOpts) -> FatTreeTopology {
    let k = opts.k;
    assert!(k >= 2 && k % 2 == 0, "fat-tree k must be even and >= 2");

    let half = k / 2;
    let link_bps = opts.link_gbps.saturating_mul(1_000_000_000);
    let latency = opts.link_latency;

    let mut core_switches = Vec::with_capacity(half * half);
    for group in 0..half {
        for index in 0..half {
            let name = format!("c{}_{}", group, index);
            core_switches.push(world.net.add_switch(name));
        }
    }

    let mut hosts = Vec::with_capacity(k * half * half);
    let mut edge_switches = Vec::with_capacity(k * half);
    let mut agg_switches = Vec::with_capacity(k * half);
    let mut pod_edges: Vec<Vec<NodeId>> = Vec::with_capacity(k);
    let mut pod_aggs: Vec<Vec<NodeId>> = Vec::with_capacity(k);

    for pod in 0..k {
        let mut edges = Vec::with_capacity(half);
        let mut aggs = Vec::with_capacity(half);

        for edge in 0..half {
            let name = format!("p{}_e{}", pod, edge);
            edges.push(world.net.add_switch(name));
        }
        for agg in 0..half {
            let name = format!("p{}_a{}", pod, agg);
            aggs.push(world.net.add_switch(name));
        }

        for (edge_idx, edge_id) in edges.iter().enumerate() {
            for host in 0..half {
                let name = format!("h{}_{}_{}", pod, edge_idx, host);
                let host_id = world.net.add_host(name);
                world.net.connect(host_id, *edge_id, latency, link_bps);
                world.net.connect(*edge_id, host_id, latency, link_bps);
                hosts.push(host_id);
            }
        }

        edge_switches.extend(edges.iter().copied());
        agg_switches.extend(aggs.iter().copied());
        pod_edges.push(edges);
        pod_aggs.push(aggs);
    }

    for pod in 0..k {
        for edge in 0..half {
            for agg in 0..half {
                let edge_id = pod_edges[pod][edge];
                let agg_id = pod_aggs[pod][agg];
                world.net.connect(edge_id, agg_id, latency, link_bps);
                world.net.connect(agg_id, edge_id, latency, link_bps);
            }
        }
    }

    for pod in 0..k {
        for agg in 0..half {
            let agg_id = pod_aggs[pod][agg];
            for index in 0..half {
                let core_id = core_switches[agg * half + index];
                world.net.connect(agg_id, core_id, latency, link_bps);
                world.net.connect(core_id, agg_id, latency, link_bps);
            }
        }
    }

    FatTreeTopology {
        k,
        hosts,
        edge_switches,
        agg_switches,
        core_switches,
    }
}
