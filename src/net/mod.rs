//! 网络模拟模块
//!
//! 此模块包含网络模拟的核心组件，如节点、链路、数据包和网络拓扑。

// 子模块声明
mod id;
mod packet;
mod transport;
mod api;
mod proto_bridge;
mod node;
mod link;
mod stats;
mod network;
mod network_viz;
mod network_proto;
mod deliver_packet;
mod net_world;
mod link_ready;
mod routing;

// 重新导出公共接口
pub use id::{NodeId, LinkId};
pub use packet::{Ecn, Packet};
pub use transport::{DctcpSegment, TcpSegment, Transport};
pub use api::NetApi;
pub(crate) use proto_bridge::{with_dctcp_stack, with_tcp_stack};
pub use node::{Node, Host, Switch};
pub use link::Link;
pub use stats::Stats;
pub use network::{EcmpHashMode, Network};
pub use deliver_packet::DeliverPacket;
pub use net_world::NetWorld;
pub use link_ready::LinkReady;
pub use routing::RoutingTable;
