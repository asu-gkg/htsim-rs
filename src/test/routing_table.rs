use crate::net::{NodeId, RoutingTable};
use std::collections::HashSet;

fn build_rev_adj(adj: &[Vec<NodeId>]) -> Vec<Vec<NodeId>> {
    let mut rev = vec![Vec::new(); adj.len()];
    for (from, nbrs) in adj.iter().enumerate() {
        for &to in nbrs {
            rev[to.0].push(NodeId(from));
        }
    }
    rev
}

#[test]
fn routing_table_builds_next_hops_for_shortest_paths() {
    // Diamond:
    // 0 -> 1 -> 3
    //  \-> 2 ->/
    let adj = vec![
        vec![NodeId(1), NodeId(2)],
        vec![NodeId(3)],
        vec![NodeId(3)],
        vec![],
    ];
    let rev_adj = build_rev_adj(&adj);

    let mut rt = RoutingTable::new(0);
    rt.ensure_built(&adj, &rev_adj);

    let nh_03: HashSet<NodeId> = rt
        .next_hops(NodeId(0), NodeId(3))
        .expect("next_hops(0,3)")
        .iter()
        .copied()
        .collect();
    assert_eq!(nh_03, HashSet::from([NodeId(1), NodeId(2)]));

    assert_eq!(rt.next_hops(NodeId(0), NodeId(1)).unwrap(), &[NodeId(1)]);
    assert_eq!(rt.next_hops(NodeId(0), NodeId(2)).unwrap(), &[NodeId(2)]);
    assert_eq!(rt.next_hops(NodeId(1), NodeId(3)).unwrap(), &[NodeId(3)]);
    assert_eq!(rt.next_hops(NodeId(2), NodeId(3)).unwrap(), &[NodeId(3)]);

    assert!(rt.next_hops(NodeId(3), NodeId(0)).is_none());
    assert!(rt.next_hops(NodeId(0), NodeId(0)).is_none());
}

#[test]
fn routing_table_pick_ecmp_is_deterministic_and_within_candidates() {
    let rt = RoutingTable::new(123);
    let cands = [NodeId(1), NodeId(2)];

    let a = rt.pick_ecmp(NodeId(0), NodeId(3), 999, &cands);
    let b = rt.pick_ecmp(NodeId(0), NodeId(3), 999, &cands);
    assert_eq!(a, b);
    assert!(cands.contains(&a));
}

#[test]
fn routing_table_requires_mark_dirty_to_rebuild() {
    let mut adj = vec![
        vec![NodeId(1), NodeId(2)],
        vec![NodeId(3)],
        vec![NodeId(3)],
        vec![],
    ];
    let mut rev_adj = build_rev_adj(&adj);

    let mut rt = RoutingTable::new(0);
    rt.ensure_built(&adj, &rev_adj);
    let before: HashSet<NodeId> = rt
        .next_hops(NodeId(0), NodeId(3))
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert_eq!(before, HashSet::from([NodeId(1), NodeId(2)]));

    // Mutate topology (remove 0->2 edge) but do not mark dirty.
    adj[0] = vec![NodeId(1)];
    rev_adj = build_rev_adj(&adj);
    rt.ensure_built(&adj, &rev_adj);
    let still_stale: HashSet<NodeId> = rt
        .next_hops(NodeId(0), NodeId(3))
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert_eq!(still_stale, HashSet::from([NodeId(1), NodeId(2)]));

    rt.mark_dirty();
    rt.ensure_built(&adj, &rev_adj);
    let after: HashSet<NodeId> = rt
        .next_hops(NodeId(0), NodeId(3))
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert_eq!(after, HashSet::from([NodeId(1)]));
}

#[test]
fn routing_table_pick_ecmp_respects_hash_salt_for_some_key() {
    // With different salts, some key should map to a different candidate.
    let cands = [NodeId(1), NodeId(2)];
    let rt0 = RoutingTable::new(0);
    let rt1 = RoutingTable::new(1);

    let mut found = None;
    for key in 0..10_000u64 {
        let a = rt0.pick_ecmp_with_key(NodeId(0), NodeId(3), key, &cands);
        let b = rt1.pick_ecmp_with_key(NodeId(0), NodeId(3), key, &cands);
        if a != b {
            found = Some((key, a, b));
            break;
        }
    }

    assert!(
        found.is_some(),
        "expected at least one key to differ between salts"
    );
}

#[test]
fn routing_table_pick_ecmp_with_single_candidate_always_returns_it() {
    let rt = RoutingTable::new(0);
    let cands = [NodeId(7)];
    for key in 0..100u64 {
        assert_eq!(
            rt.pick_ecmp_with_key(NodeId(1), NodeId(2), key, &cands),
            NodeId(7)
        );
    }
}
