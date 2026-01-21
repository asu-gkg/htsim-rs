//! Network-facing API used by protocol stacks.

use crate::sim::Simulator;

use super::{NodeId, Packet};

/// Minimal network API for protocol stacks.
pub trait NetApi {
    fn make_packet(&mut self, flow_id: u64, size_bytes: u32, route: Vec<NodeId>) -> Packet;
    fn make_packet_dynamic(&mut self, flow_id: u64, size_bytes: u32, src: NodeId, dst: NodeId) -> Packet;
    fn forward_from(&mut self, from: NodeId, pkt: Packet, sim: &mut Simulator);

    fn viz_tcp_send_data(&mut self, t_ns: u64, conn_id: u64, seq: u64, len: u32, retrans: bool);
    fn viz_tcp_send_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool);
    fn viz_tcp_recv_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool);
    fn viz_tcp_rto(&mut self, t_ns: u64, conn_id: u64, seq: u64);
    fn viz_dctcp_cwnd(
        &mut self,
        t_ns: u64,
        conn_id: u64,
        cwnd_bytes: u64,
        ssthresh_bytes: u64,
        inflight_bytes: u64,
        alpha: f64,
    );
}

impl NetApi for super::Network {
    fn make_packet(&mut self, flow_id: u64, size_bytes: u32, route: Vec<NodeId>) -> Packet {
        super::Network::make_packet(self, flow_id, size_bytes, route)
    }

    fn make_packet_dynamic(&mut self, flow_id: u64, size_bytes: u32, src: NodeId, dst: NodeId) -> Packet {
        super::Network::make_packet_dynamic(self, flow_id, size_bytes, src, dst)
    }

    fn forward_from(&mut self, from: NodeId, pkt: Packet, sim: &mut Simulator) {
        super::Network::forward_from(self, from, pkt, sim)
    }

    fn viz_tcp_send_data(&mut self, t_ns: u64, conn_id: u64, seq: u64, len: u32, retrans: bool) {
        self.viz_tcp_send_data(t_ns, conn_id, seq, len, retrans)
    }

    fn viz_tcp_send_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool) {
        self.viz_tcp_send_ack(t_ns, conn_id, ack, ecn_echo)
    }

    fn viz_tcp_recv_ack(&mut self, t_ns: u64, conn_id: u64, ack: u64, ecn_echo: bool) {
        self.viz_tcp_recv_ack(t_ns, conn_id, ack, ecn_echo)
    }

    fn viz_tcp_rto(&mut self, t_ns: u64, conn_id: u64, seq: u64) {
        self.viz_tcp_rto(t_ns, conn_id, seq)
    }

    fn viz_dctcp_cwnd(
        &mut self,
        t_ns: u64,
        conn_id: u64,
        cwnd_bytes: u64,
        ssthresh_bytes: u64,
        inflight_bytes: u64,
        alpha: f64,
    ) {
        self.viz_dctcp_cwnd(
            t_ns,
            conn_id,
            cwnd_bytes,
            ssthresh_bytes,
            inflight_bytes,
            alpha,
        )
    }
}
