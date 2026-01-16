//! 仿真核心模块
//!
//! 此模块包含事件驱动仿真的核心组件，如仿真时间、事件、世界和仿真器。

// 子模块声明
mod time;
mod event;
mod world;
mod scheduled_event;
mod simulator;

// 重新导出公共接口
pub use time::SimTime;
pub use event::Event;
pub use world::World;
pub use scheduled_event::ScheduledEvent;
pub use simulator::Simulator;