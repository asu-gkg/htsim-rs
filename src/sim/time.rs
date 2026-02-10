//! 仿真时间类型
//!
//! 定义仿真时间及其单位转换。

/// 仿真时间（纳秒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct SimTime(pub u64);

impl SimTime {
    pub const ZERO: SimTime = SimTime(0);
    pub fn from_micros(us: u64) -> SimTime {
        SimTime(us.saturating_mul(1_000))
    }
    pub fn from_millis(ms: u64) -> SimTime {
        SimTime(ms.saturating_mul(1_000_000))
    }
    pub fn from_secs(s: u64) -> SimTime {
        SimTime(s.saturating_mul(1_000_000_000))
    }
}
