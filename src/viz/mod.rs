//! 可视化事件记录（用于离线 HTML 回放）
//!
//! 设计目标：
//! - **结构化**：用 JSON 事件而不是解析文本日志
//! - **轻量**：不引入复杂依赖/运行时服务
//! - **可回放**：支持时间轴播放、单步、过滤（pkt/flow）

mod types;

pub use types::{VizCwndReason, VizEvent, VizEventKind, VizLinkInfo, VizLogger, VizNodeInfo, VizNodeKind, VizPacketKind, VizTcp};
