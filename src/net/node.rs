//! 节点类型
//!
//! 定义网络节点，包括节点 trait 和具体实现（主机、交换机）。

use super::id::NodeId;
use super::network::Network;
use super::packet::Packet;
use crate::sim::Simulator;
use tracing::{debug, info, trace};

/// 节点接口
pub trait Node: Send {
    /// 获取节点标识符
    fn id(&self) -> NodeId;

    /// 获取节点名称
    fn name(&self) -> &str;

    /// 处理到达的数据包
    fn on_packet(&mut self, pkt: Packet, sim: &mut Simulator, net: &mut Network);
}

/// 主机节点
#[derive(Debug)]
pub struct Host {
    id: NodeId,
    name: String,
}

impl Host {
    /// 创建新主机
    pub fn new(id: NodeId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }
}

impl Node for Host {
    fn id(&self) -> NodeId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    #[tracing::instrument(skip(self, sim, net), fields(node_name = %self.name(), node_id = ?self.id(), pkt_id = pkt.id, flow_id = pkt.flow_id))]
    fn on_packet(&mut self, pkt: Packet, sim: &mut Simulator, net: &mut Network) {
        debug!("🖥️  Host 处理数据包");
        trace!(
            dst = ?pkt.dst,
            hops_taken = pkt.hops_taken,
            "数据包信息"
        );
        
        if self.id != pkt.dst {
            debug!("未到达目的地，继续转发");
            net.forward_from(self.id, pkt, sim);
        } else {
            info!("已到达目的地，标记为已送达");
            net.on_delivered(pkt);
        }
    }
}

/// 交换机节点
#[derive(Debug)]
pub struct Switch {
    id: NodeId,
    name: String,
}

impl Switch {
    /// 创建新交换机
    pub fn new(id: NodeId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }
}

impl Node for Switch {
    fn id(&self) -> NodeId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    #[tracing::instrument(skip(self, sim, net), fields(node_name = %self.name(), node_id = ?self.id(), pkt_id = pkt.id, flow_id = pkt.flow_id))]
    fn on_packet(&mut self, pkt: Packet, sim: &mut Simulator, net: &mut Network) {
        debug!("🔀 Switch 处理数据包");
        trace!(
            dst = ?pkt.dst,
            hops_taken = pkt.hops_taken,
            "数据包信息"
        );
        
        if self.id != pkt.dst {
            debug!("未到达目的地，继续转发");
            net.forward_from(self.id, pkt, sim);
        } else {
            info!("已到达目的地，标记为已送达");
            net.on_delivered(pkt);
        }
    }
}