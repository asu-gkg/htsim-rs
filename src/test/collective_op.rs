use crate::cc::collective::CollectiveOp;

#[test]
fn collective_op_steps_edge_cases() {
    // ranks=0/1 should not underflow.
    for op in [
        CollectiveOp::Allreduce,
        CollectiveOp::Allgather,
        CollectiveOp::Reducescatter,
        CollectiveOp::Alltoall,
    ] {
        assert_eq!(op.total_steps(0), 0);
        assert_eq!(op.total_steps(1), 0);
    }

    assert_eq!(CollectiveOp::Allreduce.total_steps(2), 2);
    assert_eq!(CollectiveOp::Allgather.total_steps(2), 1);
    assert_eq!(CollectiveOp::Reducescatter.total_steps(2), 1);
    assert_eq!(CollectiveOp::Alltoall.total_steps(2), 1);
}

#[test]
fn collective_op_chunk_bytes_rounds_up() {
    let comm_bytes = 101;
    let ranks = 4;
    assert_eq!(CollectiveOp::Allreduce.chunk_bytes(comm_bytes, ranks), 26);
    assert_eq!(
        CollectiveOp::Reducescatter.chunk_bytes(comm_bytes, ranks),
        26
    );
    assert_eq!(CollectiveOp::Alltoall.chunk_bytes(comm_bytes, ranks), 26);
    assert_eq!(
        CollectiveOp::Allgather.chunk_bytes(comm_bytes, ranks),
        comm_bytes
    );
}

#[test]
fn collective_op_chunk_bytes_ranks_one_is_identity() {
    let comm_bytes = 123;
    let ranks = 1;
    assert_eq!(
        CollectiveOp::Allreduce.chunk_bytes(comm_bytes, ranks),
        comm_bytes
    );
    assert_eq!(
        CollectiveOp::Reducescatter.chunk_bytes(comm_bytes, ranks),
        comm_bytes
    );
    assert_eq!(
        CollectiveOp::Alltoall.chunk_bytes(comm_bytes, ranks),
        comm_bytes
    );
    assert_eq!(
        CollectiveOp::Allgather.chunk_bytes(comm_bytes, ranks),
        comm_bytes
    );
}

#[test]
fn parse_collective_op_trims_whitespace() {
    assert_eq!(
        CollectiveOp::parse("  all_to_all \n").unwrap(),
        CollectiveOp::Alltoall
    );
    assert_eq!(
        CollectiveOp::parse(" allreduce_async ").unwrap(),
        CollectiveOp::Allreduce
    );
    assert_eq!(
        CollectiveOp::parse(" \treduce-scatter\t").unwrap(),
        CollectiveOp::Reducescatter
    );
    assert_eq!(CollectiveOp::parse("   ").unwrap(), CollectiveOp::Allreduce);
}
