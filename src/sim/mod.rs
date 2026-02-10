//! 仿真核心模块
//!
//! 此模块包含事件驱动仿真的核心组件，如仿真时间、事件、世界和仿真器。

// 子模块声明
mod event;
mod scheduled_event;
mod simulator;
mod time;
mod workload;
mod world;

// 重新导出公共接口
pub use event::Event;
pub use scheduled_event::ScheduledEvent;
pub use simulator::Simulator;
pub use time::SimTime;
pub use workload::{
    GpuSpec, HostSpec, RankSpec, RankStepKind, RankStepSpec, RoutingMode, SendRecvDirection,
    StepSpec, TopologySpec, TransportProtocol, WorkloadDefaults, WorkloadMeta, WorkloadSpec,
};
pub use world::World;
