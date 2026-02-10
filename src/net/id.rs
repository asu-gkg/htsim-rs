//! 标识符类型
//!
//! 定义节点和链路的唯一标识符。

/// 节点标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// 链路标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkId(pub usize);
