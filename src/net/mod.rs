//! 网络模拟模块
//!
//! 此模块包含网络模拟的核心组件，如节点、链路、数据包和网络拓扑。

// 子模块声明
mod api;
mod deliver_packet;
mod id;
mod link;
mod link_ready;
mod net_world;
mod network;
mod network_proto;
mod network_viz;
mod node;
mod packet;
mod proto_bridge;
mod routing;
mod stats;
mod transport;

// 重新导出公共接口
pub use api::NetApi;
pub use deliver_packet::DeliverPacket;
pub use id::{LinkId, NodeId};
pub use link::Link;
pub use link_ready::LinkReady;
pub use net_world::NetWorld;
pub use network::{EcmpHashMode, Network};
pub use node::{Host, Node, Switch};
pub use packet::{Ecn, Packet};
pub(crate) use proto_bridge::{with_dctcp_stack, with_tcp_stack};
pub use routing::RoutingTable;
pub use stats::Stats;
pub use transport::{DctcpSegment, TcpSegment, Transport};
