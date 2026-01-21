//! Protocol dispatch hooks for the network.

use crate::sim::Simulator;
use tracing::{debug, info};

use super::{Network, NodeId, Packet, Transport};

impl Network {
    /// 数据包送达目的地时的处理
    #[tracing::instrument(skip(self, sim), fields(pkt_id = pkt.id, flow_id = pkt.flow_id))]
    pub(crate) fn on_delivered(&mut self, at: NodeId, pkt: Packet, sim: &mut Simulator) {
        info!("✅ 数据包送达目的地");

        self.viz_delivered(sim.now(), &pkt, at);

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
        } else if let Transport::Dctcp(seg) = pkt.transport {
            let conn_id = pkt.flow_id;
            let ecn = pkt.ecn;
            let mut dctcp = std::mem::take(&mut self.dctcp);
            dctcp.on_dctcp_segment(conn_id, at, seg, ecn, sim, self);
            self.dctcp = dctcp;
        }
    }
}
