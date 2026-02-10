//! 队列策略（Queue disciplines）
//!
//! 目前先提供最基础的 DropTail（尾丢弃）队列，后续可以在此扩展 RED/CoDel 等策略。

use crate::net::Packet;

mod drop_tail;
mod priority;

pub use drop_tail::DropTailQueue;
pub use priority::PriorityQueue;

pub const DEFAULT_PKT_BYTES: u64 = 1500;

pub fn mem_from_pkt(pkts: u64) -> u64 {
    pkts.saturating_mul(DEFAULT_PKT_BYTES)
}

/// Packet 队列抽象
pub trait PacketQueue: std::fmt::Debug {
    /// 入队：成功返回 Ok；若被丢弃则返回 Err(pkt)
    fn enqueue(&mut self, pkt: Packet) -> Result<(), Packet>;
    /// 出队：按队列策略返回下一个 packet
    fn dequeue(&mut self) -> Option<Packet>;

    fn len(&self) -> usize;
    fn bytes(&self) -> u64;
    fn capacity_bytes(&self) -> u64;
}
