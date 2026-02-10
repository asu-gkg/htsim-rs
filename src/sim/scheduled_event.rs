//! 调度事件
//!
//! 定义调度事件结构及其优先级比较。

use super::event::Event;
use super::time::SimTime;
use std::cmp::Ordering;

/// 调度事件，包含执行时间、序列号和事件对象。
pub struct ScheduledEvent {
    pub(crate) at: SimTime,
    pub(crate) seq: u64,
    pub(crate) ev: Box<dyn Event>,
}

// BinaryHeap 是 max-heap；我们需要最小时间优先，因此反向比较。
impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.at.cmp(&other.at) {
            Ordering::Equal => self.seq.cmp(&other.seq),
            ord => ord,
        }
        .reverse()
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.seq == other.seq
    }
}

impl Eq for ScheduledEvent {}
