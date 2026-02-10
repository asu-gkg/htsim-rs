//! 事件 trait
//!
//! 定义仿真事件接口。

use super::simulator::Simulator;
use super::world::World;

/// 事件：可被调度执行。使用 `self: Box<Self>` 以支持 move/所有权转移。
pub trait Event: Send + 'static {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World);
}
