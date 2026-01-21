//! Ring-based collective communication algorithms.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::net::{NetWorld, NodeId};
use crate::sim::{Event, SimTime, Simulator, World};

/// Routing policy used by ring collectives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMode {
    PerFlow,
    PerPacket,
}

/// Callback invoked when a flow finishes.
pub type RingDoneCallback = Box<dyn Fn(SimTime, &mut Simulator) + Send>;

/// Transport adapter used by ring collectives.
pub trait RingTransport: Send + 'static {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: RoutingMode,
        sim: &mut Simulator,
        world: &mut NetWorld,
        done: RingDoneCallback,
    );
}

#[derive(Clone)]
struct State {
    ranks: usize,
    hosts: Vec<NodeId>,
    chunk_bytes: u64,
    routing: RoutingMode,
    step: usize,
    inflight: usize,
    next_flow_id: u64,
    start_at: Option<SimTime>,
    reduce_done_at: Option<SimTime>,
    done_at: Option<SimTime>,
    flow_start_at: HashMap<u64, SimTime>,
    flow_fct_ns: Vec<u64>,
}

impl State {
    fn total_steps(&self) -> usize {
        self.ranks.saturating_sub(1) * 2
    }
}

struct StepContext {
    ranks: usize,
    hosts: Vec<NodeId>,
    chunk_bytes: u64,
    routing: RoutingMode,
    start_flow_id: u64,
}

struct StartStep {
    state: Arc<Mutex<State>>,
    transport: Arc<Mutex<Box<dyn RingTransport>>>,
}

struct FlowDone {
    state: Arc<Mutex<State>>,
    transport: Arc<Mutex<Box<dyn RingTransport>>>,
    flow_id: u64,
    done_at: SimTime,
}

impl Event for StartStep {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let StartStep { state, transport } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let ctx = {
            let mut st = state.lock().expect("ring allreduce state lock");
            let total_steps = st.total_steps();
            if total_steps == 0 {
                if st.start_at.is_none() {
                    st.start_at = Some(sim.now());
                }
                st.done_at = Some(sim.now());
                return;
            }
            if st.step >= total_steps {
                st.done_at = Some(sim.now());
                return;
            }
            if st.start_at.is_none() {
                st.start_at = Some(sim.now());
            }
            st.inflight = st.ranks;
            let start_flow_id = st.next_flow_id;
            st.next_flow_id = st.next_flow_id.saturating_add(st.ranks as u64);
            let step_start = sim.now();
            for rank in 0..st.ranks {
                let flow_id = start_flow_id.saturating_add(rank as u64);
                st.flow_start_at.insert(flow_id, step_start);
            }
            StepContext {
                ranks: st.ranks,
                hosts: st.hosts.clone(),
                chunk_bytes: st.chunk_bytes,
                routing: st.routing,
                start_flow_id,
            }
        };

        let transport_arc = Arc::clone(&transport);
        let mut transport = transport_arc.lock().expect("ring transport lock");

        for rank in 0..ctx.ranks {
            let flow_id = ctx.start_flow_id.saturating_add(rank as u64);
            let src = ctx.hosts[rank];
            let dst = ctx.hosts[(rank + 1) % ctx.ranks];
            let done_state = Arc::clone(&state);
            let done_transport = Arc::clone(&transport_arc);
            let done_cb: RingDoneCallback = Box::new(move |now, sim| {
                sim.schedule(
                    now,
                    FlowDone {
                        state: Arc::clone(&done_state),
                        transport: Arc::clone(&done_transport),
                        flow_id,
                        done_at: now,
                    },
                );
            });
            transport.start_flow(
                flow_id,
                src,
                dst,
                ctx.chunk_bytes,
                ctx.routing,
                sim,
                w,
                done_cb,
            );
        }
    }
}

impl Event for FlowDone {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        let FlowDone {
            state,
            transport,
            flow_id,
            done_at,
        } = *self;
        let mut start_next = false;
        {
            let mut st = state.lock().expect("ring allreduce state lock");
            if st.inflight == 0 || st.done_at.is_some() {
                return;
            }
            if let Some(start_at) = st.flow_start_at.remove(&flow_id) {
                let fct_ns = done_at.0.saturating_sub(start_at.0);
                st.flow_fct_ns.push(fct_ns);
            }
            st.inflight = st.inflight.saturating_sub(1);
            if st.inflight == 0 {
                if st.step + 1 == st.ranks.saturating_sub(1) {
                    st.reduce_done_at = Some(sim.now());
                }
                st.step = st.step.saturating_add(1);
                if st.step >= st.total_steps() {
                    st.done_at = Some(sim.now());
                } else {
                    start_next = true;
                }
            }
        }

        if start_next {
            sim.schedule(
                sim.now(),
                StartStep {
                    state,
                    transport,
                },
            );
        }
    }
}

/// Configuration for a ring allreduce.
pub struct RingAllreduceConfig {
    pub ranks: usize,
    pub hosts: Vec<NodeId>,
    pub chunk_bytes: u64,
    pub routing: RoutingMode,
    pub start_flow_id: u64,
    pub transport: Box<dyn RingTransport>,
}

/// Runtime stats collected by a ring allreduce.
#[derive(Debug, Clone)]
pub struct RingAllreduceStats {
    pub start_at: Option<SimTime>,
    pub reduce_done_at: Option<SimTime>,
    pub done_at: Option<SimTime>,
    pub total_steps: usize,
    pub flow_fct_ns: Vec<u64>,
}

/// Handle for inspecting ring allreduce progress/results.
pub struct RingAllreduceHandle {
    state: Arc<Mutex<State>>,
}

impl RingAllreduceHandle {
    pub fn stats(&self) -> RingAllreduceStats {
        let st = self.state.lock().expect("ring allreduce state lock");
        RingAllreduceStats {
            start_at: st.start_at,
            reduce_done_at: st.reduce_done_at,
            done_at: st.done_at,
            total_steps: st.total_steps(),
            flow_fct_ns: st.flow_fct_ns.clone(),
        }
    }
}

/// Schedule a ring allreduce at SimTime::ZERO and return a handle for stats.
pub fn start_ring_allreduce(
    sim: &mut Simulator,
    cfg: RingAllreduceConfig,
) -> RingAllreduceHandle {
    let state = Arc::new(Mutex::new(State {
        ranks: cfg.ranks,
        hosts: cfg.hosts,
        chunk_bytes: cfg.chunk_bytes,
        routing: cfg.routing,
        step: 0,
        inflight: 0,
        next_flow_id: cfg.start_flow_id,
        start_at: None,
        reduce_done_at: None,
        done_at: None,
        flow_start_at: HashMap::new(),
        flow_fct_ns: Vec::new(),
    }));

    let transport = Arc::new(Mutex::new(cfg.transport));

    sim.schedule(
        SimTime::ZERO,
        StartStep {
            state: Arc::clone(&state),
            transport: Arc::clone(&transport),
        },
    );

    RingAllreduceHandle { state }
}
