//! 网络模拟模块
//!
//! 此模块包含网络模拟的核心组件，如节点、链路、数据包和网络拓扑。

// 子模块声明
mod id;
mod packet;
mod node;
mod link;
mod stats;
mod network;
mod deliver_packet;
mod net_world;

// 重新导出公共接口
pub use id::{NodeId, LinkId};
pub use packet::Packet;
pub use node::{Node, Host, Switch};
pub use link::Link;
pub use stats::Stats;
pub use network::Network;
pub use deliver_packet::DeliverPacket;
pub use net_world::NetWorld;