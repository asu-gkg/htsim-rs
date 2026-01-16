//! 网络拓扑管理
//!
//! 定义网络拓扑结构，包含节点、链路、数据包转发和统计信息。

use std::collections::HashMap;

use super::deliver_packet::DeliverPacket;
use super::id::{LinkId, NodeId};
use super::link::Link;
use super::node::{Host, Node, Switch};
use super::packet::Packet;
use super::stats::Stats;
use crate::sim::{SimTime, Simulator};
use tracing::{debug, info, trace};

/// 网络拓扑
#[derive(Default)]
pub struct Network {
    nodes: Vec<Option<Box<dyn Node>>>,
    links: Vec<Link>,
    edges: HashMap<(NodeId, NodeId), LinkId>,
    next_pkt_id: u64,
    pub stats: Stats,
}

impl Network {
    /// 添加主机节点
    pub fn add_host(&mut self, name: impl Into<String>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Some(Box::new(Host::new(id, name))));
        id
    }

    /// 添加交换机节点
    pub fn add_switch(&mut self, name: impl Into<String>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Some(Box::new(Switch::new(id, name))));
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
        id
    }

    /// 创建数据包
    pub fn make_packet(&mut self, flow_id: u64, size_bytes: u32, route: Vec<NodeId>) -> Packet {
        let id = self.next_pkt_id;
        self.next_pkt_id = self.next_pkt_id.wrapping_add(1);
        Packet {
            id,
            flow_id,
            size_bytes,
            route,
            hop: 0,
        }
    }

    /// 将数据包交付给节点处理
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, to = ?to))]
    pub fn deliver(&mut self, to: NodeId, pkt: Packet, sim: &mut Simulator) {
        debug!("📬 将数据包交付给节点处理");
        
        // 暂时把节点取出来，避免 &mut self 与 &mut node 的重叠借用。
        let mut node = self.nodes[to.0].take().expect("node exists");
        let node_name = node.name().to_string();
        trace!(node_name = %node_name, "取出节点");
        
        node.on_packet(pkt, sim, self);
        
        trace!("节点处理完成，放回节点");
        self.nodes[to.0] = Some(node);
    }

    /// 从指定节点转发数据包
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, from = ?from, hop = pkt.hop))]
    pub fn forward_from(&mut self, from: NodeId, pkt: Packet, sim: &mut Simulator) {
        debug!("🚀 从指定节点转发数据包");
        
        let to = pkt.next().expect("has_next checked by caller");
        trace!(to = ?to, "查找下一跳");
        
        let link_id = *self
            .edges
            .get(&(from, to))
            .unwrap_or_else(|| panic!("no link from {:?} to {:?}", from, to));
        let link = &mut self.links[link_id.0];
        debug!(
            link_id = ?link_id,
            latency = ?link.latency,
            bandwidth_bps = link.bandwidth_bps,
            "找到链路"
        );

        let now = sim.now();
        let start = now.max(link.busy_until);
        let tx_time = link.tx_time(pkt.size_bytes);
        let depart = SimTime(start.0.saturating_add(tx_time.0));
        link.busy_until = depart;
        let arrive = SimTime(depart.0.saturating_add(link.latency.0));

        trace!(
            now = ?now,
            busy_until = ?link.busy_until,
            start = ?start,
            tx_time = ?tx_time,
            depart = ?depart,
            arrive = ?arrive,
            "计算传输时间"
        );
        
        debug!(
            arrive = ?arrive,
            to = ?to,
            next_hop = pkt.hop + 1,
            "调度数据包到达事件"
        );
        
        sim.schedule(arrive, DeliverPacket { to, pkt: pkt.advance() });
    }

    /// 数据包送达目的地时的处理
    #[tracing::instrument(skip(self), fields(pkt_id = pkt.id, flow_id = pkt.flow_id))]
    pub(crate) fn on_delivered(&mut self, pkt: Packet) {
        info!("✅ 数据包送达目的地");
        
        let old_pkts = self.stats.delivered_pkts;
        let old_bytes = self.stats.delivered_bytes;
        
        self.stats.delivered_pkts += 1;
        self.stats.delivered_bytes += pkt.size_bytes as u64;
        
        debug!(
            size_bytes = pkt.size_bytes,
            delivered_pkts = old_pkts,
            new_delivered_pkts = self.stats.delivered_pkts,
            delivered_bytes = old_bytes,
            new_delivered_bytes = self.stats.delivered_bytes,
            "更新统计信息"
        );
    }
}