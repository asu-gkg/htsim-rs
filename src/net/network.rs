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
use crate::proto::tcp::TcpStack;
use crate::proto::Transport;
use crate::queue::DropTailQueue;
use crate::sim::{SimTime, Simulator};
use crate::viz::{VizEvent, VizEventKind, VizLogger, VizNodeKind, VizPacketKind, VizTcp};
use tracing::{debug, info, trace};

/// 网络拓扑
pub struct Network {
    nodes: Vec<Option<Box<dyn Node>>>,
    node_names: Vec<String>,
    node_kinds: Vec<VizNodeKind>,
    links: Vec<Link>,
    edges: HashMap<(NodeId, NodeId), LinkId>,
    adj: Vec<Vec<NodeId>>,
    rev_adj: Vec<Vec<NodeId>>,
    routing: RoutingTable,
    next_pkt_id: u64,
    pub stats: Stats,
    pub tcp: TcpStack,
    pub viz: Option<VizLogger>,
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
            viz: None,
        }
    }
}

impl Network {
    fn pkt_kind(pkt: &Packet) -> VizPacketKind {
        match &pkt.transport {
            Transport::Tcp(crate::proto::TcpSegment::Ack { .. }) => VizPacketKind::Ack,
            Transport::Tcp(crate::proto::TcpSegment::Data { .. }) => VizPacketKind::Data,
            _ => VizPacketKind::Other,
        }
    }

    fn viz_push(&mut self, ev: VizEvent) {
        if let Some(v) = &mut self.viz {
            v.push(ev);
        }
    }

    pub(crate) fn viz_tcp_send_data(&mut self, t_ns: u64, conn_id: u64, seq: u64, len: u32) {
        self.viz_push(VizEvent {
            t_ns,
            pkt_id: None,
            flow_id: Some(conn_id),
            pkt_bytes: None,
            pkt_kind: Some(VizPacketKind::Data),
            kind: VizEventKind::TcpSendData(VizTcp {
                conn_id,
                seq: Some(seq),
                len: Some(len),
                ack: None,
            }),
        });
    }

    pub(crate) fn viz_tcp_send_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64) {
        self.viz_push(VizEvent {
            t_ns,
            pkt_id: None,
            flow_id: Some(conn_id),
            pkt_bytes: None,
            pkt_kind: Some(VizPacketKind::Ack),
            kind: VizEventKind::TcpSendAck(VizTcp {
                conn_id,
                seq: None,
                len: None,
                ack: Some(ack),
            }),
        });
    }

    pub(crate) fn viz_tcp_recv_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64) {
        self.viz_push(VizEvent {
            t_ns,
            pkt_id: None,
            flow_id: Some(conn_id),
            pkt_bytes: None,
            pkt_kind: Some(VizPacketKind::Ack),
            kind: VizEventKind::TcpRecvAck(VizTcp {
                conn_id,
                seq: None,
                len: None,
                ack: Some(ack),
            }),
        });
    }

    pub(crate) fn viz_tcp_rto(&mut self, t_ns: u64, conn_id: u64, seq: u64) {
        self.viz_push(VizEvent {
            t_ns,
            pkt_id: None,
            flow_id: Some(conn_id),
            pkt_bytes: None,
            pkt_kind: Some(VizPacketKind::Data),
            kind: VizEventKind::TcpRto(VizTcp {
                conn_id,
                seq: Some(seq),
                len: None,
                ack: None,
            }),
        });
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

        self.viz_push(VizEvent {
            t_ns: sim.now().0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(&pkt)),
            kind: VizEventKind::ArriveNode { node: to.0 },
        });
        
        // 暂时把节点取出来，避免 &mut self 与 &mut node 的重叠借用。
        let mut node = self.nodes[to.0].take().expect("node exists");
        let node_name = self
            .node_names
            .get(to.0)
            .cloned()
            .unwrap_or_else(|| node.name().to_string());
        let node_kind = *self.node_kinds.get(to.0).unwrap_or(&VizNodeKind::Switch);
        trace!(node_name = %node_name, "取出节点");

        self.viz_push(VizEvent {
            t_ns: sim.now().0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(&pkt)),
            kind: VizEventKind::NodeRx {
                node: to.0,
                node_kind,
                node_name: node_name.clone(),
            },
        });
        
        node.on_packet(pkt, sim, self);
        
        trace!("节点处理完成，放回节点");
        self.nodes[to.0] = Some(node);
    }

    /// 从指定节点转发数据包
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, from = ?from, hops_taken = pkt.hops_taken, dst = ?pkt.dst))]
    pub fn forward_from(&mut self, from: NodeId, pkt: Packet, sim: &mut Simulator) {
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
            let nh = self.routing.pick_ecmp(from, pkt.dst, pkt.flow_id, cands);
            trace!(to = ?nh, cands = ?cands, "动态路由（ECMP）选择下一跳");
            nh
        };

        self.viz_push(VizEvent {
            t_ns: sim.now().0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(&pkt)),
            kind: VizEventKind::NodeForward {
                node: from.0,
                next: to.0,
            },
        });
        
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
            let res = link.queue.enqueue(pkt);
            let q_bytes = link.queue.bytes();
            let q_cap_bytes = link.queue.capacity_bytes();
            let q_len = link.queue.len();
            (res, q_bytes, q_cap_bytes, q_len)
        };

        match enqueue_res {
            Ok(()) => {
                self.viz_push(VizEvent {
                    t_ns: now.0,
                    pkt_id: Some(pkt_id),
                    flow_id: Some(flow_id),
                    pkt_bytes: Some(pkt_bytes),
                    pkt_kind: Some(pkt_kind),
                    kind: VizEventKind::Enqueue {
                        link_from: from.0,
                        link_to: to.0,
                        q_bytes,
                        q_cap_bytes,
                    },
                });
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
                self.viz_push(VizEvent {
                    t_ns: now.0,
                    pkt_id: Some(pkt.id),
                    flow_id: Some(pkt.flow_id),
                    pkt_bytes: Some(pkt.size_bytes),
                    pkt_kind: Some(Self::pkt_kind(&pkt)),
                    kind: VizEventKind::Drop {
                        link_from: from.0,
                        link_to: to.0,
                        q_bytes,
                        q_cap_bytes,
                    },
                });
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

        self.viz_push(VizEvent {
            t_ns: now.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(&pkt)),
            kind: VizEventKind::TxStart {
                link_from: from.0,
                link_to: to.0,
                depart_ns: depart.0,
                arrive_ns: arrive.0,
            },
        });

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

    /// 数据包送达目的地时的处理
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, flow_id = pkt.flow_id))]
    pub(crate) fn on_delivered(&mut self, at: NodeId, pkt: Packet, sim: &mut Simulator) {
        info!("✅ 数据包送达目的地");

        self.viz_push(VizEvent {
            t_ns: sim.now().0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(&pkt)),
            kind: VizEventKind::Delivered { node: at.0 },
        });
        
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

        // 传输层处理（例如 TCP：目的端产生 ACK、源端处理 ACK 驱动继续发送）
        if let Transport::Tcp(seg) = pkt.transport {
            let conn_id = pkt.flow_id;
            let mut tcp = std::mem::take(&mut self.tcp);
            tcp.on_tcp_segment(conn_id, at, seg, sim, self);
            self.tcp = tcp;
        }
    }
}