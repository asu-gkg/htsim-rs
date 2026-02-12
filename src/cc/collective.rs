//! Helpers for collective communication operations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectiveOp {
    Allreduce,
    Allgather,
    Reducescatter,
    Alltoall,
}

impl CollectiveOp {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let normalized = raw.trim().to_lowercase();
        if normalized.is_empty() {
            return Ok(Self::Allreduce);
        }
        let compact: String = normalized
            .chars()
            .filter(|ch| *ch != '_' && *ch != '-')
            .collect();
        let compact = compact.as_str();
        let compact = compact.strip_suffix("async").unwrap_or(compact);
        match compact {
            "allreduce" => Ok(Self::Allreduce),
            "allgather" => Ok(Self::Allgather),
            "reducescatter" => Ok(Self::Reducescatter),
            "alltoall" => Ok(Self::Alltoall),
            _ => Err(format!("unknown collective op: {raw}")),
        }
    }

    pub fn total_steps(self, ranks: usize) -> usize {
        let steps = ranks.saturating_sub(1);
        match self {
            Self::Allreduce => steps.saturating_mul(2),
            Self::Allgather | Self::Reducescatter | Self::Alltoall => steps,
        }
    }

    pub fn chunk_bytes(self, comm_bytes: u64, ranks: usize) -> u64 {
        match self {
            // Ring allreduce/reduce-scatter use ceil(comm_bytes / ranks) per step.
            Self::Allreduce | Self::Reducescatter => div_ceil(comm_bytes, ranks.max(1) as u64),
            // For ring allgather we treat comm_bytes as the per-rank contribution size.
            Self::Allgather => comm_bytes,
            // All-to-all splits the per-rank buffer across all ranks, including the local part.
            Self::Alltoall => div_ceil(comm_bytes, ranks.max(1) as u64),
        }
    }
}

fn div_ceil(n: u64, d: u64) -> u64 {
    if d <= 1 {
        return n;
    }
    n.saturating_add(d.saturating_sub(1)) / d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_collective_op_aliases() {
        assert_eq!(
            CollectiveOp::parse("allreduce").unwrap(),
            CollectiveOp::Allreduce
        );
        assert_eq!(
            CollectiveOp::parse("allreduce_async").unwrap(),
            CollectiveOp::Allreduce
        );
        assert_eq!(
            CollectiveOp::parse("ALLREDUCE_ASYNC").unwrap(),
            CollectiveOp::Allreduce
        );
        assert_eq!(
            CollectiveOp::parse("ALLGATHER").unwrap(),
            CollectiveOp::Allgather
        );
        assert_eq!(
            CollectiveOp::parse("reduce_scatter").unwrap(),
            CollectiveOp::Reducescatter
        );
        assert_eq!(
            CollectiveOp::parse("reduce-scatter").unwrap(),
            CollectiveOp::Reducescatter
        );
        assert_eq!(
            CollectiveOp::parse("all_to_all").unwrap(),
            CollectiveOp::Alltoall
        );
        assert_eq!(CollectiveOp::parse("").unwrap(), CollectiveOp::Allreduce);
        assert!(CollectiveOp::parse("mystery").is_err());
    }

    #[test]
    fn steps_and_chunks() {
        let ranks = 4;
        assert_eq!(CollectiveOp::Allreduce.total_steps(ranks), 6);
        assert_eq!(CollectiveOp::Allgather.total_steps(ranks), 3);
        assert_eq!(CollectiveOp::Alltoall.total_steps(ranks), 3);

        assert_eq!(CollectiveOp::Allreduce.chunk_bytes(100, ranks), 25);
        assert_eq!(CollectiveOp::Allgather.chunk_bytes(100, ranks), 100);
        assert_eq!(CollectiveOp::Alltoall.chunk_bytes(100, ranks), 25);
    }
}
