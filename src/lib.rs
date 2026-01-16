pub mod net;
pub mod sim;
pub mod demo;

// 导出 demo 模块中的公共类型和函数，供 bin 文件使用
pub use demo::{build_dumbbell, DumbbellOpts, InjectFlow};

