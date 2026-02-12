use crate::cc::ring::{self, RingAllreduceConfig, RingDoneCallback, RingTransport, RoutingMode};
use crate::net::{NetWorld, NodeId};
use crate::sim::{Event, SimTime, Simulator, World};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlowStart {
    flow_id: u64,
    src: NodeId,
    dst: NodeId,
    routing: RoutingMode,
    start_at: SimTime,
    done_at: SimTime,
    chunk_bytes: u64,
}

struct CallDone {
    done: RingDoneCallback,
}

impl Event for CallDone {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        (self.done)(sim.now(), sim);
    }
}

#[derive(Clone)]
struct RecordingTransport {
    delay: SimTime,
    records: Arc<Mutex<Vec<FlowStart>>>,
}

impl RingTransport for RecordingTransport {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: RoutingMode,
        sim: &mut Simulator,
        _world: &mut NetWorld,
        done: RingDoneCallback,
    ) {
        let start_at = sim.now();
        let done_at = SimTime(start_at.0.saturating_add(self.delay.0));
        if let Ok(mut list) = self.records.lock() {
            list.push(FlowStart {
                flow_id,
                src,
                dst,
                routing,
                start_at,
                done_at,
                chunk_bytes,
            });
        }
        sim.schedule(done_at, CallDone { done });
    }
}

#[derive(Clone)]
struct VariableDelayTransport {
    ranks: usize,
    start_flow_id: u64,
    records: Arc<Mutex<Vec<FlowStart>>>,
}

impl RingTransport for VariableDelayTransport {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: RoutingMode,
        sim: &mut Simulator,
        _world: &mut NetWorld,
        done: RingDoneCallback,
    ) {
        let start_at = sim.now();
        let ranks = self.ranks.max(1) as u64;
        let rel = flow_id.saturating_sub(self.start_flow_id);
        let step = rel / ranks;
        let rank = rel % ranks;
        let delay = SimTime::from_micros(step.saturating_add(rank).saturating_add(1));
        let done_at = SimTime(start_at.0.saturating_add(delay.0));

        if let Ok(mut list) = self.records.lock() {
            list.push(FlowStart {
                flow_id,
                src,
                dst,
                routing,
                start_at,
                done_at,
                chunk_bytes,
            });
        }
        sim.schedule(done_at, CallDone { done });
    }
}

fn run_collective(
    ranks: usize,
    start_flow_id: u64,
    delay: SimTime,
    start: fn(&mut Simulator, RingAllreduceConfig) -> ring::RingAllreduceHandle,
) -> (
    ring::RingAllreduceHandle,
    Arc<Mutex<Vec<FlowStart>>>,
    SimTime,
) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let transport = RecordingTransport {
        delay,
        records: Arc::clone(&records),
    };
    let cfg = RingAllreduceConfig {
        ranks,
        hosts: (0..ranks).map(NodeId).collect(),
        chunk_bytes: 123,
        routing: RoutingMode::PerFlow,
        start_flow_id,
        transport: Box::new(transport),
        done_cb: None,
    };

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();
    let handle = start(&mut sim, cfg);
    sim.run(&mut world);
    (handle, records, sim.now())
}

fn run_collective_at(
    ranks: usize,
    start_flow_id: u64,
    delay: SimTime,
    chunk_bytes: u64,
    start_at: SimTime,
    done_cb: Option<ring::RingAllreduceDoneCallback>,
    start: fn(&mut Simulator, RingAllreduceConfig, SimTime) -> ring::RingAllreduceHandle,
) -> (
    ring::RingAllreduceHandle,
    Arc<Mutex<Vec<FlowStart>>>,
    SimTime,
) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let transport = RecordingTransport {
        delay,
        records: Arc::clone(&records),
    };
    let cfg = RingAllreduceConfig {
        ranks,
        hosts: (0..ranks).map(NodeId).collect(),
        chunk_bytes,
        routing: RoutingMode::PerFlow,
        start_flow_id,
        transport: Box::new(transport),
        done_cb,
    };

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();
    let handle = start(&mut sim, cfg, start_at);
    sim.run(&mut world);
    (handle, records, sim.now())
}

fn time_mul(t: SimTime, k: u64) -> SimTime {
    SimTime(t.0.saturating_mul(k))
}

fn time_add(a: SimTime, b: SimTime) -> SimTime {
    SimTime(a.0.saturating_add(b.0))
}

#[test]
fn ring_alltoall_covers_all_pairs() {
    let ranks = 4;
    let (handle, records, final_time) = run_collective(
        ranks,
        10,
        SimTime::from_micros(1),
        ring::start_ring_alltoall,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, ranks - 1);

    let list = records.lock().expect("records lock");
    assert_eq!(list.len(), ranks * (ranks - 1));
    assert!(final_time > SimTime::ZERO);

    let mut seen_pairs = HashMap::<(usize, usize), usize>::new();
    let mut dst_by_src = HashMap::<usize, HashSet<usize>>::new();
    for rec in list.iter() {
        let src = rec.src.0;
        let dst = rec.dst.0;
        assert_ne!(src, dst);
        *seen_pairs.entry((src, dst)).or_default() += 1;
        dst_by_src.entry(src).or_default().insert(dst);
    }

    for src in 0..ranks {
        let dsts = dst_by_src.get(&src).expect("missing src records");
        assert_eq!(dsts.len(), ranks - 1);
        for dst in 0..ranks {
            if dst == src {
                continue;
            }
            assert_eq!(
                seen_pairs.get(&(src, dst)).copied().unwrap_or(0),
                1,
                "pair src={src} dst={dst} not seen exactly once"
            );
        }
    }
}

#[test]
fn ring_allreduce_uses_neighbor_routing() {
    let ranks = 4;
    let (handle, records, _final_time) = run_collective(
        ranks,
        100,
        SimTime::from_micros(1),
        ring::start_ring_allreduce,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, (ranks - 1) * 2);

    let list = records.lock().expect("records lock");
    assert_eq!(list.len(), ranks * stats.total_steps);
    for rec in list.iter() {
        assert_eq!(rec.dst.0, (rec.src.0 + 1) % ranks);
    }
}

#[test]
fn ring_allreduce_reduce_done_at_before_done_at() {
    let ranks = 4;
    let (handle, _records, _final_time) = run_collective(
        ranks,
        1000,
        SimTime::from_micros(10),
        ring::start_ring_allreduce,
    );
    let stats = handle.stats();
    let reduce_done_at = stats.reduce_done_at.expect("reduce_done_at missing");
    let done_at = stats.done_at.expect("done_at missing");
    assert!(reduce_done_at < done_at);
}

#[test]
fn ring_reducescatter_reduce_done_at_equals_done_at() {
    let ranks = 4;
    let (handle, _records, _final_time) = run_collective(
        ranks,
        2000,
        SimTime::from_micros(10),
        ring::start_ring_reducescatter,
    );
    let stats = handle.stats();
    assert_eq!(
        stats.reduce_done_at.expect("reduce_done_at missing"),
        stats.done_at.expect("done_at missing")
    );
}

#[test]
fn ring_allgather_has_no_reduce_done_at() {
    let ranks = 4;
    let (handle, _records, _final_time) = run_collective(
        ranks,
        3000,
        SimTime::from_micros(10),
        ring::start_ring_allgather,
    );
    let stats = handle.stats();
    assert!(stats.reduce_done_at.is_none());
    assert!(stats.done_at.is_some());
}

#[test]
fn ring_single_rank_finishes_immediately_without_flows() {
    let ranks = 1;
    let (handle, records, final_time) = run_collective(
        ranks,
        4000,
        SimTime::from_micros(1),
        ring::start_ring_alltoall,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, 0);
    assert_eq!(records.lock().expect("records lock").len(), 0);
    assert_eq!(stats.start_at, Some(SimTime::ZERO));
    assert_eq!(stats.done_at, Some(SimTime::ZERO));
    assert_eq!(final_time, SimTime::ZERO);
}

#[test]
fn ring_zero_ranks_finishes_immediately_without_flows() {
    let ranks = 0;
    let (handle, records, final_time) = run_collective(
        ranks,
        5000,
        SimTime::from_micros(1),
        ring::start_ring_alltoall,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, 0);
    assert_eq!(records.lock().expect("records lock").len(), 0);
    assert_eq!(stats.start_at, Some(SimTime::ZERO));
    assert_eq!(stats.done_at, Some(SimTime::ZERO));
    assert_eq!(final_time, SimTime::ZERO);
}

#[test]
fn ring_alltoall_dst_changes_each_step() {
    let ranks = 5;
    let delay = SimTime::from_micros(3);
    let start_at = SimTime::from_micros(7);
    let (handle, records, final_time) = run_collective_at(
        ranks,
        10,
        delay,
        128,
        start_at,
        None,
        ring::start_ring_alltoall_at,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, ranks - 1);
    assert_eq!(stats.start_at, Some(start_at));
    assert_eq!(
        stats.done_at,
        Some(time_add(
            start_at,
            time_mul(delay, stats.total_steps as u64)
        ))
    );
    assert_eq!(final_time, stats.done_at.expect("done_at missing"));

    let list = records.lock().expect("records lock");
    assert_eq!(list.len(), ranks * (ranks - 1));
    assert!(list.iter().all(|rec| rec.chunk_bytes == 128));

    let mut by_step: BTreeMap<SimTime, Vec<FlowStart>> = BTreeMap::new();
    for rec in list.iter().copied() {
        by_step.entry(rec.start_at).or_default().push(rec);
    }
    assert_eq!(by_step.len(), ranks - 1);

    for (step, (step_at, flows)) in by_step.iter().enumerate() {
        let expected_at = time_add(start_at, time_mul(delay, step as u64));
        assert_eq!(*step_at, expected_at);
        assert_eq!(flows.len(), ranks);

        let mut srcs = HashSet::new();
        for rec in flows.iter() {
            srcs.insert(rec.src.0);
            let expected_dst = (rec.src.0 + step + 1) % ranks;
            assert_eq!(rec.dst.0, expected_dst);
        }
        assert_eq!(srcs.len(), ranks);
    }
}

#[test]
fn ring_flow_ids_are_unique_and_contiguous() {
    let ranks = 4;
    let start_flow_id = 42;
    let (handle, records, _final_time) = run_collective(
        ranks,
        start_flow_id,
        SimTime::from_micros(1),
        ring::start_ring_allgather,
    );
    let stats = handle.stats();
    let total_flows = ranks * stats.total_steps;

    let list = records.lock().expect("records lock");
    assert_eq!(list.len(), total_flows);

    let mut ids: Vec<u64> = list.iter().map(|rec| rec.flow_id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total_flows);

    let expected_last = start_flow_id.saturating_add(total_flows.saturating_sub(1) as u64);
    assert_eq!(ids.first().copied(), Some(start_flow_id));
    assert_eq!(ids.last().copied(), Some(expected_last));
    for (off, id) in ids.iter().enumerate() {
        assert_eq!(*id, start_flow_id.saturating_add(off as u64));
    }
}

#[test]
fn ring_step_start_times_advance_by_transport_delay() {
    let ranks = 4;
    let delay = SimTime::from_micros(2);
    let start_at = SimTime::from_micros(5);
    let (handle, records, _final_time) = run_collective_at(
        ranks,
        100,
        delay,
        1,
        start_at,
        None,
        ring::start_ring_allreduce_at,
    );
    let stats = handle.stats();

    let list = records.lock().expect("records lock");
    let mut by_step: BTreeMap<SimTime, usize> = BTreeMap::new();
    for rec in list.iter() {
        *by_step.entry(rec.start_at).or_default() += 1;
    }
    assert_eq!(by_step.len(), stats.total_steps);
    assert!(by_step.values().all(|n| *n == ranks));

    for (step, step_at) in by_step.keys().copied().enumerate() {
        let expected_at = time_add(start_at, time_mul(delay, step as u64));
        assert_eq!(step_at, expected_at);
    }
}

#[test]
fn ring_flow_fcts_match_constant_transport_delay() {
    let ranks = 4;
    let delay = SimTime::from_micros(9);
    let (handle, _records, _final_time) =
        run_collective(ranks, 77, delay, ring::start_ring_reducescatter);
    let stats = handle.stats();
    assert_eq!(stats.flow_fct_ns.len(), ranks * stats.total_steps);
    assert!(stats.flow_fct_ns.iter().all(|fct| *fct == delay.0));
}

#[test]
fn ring_allreduce_reports_expected_reduce_and_done_times_and_calls_done_cb_once() {
    let ranks = 4;
    let delay = SimTime::from_micros(4);
    let start_at = SimTime::from_micros(8);
    let called = Arc::new(AtomicUsize::new(0));
    let called_at = Arc::new(Mutex::new(Vec::new()));

    let called2 = Arc::clone(&called);
    let called_at2 = Arc::clone(&called_at);
    let done_cb: Option<ring::RingAllreduceDoneCallback> =
        Some(Box::new(move |now: SimTime, _sim: &mut Simulator| {
            called2.fetch_add(1, Ordering::SeqCst);
            called_at2.lock().expect("called_at lock").push(now);
        }));

    let (handle, _records, final_time) = run_collective_at(
        ranks,
        1000,
        delay,
        256,
        start_at,
        done_cb,
        ring::start_ring_allreduce_at,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, (ranks - 1) * 2);
    assert_eq!(stats.start_at, Some(start_at));

    let expected_reduce_done_at = time_add(start_at, time_mul(delay, (ranks - 1) as u64));
    let expected_done_at = time_add(start_at, time_mul(delay, stats.total_steps as u64));
    assert_eq!(stats.reduce_done_at, Some(expected_reduce_done_at));
    assert_eq!(stats.done_at, Some(expected_done_at));
    assert_eq!(final_time, expected_done_at);

    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(
        &*called_at.lock().expect("called_at lock"),
        &[expected_done_at]
    );
}

#[test]
fn ring_done_cb_runs_for_zero_step_collectives() {
    let called = Arc::new(AtomicUsize::new(0));
    let called_at = Arc::new(Mutex::new(Vec::new()));
    let called2 = Arc::clone(&called);
    let called_at2 = Arc::clone(&called_at);
    let done_cb: Option<ring::RingAllreduceDoneCallback> =
        Some(Box::new(move |now: SimTime, _sim: &mut Simulator| {
            called2.fetch_add(1, Ordering::SeqCst);
            called_at2.lock().expect("called_at lock").push(now);
        }));

    let (handle, records, final_time) = run_collective_at(
        1,
        123,
        SimTime::from_micros(1),
        1,
        SimTime::from_micros(9),
        done_cb,
        ring::start_ring_alltoall_at,
    );
    let stats = handle.stats();
    assert_eq!(stats.total_steps, 0);
    assert_eq!(records.lock().expect("records lock").len(), 0);
    assert_eq!(final_time, SimTime::from_micros(9));

    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(
        &*called_at.lock().expect("called_at lock"),
        &[SimTime::from_micros(9)]
    );
}

#[test]
fn ring_alltoall_waits_for_slowest_flow_each_step_with_variable_delays() {
    let ranks = 4;
    let start_flow_id = 10_000;
    let chunk_bytes = 7;
    let start_at = SimTime::from_micros(100);
    let called = Arc::new(AtomicUsize::new(0));
    let called_at = Arc::new(Mutex::new(Vec::new()));

    let called2 = Arc::clone(&called);
    let called_at2 = Arc::clone(&called_at);
    let done_cb: Option<ring::RingAllreduceDoneCallback> =
        Some(Box::new(move |now: SimTime, _sim: &mut Simulator| {
            called2.fetch_add(1, Ordering::SeqCst);
            called_at2.lock().expect("called_at lock").push(now);
        }));

    let records = Arc::new(Mutex::new(Vec::new()));
    let transport = VariableDelayTransport {
        ranks,
        start_flow_id,
        records: Arc::clone(&records),
    };

    let cfg = RingAllreduceConfig {
        ranks,
        hosts: (0..ranks).map(NodeId).collect(),
        chunk_bytes,
        routing: RoutingMode::PerPacket,
        start_flow_id,
        transport: Box::new(transport),
        done_cb,
    };

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();
    let handle = ring::start_ring_alltoall_at(&mut sim, cfg, start_at);
    sim.run(&mut world);

    let stats = handle.stats();
    assert_eq!(stats.start_at, Some(start_at));
    assert_eq!(stats.total_steps, ranks - 1);

    // Delay model in VariableDelayTransport:
    // flow delay = (step + rank + 1) us
    // -> step duration = max_rank delay = (step + ranks) us
    let mut expected_done_at = start_at;
    for step in 0..stats.total_steps {
        expected_done_at = time_add(
            expected_done_at,
            SimTime::from_micros((step as u64).saturating_add(ranks as u64)),
        );
    }
    assert_eq!(stats.done_at, Some(expected_done_at));
    assert_eq!(sim.now(), expected_done_at);

    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(
        &*called_at.lock().expect("called_at lock"),
        &[expected_done_at]
    );

    let list = records.lock().expect("records lock");
    assert_eq!(list.len(), ranks * stats.total_steps);
    assert!(list.iter().all(|rec| rec.chunk_bytes == chunk_bytes));
    assert!(list.iter().all(|rec| rec.routing == RoutingMode::PerPacket));

    // Steps start only after the slowest flow in the previous step finishes.
    let mut by_step: BTreeMap<SimTime, Vec<FlowStart>> = BTreeMap::new();
    for rec in list.iter().copied() {
        by_step.entry(rec.start_at).or_default().push(rec);
    }
    assert_eq!(by_step.len(), stats.total_steps);

    let mut expected_step_at = start_at;
    let mut prev_max_done_at: Option<SimTime> = None;
    for (step_idx, (step_at, flows)) in by_step.iter().enumerate() {
        assert_eq!(*step_at, expected_step_at);
        assert_eq!(flows.len(), ranks);

        if let Some(prev) = prev_max_done_at {
            assert_eq!(*step_at, prev);
        }

        let mut max_done_at = SimTime::ZERO;
        for rec in flows.iter() {
            let expected_delay = SimTime::from_micros(step_idx as u64 + rec.src.0 as u64 + 1);
            let expected_flow_done_at = time_add(*step_at, expected_delay);
            assert_eq!(rec.done_at, expected_flow_done_at);
            max_done_at = max_done_at.max(rec.done_at);
        }

        prev_max_done_at = Some(max_done_at);
        expected_step_at = max_done_at;
    }
}

#[test]
fn ring_collectives_constant_delay_have_expected_flow_counts_and_duration() {
    let delay = SimTime::from_micros(2);

    for ranks in 0..=6 {
        // allreduce
        {
            let (handle, records, final_time) =
                run_collective(ranks, 1, delay, ring::start_ring_allreduce);
            let stats = handle.stats();
            let expected_steps = ranks.saturating_sub(1).saturating_mul(2);
            assert_eq!(stats.total_steps, expected_steps);
            assert_eq!(
                records.lock().expect("records lock").len(),
                ranks * expected_steps
            );
            assert_eq!(stats.flow_fct_ns.len(), ranks * expected_steps);
            assert!(stats.flow_fct_ns.iter().all(|v| *v == delay.0));

            let expected_done = time_mul(delay, expected_steps as u64);
            assert_eq!(stats.start_at, Some(SimTime::ZERO));
            assert_eq!(stats.done_at, Some(expected_done));
            assert_eq!(final_time, expected_done);
            if ranks >= 2 {
                assert!(stats.reduce_done_at.is_some());
                assert!(stats.reduce_done_at.unwrap() < stats.done_at.unwrap());
            } else {
                assert!(stats.reduce_done_at.is_none());
            }
        }

        // allgather
        {
            let (handle, records, final_time) =
                run_collective(ranks, 1, delay, ring::start_ring_allgather);
            let stats = handle.stats();
            let expected_steps = ranks.saturating_sub(1);
            assert_eq!(stats.total_steps, expected_steps);
            assert_eq!(
                records.lock().expect("records lock").len(),
                ranks * expected_steps
            );
            assert_eq!(stats.flow_fct_ns.len(), ranks * expected_steps);
            assert!(stats.flow_fct_ns.iter().all(|v| *v == delay.0));

            let expected_done = time_mul(delay, expected_steps as u64);
            assert_eq!(stats.start_at, Some(SimTime::ZERO));
            assert_eq!(stats.done_at, Some(expected_done));
            assert_eq!(final_time, expected_done);
            assert!(stats.reduce_done_at.is_none());
        }

        // reduce-scatter
        {
            let (handle, records, final_time) =
                run_collective(ranks, 1, delay, ring::start_ring_reducescatter);
            let stats = handle.stats();
            let expected_steps = ranks.saturating_sub(1);
            assert_eq!(stats.total_steps, expected_steps);
            assert_eq!(
                records.lock().expect("records lock").len(),
                ranks * expected_steps
            );
            assert_eq!(stats.flow_fct_ns.len(), ranks * expected_steps);
            assert!(stats.flow_fct_ns.iter().all(|v| *v == delay.0));

            let expected_done = time_mul(delay, expected_steps as u64);
            assert_eq!(stats.start_at, Some(SimTime::ZERO));
            assert_eq!(stats.done_at, Some(expected_done));
            assert_eq!(final_time, expected_done);
            if ranks >= 2 {
                assert_eq!(stats.reduce_done_at, stats.done_at);
            } else {
                assert!(stats.reduce_done_at.is_none());
            }
        }

        // all-to-all
        {
            let (handle, records, final_time) =
                run_collective(ranks, 1, delay, ring::start_ring_alltoall);
            let stats = handle.stats();
            let expected_steps = ranks.saturating_sub(1);
            assert_eq!(stats.total_steps, expected_steps);
            assert_eq!(
                records.lock().expect("records lock").len(),
                ranks * expected_steps
            );
            assert_eq!(stats.flow_fct_ns.len(), ranks * expected_steps);
            assert!(stats.flow_fct_ns.iter().all(|v| *v == delay.0));

            let expected_done = time_mul(delay, expected_steps as u64);
            assert_eq!(stats.start_at, Some(SimTime::ZERO));
            assert_eq!(stats.done_at, Some(expected_done));
            assert_eq!(final_time, expected_done);
            assert!(stats.reduce_done_at.is_none());
        }
    }
}

#[test]
fn ring_allgather_and_reducescatter_use_neighbor_routing() {
    let ranks = 5;

    for start in [ring::start_ring_allgather, ring::start_ring_reducescatter] {
        let (_handle, records, _final_time) =
            run_collective(ranks, 1, SimTime::from_micros(1), start);
        let list = records.lock().expect("records lock");
        assert!(list.iter().all(|rec| rec.dst.0 == (rec.src.0 + 1) % ranks));
    }
}

#[test]
fn ring_alltoall_covers_all_pairs_for_multiple_sizes() {
    for ranks in 2..=7 {
        let (_handle, records, _final_time) =
            run_collective(ranks, 1, SimTime::from_micros(1), ring::start_ring_alltoall);
        let list = records.lock().expect("records lock");
        assert_eq!(list.len(), ranks * (ranks - 1));

        let mut seen = HashSet::new();
        for rec in list.iter() {
            assert_ne!(rec.src.0, rec.dst.0);
            assert!(seen.insert((rec.src.0, rec.dst.0)));
        }
        assert_eq!(seen.len(), ranks * (ranks - 1));
    }
}
