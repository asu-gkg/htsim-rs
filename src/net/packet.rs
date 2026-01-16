//! 数据包类型
//!
//! 定义网络数据包及其相关操作。

use super::id::NodeId;

/// 网络数据包
#[derive(Debug, Clone)]
pub struct Packet {
    pub id: u64,
    pub flow_id: u64,
    pub size_bytes: u32,
    pub route: Vec<NodeId>,
    pub hop: usize, // 当前所在节点在 route 中的索引
}

impl Packet {
    /// 获取源节点
    pub fn src(&self) -> NodeId {
        self.route[0]
    }

    /// 获取目标节点
    pub fn dst(&self) -> NodeId {
        *self.route.last().expect("route non-empty")
    }

    /// 获取当前所在节点
    pub fn at(&self) -> NodeId {
        self.route[self.hop]
    }

    /// 检查是否有下一跳
    pub fn has_next(&self) -> bool {
        self.hop + 1 < self.route.len()
    }

    /// 获取下一跳节点（如果有）
    pub fn next(&self) -> Option<NodeId> {
        self.route.get(self.hop + 1).copied()
    }

    /// 前进到下一跳
    pub fn advance(mut self) -> Self {
        self.hop += 1;
        self
    }
}