//! 数据包交付事件
//!
//! 定义网络模拟中的数据包交付事件。

use super::id::NodeId;
use super::packet::Packet;
use super::net_world::NetWorld;
use crate::sim::{Event, Simulator, World};
use tracing::{debug, info, trace};

/// 事件：把一个 packet 交给某个节点处理。
#[derive(Debug)]
pub struct DeliverPacket {
    pub to: NodeId,
    pub pkt: Packet,
}

impl Event for DeliverPacket {
    #[tracing::instrument(skip(self, sim, world), fields(pkt_id = self.pkt.id, flow_id = self.pkt.flow_id, to = ?self.to))]
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let DeliverPacket { to, pkt } = *self;
        
        info!("📨 数据包到达事件执行");
        debug!(
            pkt_id = pkt.id,
            flow_id = pkt.flow_id,
            size_bytes = pkt.size_bytes,
            dst = ?pkt.dst,
            hops_taken = pkt.hops_taken,
            to = ?to,
            now = ?sim.now(),
            "数据包到达节点"
        );
        
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");
        w.net.deliver(to, pkt, sim);
        
        trace!("DeliverPacket::execute 完成");
    }
}