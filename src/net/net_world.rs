//! 网络世界实现
//!
//! 定义网络仿真的世界（World）实现，持有网络拓扑。

use super::network::Network;
use crate::sim::World;
use std::any::Any;

/// 一个默认的网络世界实现：持有 Network。
#[derive(Default)]
pub struct NetWorld {
    pub net: Network,
}

impl World for NetWorld {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
