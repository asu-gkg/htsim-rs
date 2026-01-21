//! Visualization hooks for the network.

use crate::sim::SimTime;
use crate::viz::{
    VizEvent, VizEventKind, VizLinkInfo, VizNodeInfo, VizNodeKind, VizPacketKind, VizTcp,
};

use super::{DctcpSegment, Network, NodeId, Packet, TcpSegment, Transport};

impl Network {
    pub(crate) fn pkt_kind(pkt: &Packet) -> VizPacketKind {
        match &pkt.transport {
            Transport::Tcp(TcpSegment::Ack { .. }) => VizPacketKind::Ack,
            Transport::Tcp(TcpSegment::Data { .. }) => VizPacketKind::Data,
            Transport::Tcp(TcpSegment::Syn) => VizPacketKind::Ack,
            Transport::Tcp(TcpSegment::SynAck) => VizPacketKind::Ack,
            Transport::Tcp(TcpSegment::HandshakeAck) => VizPacketKind::Ack,
            Transport::Dctcp(DctcpSegment::Ack { .. }) => VizPacketKind::Ack,
            Transport::Dctcp(DctcpSegment::Data { .. }) => VizPacketKind::Data,
            _ => VizPacketKind::Other,
        }
    }

    fn viz_push(&mut self, ev: VizEvent) {
        if let Some(v) = &mut self.viz {
            v.push(ev);
        }
    }

    pub fn emit_viz_meta(&mut self) {
        if self.viz.is_none() {
            return;
        }
        let nodes = self
            .node_names
            .iter()
            .enumerate()
            .map(|(id, name)| VizNodeInfo {
                id,
                name: name.clone(),
                kind: *self.node_kinds.get(id).unwrap_or(&VizNodeKind::Switch),
            })
            .collect::<Vec<_>>();
        let links = self
            .links
            .iter()
            .map(|l| VizLinkInfo {
                from: l.from.0,
                to: l.to.0,
                bandwidth_bps: l.bandwidth_bps,
                latency_ns: l.latency.0,
                q_cap_bytes: l.queue.capacity_bytes(),
            })
            .collect::<Vec<_>>();
        self.viz_push(VizEvent {
            t_ns: 0,
            pkt_id: None,
            flow_id: None,
            pkt_bytes: None,
            pkt_kind: None,
            kind: VizEventKind::Meta { nodes, links },
        });
    }

    pub(crate) fn viz_tcp_send_data(&mut self, t_ns: u64, conn_id: u64, seq: u64, len: u32, retrans: bool) {
        let retrans = if retrans { Some(true) } else { None };
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
                retrans,
                ecn_echo: None,
            }),
        });
    }

    pub(crate) fn viz_tcp_send_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool) {
        let ecn_echo = if ecn_echo { Some(true) } else { None };
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
                retrans: None,
                ecn_echo,
            }),
        });
    }

    pub(crate) fn viz_tcp_recv_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool) {
        let ecn_echo = if ecn_echo { Some(true) } else { None };
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
                retrans: None,
                ecn_echo,
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
                retrans: None,
                ecn_echo: None,
            }),
        });
    }

    pub(crate) fn viz_dctcp_cwnd(
        &mut self,
        t_ns: u64,
        conn_id: u64,
        cwnd_bytes: u64,
        ssthresh_bytes: u64,
        inflight_bytes: u64,
        alpha: f64,
    ) {
        self.viz_push(VizEvent {
            t_ns,
            pkt_id: None,
            flow_id: Some(conn_id),
            pkt_bytes: None,
            pkt_kind: None,
            kind: VizEventKind::DctcpCwnd {
                conn_id,
                cwnd_bytes,
                ssthresh_bytes,
                inflight_bytes,
                alpha,
            },
        });
    }

    pub(crate) fn viz_arrive_node(&mut self, t: SimTime, pkt: &Packet, node: NodeId) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::ArriveNode { node: node.0 },
        });
    }

    pub(crate) fn viz_node_rx(
        &mut self,
        t: SimTime,
        pkt: &Packet,
        node: NodeId,
        node_kind: VizNodeKind,
        node_name: &str,
    ) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::NodeRx {
                node: node.0,
                node_kind,
                node_name: node_name.to_string(),
            },
        });
    }

    pub(crate) fn viz_node_forward(
        &mut self,
        t: SimTime,
        pkt: &Packet,
        node: NodeId,
        next: NodeId,
    ) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::NodeForward {
                node: node.0,
                next: next.0,
            },
        });
    }

    pub(crate) fn viz_enqueue(
        &mut self,
        t: SimTime,
        pkt_id: u64,
        flow_id: u64,
        pkt_bytes: u32,
        pkt_kind: VizPacketKind,
        from: NodeId,
        to: NodeId,
        q_bytes: u64,
        q_cap_bytes: u64,
    ) {
        self.viz_push(VizEvent {
            t_ns: t.0,
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
    }

    pub(crate) fn viz_drop(
        &mut self,
        t: SimTime,
        pkt: &Packet,
        from: NodeId,
        to: NodeId,
        q_bytes: u64,
        q_cap_bytes: u64,
    ) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::Drop {
                link_from: from.0,
                link_to: to.0,
                q_bytes,
                q_cap_bytes,
            },
        });
    }

    pub(crate) fn viz_tx_start(
        &mut self,
        t: SimTime,
        pkt: &Packet,
        from: NodeId,
        to: NodeId,
        depart: SimTime,
        arrive: SimTime,
    ) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::TxStart {
                link_from: from.0,
                link_to: to.0,
                depart_ns: depart.0,
                arrive_ns: arrive.0,
            },
        });
    }

    pub(crate) fn viz_delivered(&mut self, t: SimTime, pkt: &Packet, node: NodeId) {
        self.viz_push(VizEvent {
            t_ns: t.0,
            pkt_id: Some(pkt.id),
            flow_id: Some(pkt.flow_id),
            pkt_bytes: Some(pkt.size_bytes),
            pkt_kind: Some(Self::pkt_kind(pkt)),
            kind: VizEventKind::Delivered { node: node.0 },
        });
    }
}
