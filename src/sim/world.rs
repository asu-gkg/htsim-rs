//! 世界 trait
//!
//! 定义仿真世界接口。

use std::any::Any;
use super::simulator::Simulator;

/// 仿真世界：由业务层实现（例如网络拓扑/统计等）。
pub trait World: Any {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn on_tick(&mut self, _sim: &mut Simulator) {}
}