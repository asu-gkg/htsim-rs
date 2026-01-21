//! 网络拓扑管理
//!
//! 定义网络拓扑结构，包含节点、链路、数据包转发和统计信息。

use std::collections::HashMap;

use super::deliver_packet::DeliverPacket;
use super::id::{LinkId, NodeId};
use super::link_ready::LinkReady;
use super::link::Link;
use super::node::{Host, Node, Switch};
use super::packet::Packet;
use super::stats::Stats;
use super::routing::RoutingTable;
use crate::proto::dctcp::DctcpStack;
use crate::proto::tcp::TcpStack;
use crate::queue::DropTailQueue;
use crate::sim::{SimTime, Simulator};
use crate::viz::{VizLogger, VizNodeKind};
use tracing::{debug, trace};

/// ECMP 哈希的粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmpHashMode {
    /// 按 flow_id（默认，per-flow ECMP）
    Flow,
    /// 按 packet（包含 pkt_id，per-packet ECMP）
    Packet,
}

/// 网络拓扑
pub struct Network {
    nodes: Vec<Option<Box<dyn Node>>>,
    pub(super) node_names: Vec<String>,
    pub(super) node_kinds: Vec<VizNodeKind>,
    pub(super) links: Vec<Link>,
    edges: HashMap<(NodeId, NodeId), LinkId>,
    adj: Vec<Vec<NodeId>>,
    rev_adj: Vec<Vec<NodeId>>,
    routing: RoutingTable,
    next_pkt_id: u64,
    pub stats: Stats,
    pub tcp: TcpStack,
    pub dctcp: DctcpStack,
    pub viz: Option<VizLogger>,
    ecmp_hash_mode: EcmpHashMode,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            node_names: Vec::new(),
            node_kinds: Vec::new(),
            links: Vec::new(),
            edges: HashMap::new(),
            adj: Vec::new(),
            rev_adj: Vec::new(),
            // 固定盐，保证每次运行 ECMP 选择可重复
            routing: RoutingTable::new(0xC5A1_DA7A_5EED_1234),
            next_pkt_id: 0,
            stats: Stats::default(),
            tcp: TcpStack::default(),
            dctcp: DctcpStack::default(),
            viz: None,
            ecmp_hash_mode: EcmpHashMode::Flow,
        }
    }
}

impl Network {
    /// 设置 ECMP 哈希粒度（per-flow / per-packet）。
    pub fn set_ecmp_hash_mode(&mut self, mode: EcmpHashMode) {
        self.ecmp_hash_mode = mode;
    }

    /// 添加主机节点
    pub fn add_host(&mut self, name: impl Into<String>) -> NodeId {
        let name = name.into();
        let id = NodeId(self.nodes.len());
        self.nodes.push(Some(Box::new(Host::new(id, name.clone()))));
        self.node_names.push(name);
        self.node_kinds.push(VizNodeKind::Host);
        self.adj.push(Vec::new());
        self.rev_adj.push(Vec::new());
        id
    }

    /// 添加交换机节点
    pub fn add_switch(&mut self, name: impl Into<String>) -> NodeId {
        let name = name.into();
        let id = NodeId(self.nodes.len());
        self.nodes.push(Some(Box::new(Switch::new(id, name.clone()))));
        self.node_names.push(name);
        self.node_kinds.push(VizNodeKind::Switch);
        self.adj.push(Vec::new());
        self.rev_adj.push(Vec::new());
        id
    }

    /// 连接两个节点（创建单向链路）
    pub fn connect(
        &mut self,
        from: NodeId,
        to: NodeId,
        latency: SimTime,
        bandwidth_bps: u64,
    ) -> LinkId {
        let id = LinkId(self.links.len());
        self.links.push(Link::new(from, to, latency, bandwidth_bps));
        self.edges.insert((from, to), id);
        self.adj[from.0].push(to);
        self.rev_adj[to.0].push(from);
        self.routing.mark_dirty();
        id
    }

    /// 设置某条单向链路的队列容量（字节）。
    ///
    /// 用于实验中把“瓶颈链路”改为有限缓冲，从而产生丢包（DropTail）。
    pub fn set_link_queue_capacity_bytes(&mut self, from: NodeId, to: NodeId, capacity_bytes: u64) {
        let link_id = *self
            .edges
            .get(&(from, to))
            .unwrap_or_else(|| panic!("no link from {:?} to {:?}", from, to));
        self.links[link_id.0].queue = Box::new(DropTailQueue::new(capacity_bytes));
    }

    /// 设置所有链路的队列容量（字节）。
    pub fn set_all_link_queue_capacity_bytes(&mut self, capacity_bytes: u64) {
        for link in &mut self.links {
            link.queue = Box::new(DropTailQueue::new(capacity_bytes));
        }
    }

    /// 设置某条单向链路的 ECN 标记阈值（bytes）。
    pub fn set_link_ecn_threshold_bytes(&mut self, from: NodeId, to: NodeId, threshold_bytes: u64) {
        let link_id = *self
            .edges
            .get(&(from, to))
            .unwrap_or_else(|| panic!("no link from {:?} to {:?}", from, to));
        self.links[link_id.0].ecn_threshold_bytes = Some(threshold_bytes);
    }

    /// 设置所有链路的 ECN 标记阈值（bytes）。
    pub fn set_all_link_ecn_threshold_bytes(&mut self, threshold_bytes: u64) {
        for link in &mut self.links {
            link.ecn_threshold_bytes = Some(threshold_bytes);
        }
    }

    /// 生成基于 ECMP 的单路径（按最短跳数 + flow_id 选择下一跳）。
    pub fn route_ecmp_path(&mut self, src: NodeId, dst: NodeId, flow_id: u64) -> Vec<NodeId> {
        self.routing.ensure_built(&self.adj, &self.rev_adj);
        let mut path = vec![src];
        let mut cur = src;
        let max_hops = self.nodes.len().saturating_add(1);
        while cur != dst {
            let cands = self
                .routing
                .next_hops(cur, dst)
                .unwrap_or_else(|| panic!("no route from {:?} to {:?}", cur, dst));
            let nh = self.routing.pick_ecmp_with_key(cur, dst, flow_id, cands);
            path.push(nh);
            cur = nh;
            if path.len() > max_hops {
                panic!("routing loop from {:?} to {:?} (flow_id={})", src, dst, flow_id);
            }
        }
        path
    }

    /// 创建数据包
    pub fn make_packet(&mut self, flow_id: u64, size_bytes: u32, route: Vec<NodeId>) -> Packet {
        let id = self.next_pkt_id;
        self.next_pkt_id = self.next_pkt_id.wrapping_add(1);
        Packet::new_preset(id, flow_id, size_bytes, route)
    }

    /// 创建“纯动态路由”的数据包：每一跳根据 FIB/ECMP 决定下一跳
    pub fn make_packet_dynamic(
        &mut self,
        flow_id: u64,
        size_bytes: u32,
        src: NodeId,
        dst: NodeId,
    ) -> Packet {
        let id = self.next_pkt_id;
        self.next_pkt_id = self.next_pkt_id.wrapping_add(1);
        Packet::new_dynamic(id, flow_id, size_bytes, src, dst)
    }

    /// 创建“混合路由”的数据包：先沿 prefix 预设前缀走，再动态路由到 dst
    pub fn make_packet_mixed(
        &mut self,
        flow_id: u64,
        size_bytes: u32,
        prefix: Vec<NodeId>,
        dst: NodeId,
    ) -> Packet {
        let id = self.next_pkt_id;
        self.next_pkt_id = self.next_pkt_id.wrapping_add(1);
        Packet::new_mixed(id, flow_id, size_bytes, prefix, dst)
    }

    /// 将数据包交付给节点处理
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, to = ?to))]
    pub fn deliver(&mut self, to: NodeId, pkt: Packet, sim: &mut Simulator) {
        debug!("📬 将数据包交付给节点处理");

        self.viz_arrive_node(sim.now(), &pkt, to);
        
        // 暂时把节点取出来，避免 &mut self 与 &mut node 的重叠借用。
        let mut node = self.nodes[to.0].take().expect("node exists");
        let node_name = self
            .node_names
            .get(to.0)
            .cloned()
            .unwrap_or_else(|| node.name().to_string());
        let node_kind = *self.node_kinds.get(to.0).unwrap_or(&VizNodeKind::Switch);
        trace!(node_name = %node_name, "取出节点");

        self.viz_node_rx(sim.now(), &pkt, to, node_kind, &node_name);
        
        node.on_packet(pkt, sim, self);
        
        trace!("节点处理完成，放回节点");
        self.nodes[to.0] = Some(node);
    }

    /// 从指定节点转发数据包
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, from = ?from, hops_taken = pkt.hops_taken, dst = ?pkt.dst))]
    pub fn forward_from(&mut self, from: NodeId, mut pkt: Packet, sim: &mut Simulator) {
        debug!("🚀 从指定节点转发数据包");

        let to = if let Some(nh) = pkt.preset_next() {
            trace!(to = ?nh, "使用预设下一跳");
            nh
        } else {
            // 动态路由：根据 FIB/ECMP 选择下一跳
            self.routing.ensure_built(&self.adj, &self.rev_adj);
            let cands = self
                .routing
                .next_hops(from, pkt.dst)
                .unwrap_or_else(|| panic!("no route from {:?} to {:?}", from, pkt.dst));
            let key = match self.ecmp_hash_mode {
                EcmpHashMode::Flow => pkt.flow_id,
                EcmpHashMode::Packet => pkt.flow_id ^ pkt.id,
            };
            let nh = self.routing.pick_ecmp_with_key(from, pkt.dst, key, cands);
            trace!(to = ?nh, cands = ?cands, "动态路由（ECMP）选择下一跳");
            nh
        };

        self.viz_node_forward(sim.now(), &pkt, from, to);
        
        let link_id = *self
            .edges
            .get(&(from, to))
            .unwrap_or_else(|| panic!("no link from {:?} to {:?}", from, to));
        debug!(
            link_id = ?link_id,
            latency = ?self.links[link_id.0].latency,
            bandwidth_bps = self.links[link_id.0].bandwidth_bps,
            "找到链路"
        );

        // 入队：若队列满则直接丢弃（DropTail）
        let now = sim.now();
        let (pkt_id, flow_id, pkt_bytes, pkt_kind) =
            (pkt.id, pkt.flow_id, pkt.size_bytes, Self::pkt_kind(&pkt));

        // 为了避免同时可变借用 `self.links[..]` 与 `self`（写 viz），先把结果与队列状态拷出来
        let (enqueue_res, q_bytes, q_cap_bytes, q_len) = {
            let link = &mut self.links[link_id.0];
            if let Some(th) = link.ecn_threshold_bytes {
                let q_next = link.queue.bytes().saturating_add(pkt.size_bytes as u64);
                if q_next >= th {
                    pkt.mark_ce_if_ect();
                }
            }
            let res = link.queue.enqueue(pkt);
            let q_bytes = link.queue.bytes();
            let q_cap_bytes = link.queue.capacity_bytes();
            let q_len = link.queue.len();
            (res, q_bytes, q_cap_bytes, q_len)
        };

        match enqueue_res {
            Ok(()) => {
                self.viz_enqueue(
                    now,
                    pkt_id,
                    flow_id,
                    pkt_bytes,
                    pkt_kind,
                    from,
                    to,
                    q_bytes,
                    q_cap_bytes,
                );
                trace!(
                    now = ?now,
                    q_len,
                    q_bytes,
                    "packet 入队成功"
                );
            }
            Err(pkt) => {
                self.stats.dropped_pkts += 1;
                self.stats.dropped_bytes += pkt.size_bytes as u64;
                self.viz_drop(now, &pkt, from, to, q_bytes, q_cap_bytes);
                debug!(
                    now = ?now,
                    link_id = ?link_id,
                    dropped_pkts = self.stats.dropped_pkts,
                    "队列已满，DropTail 丢弃 packet"
                );
                return;
            }
        }

        // 若链路空闲，则立即开始发送队头 packet
        if now >= self.links[link_id.0].busy_until {
            self.transmit_next_on_link(link_id, sim);
        }
    }

    /// depart 时刻触发：链路完成一次序列化发送，尝试发送下一个队头 packet
    pub(crate) fn on_link_ready(&mut self, link_id: LinkId, sim: &mut Simulator) {
        let now = sim.now();
        let busy_until = self.links[link_id.0].busy_until;
        // 可能会遇到同一时刻的竞态（LinkReady 与新的 forward_from 同时发生）
        if busy_until > now {
            return;
        }
        debug!(
            now = ?now,
            busy_until = ?busy_until,
            "链路空闲，尝试发送下一个队头 packet"
        );
        self.transmit_next_on_link(link_id, sim);
    }

    fn transmit_next_on_link(&mut self, link_id: LinkId, sim: &mut Simulator) {
        let now = sim.now();

        // 先取出必要的链路参数，避免同时持有 link 的可变借用与 schedule
        let (from, to, latency, bandwidth_bps, pkt_opt) = {
            let link = &mut self.links[link_id.0];
            let pkt_opt = link.queue.dequeue();
            (link.from, link.to, link.latency, link.bandwidth_bps, pkt_opt)
        };

        let Some(pkt) = pkt_opt else {
            return;
        };

        // 重新借用 link 更新 busy_until（仅此处更新）
        let tx_time = {
            let link = &self.links[link_id.0];
            // 使用链路带宽计算序列化时延
            link.tx_time(pkt.size_bytes)
        };
        let depart = SimTime(now.0.saturating_add(tx_time.0));
        {
            let link = &mut self.links[link_id.0];
            link.busy_until = depart;
        }
        let arrive = SimTime(depart.0.saturating_add(latency.0));

        self.viz_tx_start(now, &pkt, from, to, depart, arrive);

        trace!(
            now = ?now,
            link_id = ?link_id,
            to = ?to,
            tx_time = ?tx_time,
            depart = ?depart,
            arrive = ?arrive,
            bandwidth_bps = bandwidth_bps,
            "链路发送队头 packet"
        );

        // 到达事件（传播时延 + 序列化时延）
        sim.schedule(arrive, DeliverPacket { to, pkt: pkt.advance() });
        // depart 时刻再次触发，继续出队
        sim.schedule(depart, LinkReady { link_id });
    }

}
