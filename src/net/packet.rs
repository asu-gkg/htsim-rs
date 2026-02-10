//! 数据包类型
//!
//! 定义网络数据包及其相关操作。

use super::id::NodeId;
use super::transport::Transport;

/// 网络数据包
#[derive(Debug, Clone)]
pub struct Packet {
    pub id: u64,
    pub flow_id: u64,
    pub size_bytes: u32,
    pub src: NodeId,
    pub dst: NodeId,
    /// ECN 标记（网络层）
    pub ecn: Ecn,
    pub routing: Routing,
    /// 传输层标签（例如 TCP 段）。默认 `None`，保持与原有“裸包”逻辑兼容。
    pub transport: Transport,
    /// 已经走过的 hop 数（用于调试/统计）
    pub hops_taken: u32,
}

/// ECN 码点（简化：只区分 Not-ECT / ECT / CE）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecn {
    NotEct,
    Ect0,
    Ce,
}

impl Ecn {
    pub fn is_ect(self) -> bool {
        matches!(self, Ecn::Ect0)
    }

    pub fn is_ce(self) -> bool {
        matches!(self, Ecn::Ce)
    }
}

/// 路由信息：支持预设、动态以及混合（前缀预设 + 余下动态）
#[derive(Debug, Clone)]
pub enum Routing {
    /// 全路径预设（等价于 C++ SINGLE_PATH：packet 携带完整 route）
    Preset { path: Vec<NodeId>, idx: usize },
    /// 完全动态（等价于 C++ ECMP_FIB：packet 不携带 route，由网络/交换机按 FIB/ECMP 选下一跳）
    Dynamic,
    /// 混合：先沿着 prefix（预设前缀）走，prefix 用完后按动态路由到 dst
    Mixed { prefix: Vec<NodeId>, idx: usize },
}

impl Packet {
    /// 预设全路径 packet（兼容旧接口：传入 Vec<NodeId>）
    pub fn new_preset(id: u64, flow_id: u64, size_bytes: u32, path: Vec<NodeId>) -> Self {
        let src = path[0];
        let dst = *path.last().expect("route non-empty");
        Self {
            id,
            flow_id,
            size_bytes,
            src,
            dst,
            ecn: Ecn::NotEct,
            routing: Routing::Preset { path, idx: 0 },
            transport: Transport::None,
            hops_taken: 0,
        }
    }

    /// 纯动态路由 packet（每一跳按 FIB/ECMP 选择下一跳）
    pub fn new_dynamic(id: u64, flow_id: u64, size_bytes: u32, src: NodeId, dst: NodeId) -> Self {
        Self {
            id,
            flow_id,
            size_bytes,
            src,
            dst,
            ecn: Ecn::NotEct,
            routing: Routing::Dynamic,
            transport: Transport::None,
            hops_taken: 0,
        }
    }

    /// 混合路由 packet：先走 prefix，再动态到 dst
    pub fn new_mixed(
        id: u64,
        flow_id: u64,
        size_bytes: u32,
        prefix: Vec<NodeId>,
        dst: NodeId,
    ) -> Self {
        let src = prefix[0];
        Self {
            id,
            flow_id,
            size_bytes,
            src,
            dst,
            ecn: Ecn::NotEct,
            routing: Routing::Mixed { prefix, idx: 0 },
            transport: Transport::None,
            hops_taken: 0,
        }
    }

    /// 若支持 ECN，则标记为 CE
    pub fn mark_ce_if_ect(&mut self) {
        if self.ecn.is_ect() {
            self.ecn = Ecn::Ce;
        }
    }

    /// 如果当前仍在预设前缀/路径上，返回下一跳；否则返回 None（表示需要动态选路或已到达）。
    pub fn preset_next(&self) -> Option<NodeId> {
        match &self.routing {
            Routing::Preset { path, idx } => path.get(idx.saturating_add(1)).copied(),
            Routing::Mixed { prefix, idx } => prefix.get(idx.saturating_add(1)).copied(),
            Routing::Dynamic => None,
        }
    }

    /// 前进一步：用于在已确定下一跳后更新 routing 状态。
    pub fn advance(mut self) -> Self {
        self.hops_taken = self.hops_taken.saturating_add(1);
        match &mut self.routing {
            Routing::Preset { idx, .. } => *idx = idx.saturating_add(1),
            Routing::Mixed { idx, prefix } => {
                // 仅当还在 prefix 内时才前进 idx；越界后视为进入动态阶段
                if idx.saturating_add(1) < prefix.len() {
                    *idx = idx.saturating_add(1);
                } else {
                    *idx = prefix.len();
                }
            }
            Routing::Dynamic => {}
        }
        self
    }
}
