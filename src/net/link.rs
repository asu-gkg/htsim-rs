//! 链路类型
//!
//! 定义网络链路及其传输时延计算。

use super::id::NodeId;
use crate::queue::{DEFAULT_PKT_BYTES, PacketQueue, PriorityQueue};
use crate::sim::SimTime;

// Default to a very large buffer so links behave as "almost infinite"
// unless experiments explicitly set a smaller capacity (e.g., to induce drops).
const DEFAULT_LINK_QUEUE_PKTS: u64 = 1_000_000;
const DEFAULT_LINK_QUEUE_BYTES: u64 = DEFAULT_LINK_QUEUE_PKTS * DEFAULT_PKT_BYTES;

/// 网络链路
#[derive(Debug)]
pub struct Link {
    pub from: NodeId,
    pub to: NodeId,
    pub latency: SimTime,
    pub bandwidth_bps: u64,
    pub busy_until: SimTime,
    /// ECN 标记阈值（bytes）。None 表示不开启 ECN 标记。
    pub ecn_threshold_bytes: Option<u64>,
    /// 链路上的排队策略（默认 DropTail，容量极大，行为与旧逻辑一致但可扩展）
    pub queue: Box<dyn PacketQueue>,
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
            ecn_threshold_bytes: None,
            queue: Box::new(PriorityQueue::new(DEFAULT_LINK_QUEUE_BYTES)),
        }
    }

    /// 计算传输指定字节数所需的时间
    pub(crate) fn tx_time(&self, bytes: u32) -> SimTime {
        // ceil(bytes*8 / bps) 秒 -> 纳秒
        if self.bandwidth_bps == 0 {
            return SimTime(u64::MAX / 4);
        }
        let bits = (bytes as u128).saturating_mul(8);
        let nanos = (bits.saturating_mul(1_000_000_000u128) + (self.bandwidth_bps as u128 - 1))
            / self.bandwidth_bps as u128;
        SimTime(nanos.min(u64::MAX as u128) as u64)
    }
}
