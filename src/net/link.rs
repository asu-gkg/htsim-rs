//! 链路类型
//!
//! 定义网络链路及其传输时延计算。

use super::id::NodeId;
use crate::sim::SimTime;

/// 网络链路
#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub from: NodeId,
    pub to: NodeId,
    pub latency: SimTime,
    pub bandwidth_bps: u64,
    pub busy_until: SimTime,
}

impl Link {
    /// 创建新链路
    pub fn new(from: NodeId, to: NodeId, latency: SimTime, bandwidth_bps: u64) -> Self {
        Self {
            from,
            to,
            latency,
            bandwidth_bps,
            busy_until: SimTime::ZERO,
        }
    }

    /// 计算传输指定字节数所需的时间
    pub(crate) fn tx_time(&self, bytes: u32) -> SimTime {
        // ceil(bytes*8 / bps) 秒 -> 纳秒
        if self.bandwidth_bps == 0 {
            return SimTime(u64::MAX / 4);
        }
        let bits = (bytes as u128).saturating_mul(8);
        let nanos = (bits.saturating_mul(1_000_000_000u128)
            + (self.bandwidth_bps as u128 - 1))
            / self.bandwidth_bps as u128;
        SimTime(nanos.min(u64::MAX as u128) as u64)
    }
}