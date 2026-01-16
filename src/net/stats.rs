//! 统计信息
//!
//! 定义网络仿真统计数据结构。

/// 网络统计信息
#[derive(Debug, Default)]
pub struct Stats {
    pub delivered_pkts: u64,
    pub delivered_bytes: u64,
}