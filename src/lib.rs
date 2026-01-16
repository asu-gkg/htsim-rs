pub mod net;
pub mod sim;
pub mod experiments;
pub mod queue;

// 保持原有 API：`htsim_rs::demo::*` 仍可用，但实现放在 experiments 目录里。
pub use experiments::demo;

// 导出 demo 模块中的公共类型和函数，供 bin 文件使用
pub use demo::{build_dumbbell, DumbbellOpts, InjectFlow};

