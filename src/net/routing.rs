//! 路由（含 ECMP）支持
//!
//! Rust 版网络模拟最初要求 packet 提前携带完整的 `route`（节点序列），
//! 无法像 C++ 版那样在交换机处使用 FIB/ECMP 动态选择下一跳。
//!
//! 本模块提供一个简单的“按最短跳数”的路由表：为每个 (from, dst)
//! 预计算所有等价最短路径的下一跳集合，用于 ECMP 选择。

use std::collections::{HashMap, VecDeque};

use super::id::NodeId;

#[derive(Debug, Default, Clone)]
pub struct RoutingTable {
    dirty: bool,
    /// (from, dst) -> 多个等价最短路径下一跳
    next_hops: HashMap<(NodeId, NodeId), Vec<NodeId>>,
    /// 用于 ECMP hashing 的盐（保证稳定且可控）
    hash_salt: u64,
}

impl RoutingTable {
    pub fn new(hash_salt: u64) -> Self {
        Self {
            dirty: true,
            next_hops: HashMap::new(),
            hash_salt,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 确保路由表基于当前拓扑是最新的。
    ///
    /// `adj[from]` 为从 `from` 出发的所有出边邻居；
    /// `rev_adj[to]` 为所有能到达 `to` 的前驱节点集合。
    pub fn ensure_built(&mut self, adj: &[Vec<NodeId>], rev_adj: &[Vec<NodeId>]) {
        if !self.dirty {
            return;
        }

        let n = adj.len();
        self.next_hops.clear();

        // 对每个 dst 在反向图上做 BFS，得到到 dst 的最短跳数距离 dist[*]。
        // 然后对每个 from，选出所有满足 dist[next] = dist[from] - 1 的 next 作为 ECMP 候选。
        let mut dist: Vec<i32> = vec![i32::MAX; n];
        let mut q: VecDeque<NodeId> = VecDeque::new();

        for dst_idx in 0..n {
            dist.fill(i32::MAX);
            q.clear();

            let dst = NodeId(dst_idx);
            dist[dst_idx] = 0;
            q.push_back(dst);

            while let Some(v) = q.pop_front() {
                let dv = dist[v.0];
                for &pred in &rev_adj[v.0] {
                    if dist[pred.0] == i32::MAX {
                        dist[pred.0] = dv.saturating_add(1);
                        q.push_back(pred);
                    }
                }
            }

            for from_idx in 0..n {
                let from = NodeId(from_idx);
                if from == dst {
                    continue;
                }
                let df = dist[from_idx];
                if df == i32::MAX {
                    continue; // unreachable
                }
                let mut cands = Vec::new();
                for &nh in &adj[from_idx] {
                    if dist[nh.0] == df - 1 {
                        cands.push(nh);
                    }
                }
                if !cands.is_empty() {
                    self.next_hops.insert((from, dst), cands);
                }
            }
        }

        self.dirty = false;
    }

    /// 获取 (from, dst) 的 ECMP 下一跳候选集合。
    pub fn next_hops(&self, from: NodeId, dst: NodeId) -> Option<&[NodeId]> {
        self.next_hops.get(&(from, dst)).map(|v| v.as_slice())
    }

    /// 基于 flow_id 的稳定 ECMP 选择。
    pub fn pick_ecmp(&self, from: NodeId, dst: NodeId, flow_id: u64, cands: &[NodeId]) -> NodeId {
        self.pick_ecmp_with_key(from, dst, flow_id, cands)
    }

    /// 基于任意 key 的稳定 ECMP 选择。
    pub fn pick_ecmp_with_key(
        &self,
        from: NodeId,
        dst: NodeId,
        key: u64,
        cands: &[NodeId],
    ) -> NodeId {
        debug_assert!(!cands.is_empty());
        let h = mix64(
            key ^ (from.0 as u64).wrapping_mul(0x9E3779B97F4A7C15)
                ^ (dst.0 as u64)
                ^ self.hash_salt,
        );
        let idx = (h as usize) % cands.len();
        cands[idx]
    }
}

/// 一个简单、确定性的 64-bit mixing（替代 RandomState，避免每次运行 hash 不稳定）。
fn mix64(mut x: u64) -> u64 {
    // splitmix64
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}
