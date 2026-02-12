use clap::Parser;
use htsim_rs::cc::collective::CollectiveOp;
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpDoneCallback};
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpDoneCallback};
use htsim_rs::queue::DEFAULT_PKT_BYTES;
use htsim_rs::sim::{
    HostSpec, RankStepKind, RankStepSpec, RoutingMode, SendRecvDirection, SimTime, Simulator,
    StepSpec, TopologySpec, TransportProtocol, WorkloadDefaults, WorkloadSpec,
};
use htsim_rs::topo::dumbbell::{DumbbellOpts, build_dumbbell};
use htsim_rs::topo::fat_tree::{FatTreeOpts, build_fat_tree};
use htsim_rs::viz::{VizEvent, VizEventKind, VizLogger};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const DEFAULT_HOST_EGRESS_QUEUE_BYTES: u64 = 16_u64 * 1024 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "workload-sim",
    about = "Run workload.json on htsim-rs network simulator"
)]
struct Args {
    /// Path to workload.json
    #[arg(long)]
    workload: PathBuf,

    /// Output viz JSON file (for viz/index.html)
    #[arg(long)]
    viz_json: Option<PathBuf>,

    /// Run until this time (ms); defaults to running until completion
    #[arg(long)]
    until_ms: Option<u64>,

    /// Override protocol: tcp or dctcp
    #[arg(long)]
    protocol: Option<String>,

    /// Override routing: per_flow or per_packet
    #[arg(long)]
    routing: Option<String>,

    /// Print per-collective flow completion time (FCT) stats
    #[arg(long)]
    fct_stats: bool,

    /// Override switch egress queue capacity in bytes
    #[arg(long)]
    queue_bytes: Option<u64>,

    /// Override switch egress queue capacity in packets (1500B each)
    #[arg(long)]
    queue_pkts: Option<u64>,

    /// Override host egress queue capacity in bytes (defaults to a large value)
    #[arg(long)]
    host_queue_bytes: Option<u64>,

    /// Override host egress queue capacity in packets (1500B each)
    #[arg(long)]
    host_queue_pkts: Option<u64>,
}

struct CollectiveRecord {
    step_id: Option<u64>,
    label: Option<String>,
    comm_id: Option<String>,
    op: Option<String>,
    comm_bytes: u64,
    hosts: usize,
    handle: ring::RingAllreduceHandle,
}

struct WorkloadState {
    steps: Vec<StepSpec>,
    hosts_all: Vec<usize>,
    host_map: HashMap<usize, NodeId>,
    gpu_map: HashMap<usize, Option<String>>,
    protocol: TransportProtocol,
    routing: CcRoutingMode,
    next_flow_id: u64,
    tcp_cfg: TcpConfig,
    dctcp_cfg: DctcpConfig,
    collective_handles: Arc<Mutex<Vec<CollectiveRecord>>>,
}

struct StartWorkloadStep {
    idx: usize,
    state: Arc<Mutex<WorkloadState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsyncWaitKind {
    None,
    All,
    Stream(u64),
}

struct RankState {
    steps: Vec<RankStepSpec>,
    idx: usize,
    pending_async_total: usize,
    pending_async_by_stream: HashMap<u64, usize>,
    waiting_for_async: AsyncWaitKind,
}

struct CollectiveWait {
    hosts: Vec<usize>,
    comm_bytes: u64,
    op: String,
    is_async: bool,
    comm_stream: u64,
    arrived: Vec<usize>,
}

struct SendRecvWait {
    comm_bytes: u64,
    sender: Option<usize>,
    receiver: Option<usize>,
    arrived: Vec<usize>,
}

struct RankWorkloadState {
    ranks: HashMap<usize, RankState>,
    hosts_all: Vec<usize>,
    host_map: HashMap<usize, NodeId>,
    gpu_map: HashMap<usize, Option<String>>,
    protocol: TransportProtocol,
    routing: CcRoutingMode,
    next_flow_id: u64,
    tcp_cfg: TcpConfig,
    dctcp_cfg: DctcpConfig,
    pending_collectives: HashMap<String, CollectiveWait>,
    pending_sendrecv: HashMap<String, SendRecvWait>,
    collective_handles: Arc<Mutex<Vec<CollectiveRecord>>>,
}

struct StartRankStep {
    rank_id: usize,
    state: Arc<Mutex<RankWorkloadState>>,
}

struct TcpRingTransport {
    cfg: TcpConfig,
}

impl RingTransport for TcpRingTransport {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: CcRoutingMode,
        sim: &mut Simulator,
        world: &mut NetWorld,
        done: ring::RingDoneCallback,
    ) {
        let mut tcp = std::mem::take(&mut world.net.tcp);
        let conn = match routing {
            CcRoutingMode::PerFlow => {
                let route = world.net.route_ecmp_path(src, dst, flow_id);
                TcpConn::new(flow_id, src, dst, route, chunk_bytes, self.cfg.clone())
            }
            CcRoutingMode::PerPacket => {
                TcpConn::new_dynamic(flow_id, src, dst, chunk_bytes, self.cfg.clone())
            }
        };
        let done_cb: TcpDoneCallback = Box::new(move |_, now, sim| {
            done(now, sim);
        });
        tcp.set_done_callback(flow_id, done_cb);
        tcp.start_conn(conn, sim, &mut world.net);
        world.net.tcp = tcp;
    }
}

struct DctcpRingTransport {
    cfg: DctcpConfig,
}

impl RingTransport for DctcpRingTransport {
    fn start_flow(
        &mut self,
        flow_id: u64,
        src: NodeId,
        dst: NodeId,
        chunk_bytes: u64,
        routing: CcRoutingMode,
        sim: &mut Simulator,
        world: &mut NetWorld,
        done: ring::RingDoneCallback,
    ) {
        let mut dctcp = std::mem::take(&mut world.net.dctcp);
        let conn = match routing {
            CcRoutingMode::PerFlow => {
                let route = world.net.route_ecmp_path(src, dst, flow_id);
                DctcpConn::new(flow_id, src, dst, route, chunk_bytes, self.cfg.clone())
            }
            CcRoutingMode::PerPacket => {
                DctcpConn::new_dynamic(flow_id, src, dst, chunk_bytes, self.cfg.clone())
            }
        };
        let done_cb: DctcpDoneCallback = Box::new(move |_, now, sim| {
            done(now, sim);
        });
        dctcp.set_done_callback(flow_id, done_cb);
        dctcp.start_conn(conn, sim, &mut world.net);
        world.net.dctcp = dctcp;
    }
}

fn compute_duration_ns_from_ms(ms: f64) -> u64 {
    if !ms.is_finite() || ms <= 0.0 {
        return 0;
    }
    (ms * 1_000_000.0).round() as u64
}

fn default_tcp_cfg() -> TcpConfig {
    // Keep RTOs reasonably small to avoid huge FCT inflation after drops, but
    // avoid sub-ms floors that can trigger spurious timeouts due to ACK/data
    // sharing on host egress queues.
    //
    // Also use a large initial ssthresh so bulk transfers can stay in slow-start
    // long enough to reach line-rate on high-BW, low-RTT topologies (e.g., 100Gbps
    // with us-scale RTTs). The smaller default (1000*MSS) can underutilize links
    // for large collectives.
    let mut cfg = TcpConfig::default();
    cfg.init_rto = SimTime::from_millis(1);
    cfg.min_rto = SimTime::from_millis(1);
    cfg.max_rto = SimTime::from_millis(200);
    cfg.init_ssthresh_bytes = (cfg.mss as u64).saturating_mul(1_000_000);
    cfg
}

fn percentile_ns(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let p = if p <= 0.0 {
        0.0
    } else if p >= 1.0 {
        1.0
    } else {
        p
    };
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = (p * sorted.len() as f64).ceil() as usize;
    let idx = idx.saturating_sub(1).min(sorted.len().saturating_sub(1));
    sorted.get(idx).copied()
}

fn start_p2p_flow(
    sim: &mut Simulator,
    world: &mut NetWorld,
    protocol: TransportProtocol,
    routing: CcRoutingMode,
    tcp_cfg: &TcpConfig,
    dctcp_cfg: &DctcpConfig,
    flow_id: u64,
    src: NodeId,
    dst: NodeId,
    bytes: u64,
    done: ring::RingDoneCallback,
) {
    match protocol {
        TransportProtocol::Tcp => {
            let mut tcp = std::mem::take(&mut world.net.tcp);
            let conn = match routing {
                CcRoutingMode::PerFlow => {
                    let route = world.net.route_ecmp_path(src, dst, flow_id);
                    TcpConn::new(flow_id, src, dst, route, bytes, tcp_cfg.clone())
                }
                CcRoutingMode::PerPacket => {
                    TcpConn::new_dynamic(flow_id, src, dst, bytes, tcp_cfg.clone())
                }
            };
            let done_cb: TcpDoneCallback = Box::new(move |_, now, sim| {
                done(now, sim);
            });
            tcp.set_done_callback(flow_id, done_cb);
            tcp.start_conn(conn, sim, &mut world.net);
            world.net.tcp = tcp;
        }
        TransportProtocol::Dctcp => {
            let mut dctcp = std::mem::take(&mut world.net.dctcp);
            let conn = match routing {
                CcRoutingMode::PerFlow => {
                    let route = world.net.route_ecmp_path(src, dst, flow_id);
                    DctcpConn::new(flow_id, src, dst, route, bytes, dctcp_cfg.clone())
                }
                CcRoutingMode::PerPacket => {
                    DctcpConn::new_dynamic(flow_id, src, dst, bytes, dctcp_cfg.clone())
                }
            };
            let done_cb: DctcpDoneCallback = Box::new(move |_, now, sim| {
                done(now, sim);
            });
            dctcp.set_done_callback(flow_id, done_cb);
            dctcp.start_conn(conn, sim, &mut world.net);
            world.net.dctcp = dctcp;
        }
    }
}

impl StartWorkloadStep {
    fn compute_duration_ns(step: &StepSpec) -> u64 {
        let ms = step.compute_ms.unwrap_or(0.0);
        compute_duration_ns_from_ms(ms)
    }
}

fn rank_step_kind(step: &RankStepSpec) -> RankStepKind {
    if let Some(kind) = &step.kind {
        return kind.clone();
    }
    if step.peer.is_some() {
        return RankStepKind::Sendrecv;
    }
    if step.comm_bytes.is_some() || step.hosts.is_some() || step.op.is_some() {
        return RankStepKind::Collective;
    }
    RankStepKind::Compute
}

fn collective_is_async(op: &str) -> bool {
    let normalized = op.trim().to_lowercase();
    let compact: String = normalized
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-')
        .collect();
    compact.ends_with("async")
}

fn comm_stream_id(comm_id: &str) -> u64 {
    // Stable 64-bit FNV-1a hash (do not use `DefaultHasher`, which is randomized).
    let mut hash: u64 = 14695981039346656037;
    for b in comm_id.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn pending_async_on_stream(rank_state: &RankState, stream: u64) -> usize {
    rank_state
        .pending_async_by_stream
        .get(&stream)
        .copied()
        .unwrap_or(0)
}

fn async_wait_kind_for_step(
    step: &RankStepSpec,
    kind: &RankStepKind,
    rank_state: &RankState,
) -> AsyncWaitKind {
    match kind {
        RankStepKind::Compute => AsyncWaitKind::None,
        RankStepKind::CollectiveWait => {
            if let Some(stream) = step.comm_stream {
                let stream = u64::from(stream);
                if pending_async_on_stream(rank_state, stream) > 0 {
                    AsyncWaitKind::Stream(stream)
                } else {
                    AsyncWaitKind::None
                }
            } else if rank_state.pending_async_total > 0 {
                AsyncWaitKind::All
            } else {
                AsyncWaitKind::None
            }
        }
        RankStepKind::Collective | RankStepKind::Sendrecv => {
            let Some(comm_id) = step.comm_id.as_deref() else {
                return AsyncWaitKind::None;
            };
            let stream = step
                .comm_stream
                .map(u64::from)
                .unwrap_or_else(|| comm_stream_id(comm_id));
            if pending_async_on_stream(rank_state, stream) > 0 {
                AsyncWaitKind::Stream(stream)
            } else {
                AsyncWaitKind::None
            }
        }
    }
}

impl htsim_rs::sim::Event for StartWorkloadStep {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn htsim_rs::sim::World) {
        let StartWorkloadStep { idx, state } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let (step, hosts, protocol, routing, next_flow_id, gpu_map, tcp_cfg, dctcp_cfg) = {
            let st = state.lock().expect("workload state lock");
            if idx >= st.steps.len() {
                return;
            }
            let step = st.steps[idx].clone();
            let hosts = step.hosts.clone().unwrap_or_else(|| st.hosts_all.clone());
            let protocol = step.protocol.unwrap_or(st.protocol);
            let routing = st.routing;
            let next_flow_id = st.next_flow_id;
            let gpu_map = st.gpu_map.clone();
            let tcp_cfg = st.tcp_cfg.clone();
            let dctcp_cfg = st.dctcp_cfg.clone();
            (
                step,
                hosts,
                protocol,
                routing,
                next_flow_id,
                gpu_map,
                tcp_cfg,
                dctcp_cfg,
            )
        };

        let duration_ns = Self::compute_duration_ns(&step);
        let step_id = step.id;
        let label = step.label.clone();

        let host_nodes = {
            let st = state.lock().expect("workload state lock");
            hosts
                .iter()
                .map(|hid| *st.host_map.get(hid).expect("unknown host id"))
                .collect::<Vec<_>>()
        };

        if duration_ns > 0 {
            if let Some(v) = &mut w.net.viz {
                for (idx, hid) in hosts.iter().enumerate() {
                    let node = host_nodes[idx];
                    let gpu = gpu_map.get(hid).and_then(|g| g.clone());
                    v.push(VizEvent {
                        t_ns: sim.now().0,
                        pkt_id: None,
                        flow_id: None,
                        pkt_bytes: None,
                        pkt_kind: None,
                        kind: VizEventKind::GpuBusy {
                            node: node.0,
                            duration_ns,
                            gpu,
                            step_id,
                            label: label.clone(),
                        },
                    });
                }
            }
        }

        let next_at = SimTime(sim.now().0.saturating_add(duration_ns));
        let comm_bytes = step.comm_bytes.unwrap_or(0);
        if comm_bytes == 0 || host_nodes.len() <= 1 {
            sim.schedule(
                next_at,
                StartWorkloadStep {
                    idx: idx.saturating_add(1),
                    state,
                },
            );
            return;
        }

        let ranks = host_nodes.len() as u64;
        let chunk_bytes = (comm_bytes + ranks - 1) / ranks;
        let total_steps = host_nodes.len().saturating_sub(1) * 2;
        let flow_span = (host_nodes.len() as u64)
            .saturating_mul(total_steps as u64)
            .max(1);

        let done_state = Arc::clone(&state);
        let next_idx = idx.saturating_add(1);
        let done_cb: ring::RingAllreduceDoneCallback = Box::new(move |now, sim| {
            sim.schedule(
                now,
                StartWorkloadStep {
                    idx: next_idx,
                    state: Arc::clone(&done_state),
                },
            );
        });

        let transport: Box<dyn RingTransport> = match protocol {
            TransportProtocol::Tcp => Box::new(TcpRingTransport { cfg: tcp_cfg }),
            TransportProtocol::Dctcp => Box::new(DctcpRingTransport { cfg: dctcp_cfg }),
        };

        let handles = {
            let st = state.lock().expect("workload state lock");
            Arc::clone(&st.collective_handles)
        };

        {
            let mut st = state.lock().expect("workload state lock");
            st.next_flow_id = st.next_flow_id.saturating_add(flow_span);
        }

        let handle = ring::start_ring_allreduce_at(
            sim,
            RingAllreduceConfig {
                ranks: host_nodes.len(),
                hosts: host_nodes,
                chunk_bytes,
                routing,
                start_flow_id: next_flow_id,
                transport,
                done_cb: Some(done_cb),
            },
            next_at,
        );
        let record = CollectiveRecord {
            step_id: step.id,
            label: step.label.clone(),
            comm_id: None,
            op: Some("allreduce".to_string()),
            comm_bytes,
            hosts: hosts.len(),
            handle,
        };
        if let Ok(mut list) = handles.lock() {
            list.push(record);
        }
    }
}

impl htsim_rs::sim::Event for StartRankStep {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn htsim_rs::sim::World) {
        let StartRankStep { rank_id, state } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let (step, kind, wait_kind, host_node, gpu, protocol, routing, tcp_cfg, dctcp_cfg, hosts_all) = {
            let mut st = state.lock().expect("rank workload state lock");
            let rank_state = match st.ranks.get_mut(&rank_id) {
                Some(entry) => entry,
                None => return,
            };
            if rank_state.idx >= rank_state.steps.len() {
                if rank_state.pending_async_total > 0 {
                    rank_state.waiting_for_async = AsyncWaitKind::All;
                }
                return;
            }
            let step = rank_state.steps[rank_state.idx].clone();
            let kind = rank_step_kind(&step);
            let wait_kind = async_wait_kind_for_step(&step, &kind, rank_state);
            let host_node = *st.host_map.get(&rank_id).expect("unknown host id");
            let gpu = st.gpu_map.get(&rank_id).and_then(|g| g.clone());
            (
                step,
                kind,
                wait_kind,
                host_node,
                gpu,
                st.protocol,
                st.routing,
                st.tcp_cfg.clone(),
                st.dctcp_cfg.clone(),
                st.hosts_all.clone(),
            )
        };

        if wait_kind != AsyncWaitKind::None {
            let mut st = state.lock().expect("rank workload state lock");
            if let Some(rank_state) = st.ranks.get_mut(&rank_id) {
                rank_state.waiting_for_async = wait_kind;
            }
            return;
        }

        {
            let mut st = state.lock().expect("rank workload state lock");
            let Some(rank_state) = st.ranks.get_mut(&rank_id) else {
                return;
            };
            if rank_state.idx >= rank_state.steps.len() {
                return;
            }
            rank_state.idx = rank_state.idx.saturating_add(1);
        }

        match kind {
            RankStepKind::Compute => {
                let duration_ns = compute_duration_ns_from_ms(step.compute_ms.unwrap_or(0.0));
                if duration_ns > 0 {
                    if let Some(v) = &mut w.net.viz {
                        v.push(VizEvent {
                            t_ns: sim.now().0,
                            pkt_id: None,
                            flow_id: None,
                            pkt_bytes: None,
                            pkt_kind: None,
                            kind: VizEventKind::GpuBusy {
                                node: host_node.0,
                                duration_ns,
                                gpu,
                                step_id: step.id,
                                label: step.label.clone(),
                            },
                        });
                    }
                }
                let next_at = SimTime(sim.now().0.saturating_add(duration_ns));
                sim.schedule(
                    next_at,
                    StartRankStep {
                        rank_id,
                        state: Arc::clone(&state),
                    },
                );
            }
            RankStepKind::CollectiveWait => {
                // If there are outstanding async collectives, we would have
                // returned earlier with `waiting_for_async` set.
                sim.schedule(
                    sim.now(),
                    StartRankStep {
                        rank_id,
                        state: Arc::clone(&state),
                    },
                );
            }
            RankStepKind::Collective => {
                let comm_id = match step.comm_id.clone() {
                    Some(id) => id,
                    None => {
                        sim.schedule(
                            sim.now(),
                            StartRankStep {
                                rank_id,
                                state: Arc::clone(&state),
                            },
                        );
                        return;
                    }
                };
                let comm_bytes = step.comm_bytes.unwrap_or(0);
                let op = step
                    .op
                    .clone()
                    .unwrap_or_else(|| "allreduce".to_string())
                    .trim()
                    .to_lowercase();
                let hosts = step.hosts.clone().unwrap_or_else(|| hosts_all.clone());
                let comm_stream = step
                    .comm_stream
                    .map(u64::from)
                    .unwrap_or_else(|| comm_stream_id(&comm_id));
                let is_async = collective_is_async(&op);

                if !hosts.contains(&rank_id) {
                    panic!(
                        "rank {} not included in collective hosts for comm_id {:?}: hosts={:?}",
                        rank_id, comm_id, hosts
                    );
                }

                // Non-blocking collective launch: allow this rank to continue immediately.
                if is_async {
                    if comm_bytes > 0 && hosts.len() > 1 {
                        let mut st = state.lock().expect("rank workload state lock");
                        let rank_state = st.ranks.get_mut(&rank_id).expect("missing rank state");
                        rank_state.pending_async_total =
                            rank_state.pending_async_total.saturating_add(1);
                        let counter = rank_state
                            .pending_async_by_stream
                            .entry(comm_stream)
                            .or_insert(0);
                        *counter = counter.saturating_add(1);
                    }
                    sim.schedule(
                        sim.now(),
                        StartRankStep {
                            rank_id,
                            state: Arc::clone(&state),
                        },
                    );
                }

                let mut start_cfg = None;
                {
                    let mut st = state.lock().expect("rank workload state lock");
                    let entry = st
                        .pending_collectives
                        .entry(comm_id.clone())
                        .or_insert_with(|| CollectiveWait {
                            hosts: hosts.clone(),
                            comm_bytes,
                            op: op.clone(),
                            is_async,
                            comm_stream,
                            arrived: Vec::new(),
                        });
                    if entry.op != op || entry.is_async != is_async {
                        panic!(
                            "comm_id {:?} collective op mismatch: existing op={:?} async={} vs new op={:?} async={}",
                            comm_id, entry.op, entry.is_async, op, is_async
                        );
                    }
                    if entry.comm_bytes != comm_bytes {
                        panic!(
                            "comm_id {:?} collective comm_bytes mismatch: existing bytes={} vs new bytes={}",
                            comm_id, entry.comm_bytes, comm_bytes
                        );
                    }
                    if entry.hosts != hosts {
                        panic!(
                            "comm_id {:?} collective hosts mismatch: existing hosts={:?} vs new hosts={:?}",
                            comm_id, entry.hosts, hosts
                        );
                    }
                    if entry.comm_stream != comm_stream {
                        panic!(
                            "comm_id {:?} collective comm_stream mismatch: existing stream={} vs new stream={}",
                            comm_id, entry.comm_stream, comm_stream
                        );
                    }
                    if !entry.arrived.contains(&rank_id) {
                        entry.arrived.push(rank_id);
                    }
                    if entry.arrived.len() == entry.hosts.len() {
                        let entry = st
                            .pending_collectives
                            .remove(&comm_id)
                            .expect("pending collective missing");
                        if entry.comm_bytes == 0 || entry.hosts.len() <= 1 {
                            start_cfg = Some((
                                None,
                                entry.hosts,
                                entry.comm_bytes,
                                Some(comm_id.clone()),
                                Some(entry.op),
                                entry.is_async,
                                entry.comm_stream,
                            ));
                        } else {
                            let host_nodes = entry
                                .hosts
                                .iter()
                                .map(|hid| *st.host_map.get(hid).expect("unknown host id"))
                                .collect::<Vec<_>>();
                            let ranks = host_nodes.len();
                            let algo = CollectiveOp::parse(&entry.op).unwrap_or_else(|err| {
                                panic!(
                                    "invalid collective op {:?} for comm_id {:?}: {err}",
                                    entry.op, comm_id
                                )
                            });
                            let total_steps = algo.total_steps(ranks);
                            let flow_span =
                                (ranks as u64).saturating_mul(total_steps as u64).max(1);
                            let start_flow_id = st.next_flow_id;
                            st.next_flow_id = st.next_flow_id.saturating_add(flow_span);
                            start_cfg = Some((
                                Some((host_nodes, start_flow_id, algo)),
                                entry.hosts,
                                entry.comm_bytes,
                                Some(comm_id.clone()),
                                Some(entry.op),
                                entry.is_async,
                                entry.comm_stream,
                            ));
                        }
                    }
                }

                if let Some((maybe_hosts, hosts, bytes, comm_id, op, is_async, comm_stream)) =
                    start_cfg
                {
                    if bytes == 0 || hosts.len() <= 1 {
                        if !is_async {
                            let done_state = Arc::clone(&state);
                            for hid in hosts {
                                sim.schedule(
                                    sim.now(),
                                    StartRankStep {
                                        rank_id: hid,
                                        state: Arc::clone(&done_state),
                                    },
                                );
                            }
                        }
                        return;
                    }
                    let (host_nodes, start_flow_id, algo) =
                        maybe_hosts.expect("collective config missing");
                    let chunk_bytes = algo.chunk_bytes(bytes, host_nodes.len());
                    let transport: Box<dyn RingTransport> = match protocol {
                        TransportProtocol::Tcp => Box::new(TcpRingTransport { cfg: tcp_cfg }),
                        TransportProtocol::Dctcp => Box::new(DctcpRingTransport { cfg: dctcp_cfg }),
                    };
                    let done_cb: Option<ring::RingAllreduceDoneCallback> = if is_async {
                        let done_state = Arc::clone(&state);
                        let done_hosts = hosts.clone();
                        let done_comm_stream = comm_stream;
                        Some(Box::new(move |now, sim| {
                            let mut wake = Vec::new();
                            {
                                let mut st = done_state.lock().expect("rank workload state lock");
                                for hid in &done_hosts {
                                    let Some(rank_state) = st.ranks.get_mut(hid) else {
                                        continue;
                                    };
                                    rank_state.pending_async_total =
                                        rank_state.pending_async_total.saturating_sub(1);
                                    let mut remove_stream = false;
                                    if let Some(counter) =
                                        rank_state.pending_async_by_stream.get_mut(&done_comm_stream)
                                    {
                                        *counter = counter.saturating_sub(1);
                                        remove_stream = *counter == 0;
                                    }
                                    if remove_stream {
                                        rank_state.pending_async_by_stream.remove(&done_comm_stream);
                                    }
                                    let should_wake = match rank_state.waiting_for_async {
                                        AsyncWaitKind::None => false,
                                        AsyncWaitKind::All => rank_state.pending_async_total == 0,
                                        AsyncWaitKind::Stream(s) => rank_state
                                            .pending_async_by_stream
                                            .get(&s)
                                            .copied()
                                            .unwrap_or(0)
                                            == 0,
                                    };
                                    if should_wake {
                                        rank_state.waiting_for_async = AsyncWaitKind::None;
                                        wake.push(*hid);
                                    }
                                }
                            }
                            for hid in wake {
                                sim.schedule(
                                    now,
                                    StartRankStep {
                                        rank_id: hid,
                                        state: Arc::clone(&done_state),
                                    },
                                );
                            }
                        }))
                    } else {
                        let done_state = Arc::clone(&state);
                        let done_hosts = hosts.clone();
                        Some(Box::new(move |now, sim| {
                            for hid in &done_hosts {
                                sim.schedule(
                                    now,
                                    StartRankStep {
                                        rank_id: *hid,
                                        state: Arc::clone(&done_state),
                                    },
                                );
                            }
                        }))
                    };
                    let handles = {
                        let st = state.lock().expect("rank workload state lock");
                        Arc::clone(&st.collective_handles)
                    };
                    let cfg = RingAllreduceConfig {
                        ranks: host_nodes.len(),
                        hosts: host_nodes,
                        chunk_bytes,
                        routing,
                        start_flow_id,
                        transport,
                        done_cb,
                    };
                    let handle = match algo {
                        CollectiveOp::Allreduce => {
                            ring::start_ring_allreduce_at(sim, cfg, sim.now())
                        }
                        CollectiveOp::Allgather => {
                            ring::start_ring_allgather_at(sim, cfg, sim.now())
                        }
                        CollectiveOp::Reducescatter => {
                            ring::start_ring_reducescatter_at(sim, cfg, sim.now())
                        }
                        CollectiveOp::Alltoall => ring::start_ring_alltoall_at(sim, cfg, sim.now()),
                    };
                    let record = CollectiveRecord {
                        step_id: step.id,
                        label: step.label.clone(),
                        comm_id,
                        op,
                        comm_bytes: bytes,
                        hosts: hosts.len(),
                        handle,
                    };
                    if let Ok(mut list) = handles.lock() {
                        list.push(record);
                    }
                }
            }
            RankStepKind::Sendrecv => {
                let comm_id = match step.comm_id.clone() {
                    Some(id) => id,
                    None => {
                        sim.schedule(
                            sim.now(),
                            StartRankStep {
                                rank_id,
                                state: Arc::clone(&state),
                            },
                        );
                        return;
                    }
                };
                let comm_bytes = step.comm_bytes.unwrap_or(0);
                let direction = step.direction.unwrap_or(SendRecvDirection::Send);
                let peer = step.peer;

                let mut start_cfg = None;
                {
                    let mut st = state.lock().expect("rank workload state lock");
                    let entry = st
                        .pending_sendrecv
                        .entry(comm_id.clone())
                        .or_insert_with(|| SendRecvWait {
                            comm_bytes,
                            sender: None,
                            receiver: None,
                            arrived: Vec::new(),
                        });
                    if entry.comm_bytes != comm_bytes {
                        panic!(
                            "comm_id {:?} sendrecv comm_bytes mismatch: existing bytes={} vs new bytes={}",
                            comm_id, entry.comm_bytes, comm_bytes
                        );
                    }
                    if !entry.arrived.contains(&rank_id) {
                        entry.arrived.push(rank_id);
                    }
                    if entry.arrived.len() > 2 {
                        panic!(
                            "comm_id {:?} sendrecv has >2 participants: {:?}",
                            comm_id, entry.arrived
                        );
                    }
                    match direction {
                        SendRecvDirection::Send => {
                            if let Some(sender) = entry.sender {
                                if sender != rank_id {
                                    panic!(
                                        "comm_id {:?} sendrecv has multiple senders: {} vs {}",
                                        comm_id, sender, rank_id
                                    );
                                }
                            }
                            if let Some(p) = peer {
                                if let Some(receiver) = entry.receiver {
                                    if receiver != p {
                                        panic!(
                                            "comm_id {:?} sendrecv peer mismatch: receiver={} vs peer={}",
                                            comm_id, receiver, p
                                        );
                                    }
                                } else {
                                    entry.receiver = Some(p);
                                }
                            }
                            entry.sender = Some(rank_id);
                        }
                        SendRecvDirection::Recv => {
                            if let Some(receiver) = entry.receiver {
                                if receiver != rank_id {
                                    panic!(
                                        "comm_id {:?} sendrecv has multiple receivers: {} vs {}",
                                        comm_id, receiver, rank_id
                                    );
                                }
                            }
                            if let Some(p) = peer {
                                if let Some(sender) = entry.sender {
                                    if sender != p {
                                        panic!(
                                            "comm_id {:?} sendrecv peer mismatch: sender={} vs peer={}",
                                            comm_id, sender, p
                                        );
                                    }
                                } else {
                                    entry.sender = Some(p);
                                }
                            }
                            entry.receiver = Some(rank_id);
                        }
                    }
                    let ready = entry.sender.is_some()
                        && entry.receiver.is_some()
                        && (entry.arrived.len() == 2 || entry.sender == entry.receiver);
                    if ready {
                        let (Some(sender), Some(receiver)) = (entry.sender, entry.receiver) else {
                            unreachable!("ready implies sender/receiver are set");
                        };
                        let entry = st
                            .pending_sendrecv
                            .remove(&comm_id)
                            .expect("pending sendrecv missing");
                        let src = *st.host_map.get(&sender).expect("unknown host id");
                        let dst = *st.host_map.get(&receiver).expect("unknown host id");
                        let flow_id = st.next_flow_id;
                        st.next_flow_id = st.next_flow_id.saturating_add(1);
                        start_cfg = Some((sender, receiver, entry.comm_bytes, flow_id, src, dst));
                    }
                }

                if let Some((sender, receiver, bytes, flow_id, src, dst)) = start_cfg {
                    let done_state = Arc::clone(&state);
                    let done_cb: ring::RingDoneCallback = Box::new(move |now, sim| {
                        for hid in [sender, receiver] {
                            sim.schedule(
                                now,
                                StartRankStep {
                                    rank_id: hid,
                                    state: Arc::clone(&done_state),
                                },
                            );
                        }
                    });
                    if bytes == 0 || sender == receiver {
                        done_cb(sim.now(), sim);
                        return;
                    }
                    start_p2p_flow(
                        sim, w, protocol, routing, &tcp_cfg, &dctcp_cfg, flow_id, src, dst, bytes,
                        done_cb,
                    );
                }
            }
        }
    }
}

fn parse_protocol(raw: Option<String>, defaults: Option<TransportProtocol>) -> TransportProtocol {
    match raw.as_deref() {
        Some("tcp") => TransportProtocol::Tcp,
        Some("dctcp") => TransportProtocol::Dctcp,
        _ => defaults.unwrap_or(TransportProtocol::Tcp),
    }
}

fn parse_routing(raw: Option<String>, defaults: Option<RoutingMode>) -> CcRoutingMode {
    match raw.as_deref() {
        Some("per_packet") => CcRoutingMode::PerPacket,
        Some("per_flow") => CcRoutingMode::PerFlow,
        _ => match defaults.unwrap_or(RoutingMode::PerFlow) {
            RoutingMode::PerFlow => CcRoutingMode::PerFlow,
            RoutingMode::PerPacket => CcRoutingMode::PerPacket,
        },
    }
}

fn build_topology(world: &mut NetWorld, topo: &TopologySpec) -> Vec<NodeId> {
    match topo {
        TopologySpec::Dumbbell {
            host_link_gbps,
            bottleneck_gbps,
            link_latency_us,
        } => {
            let opts = DumbbellOpts {
                host_link_gbps: host_link_gbps.unwrap_or(100),
                bottleneck_gbps: bottleneck_gbps.unwrap_or(10),
                link_latency: SimTime::from_micros(link_latency_us.unwrap_or(2)),
                ..DumbbellOpts::default()
            };
            let (h0, h1, _) = build_dumbbell(world, &opts);
            vec![h0, h1]
        }
        TopologySpec::FatTree {
            k,
            link_gbps,
            link_latency_us,
        } => {
            let opts = FatTreeOpts {
                k: *k as usize,
                link_gbps: link_gbps.unwrap_or(100),
                link_latency: SimTime::from_micros(link_latency_us.unwrap_or(2)),
            };
            let topo = build_fat_tree(world, &opts);
            topo.hosts
        }
    }
}

fn resolve_hosts(
    hosts: &[HostSpec],
    topo_hosts: &[NodeId],
) -> (
    Vec<usize>,
    HashMap<usize, NodeId>,
    HashMap<usize, Option<String>>,
) {
    let mut host_ids = Vec::new();
    let mut host_map = HashMap::new();
    let mut gpu_map = HashMap::new();

    if hosts.is_empty() {
        for (idx, node) in topo_hosts.iter().enumerate() {
            host_ids.push(idx);
            host_map.insert(idx, *node);
            gpu_map.insert(idx, None);
        }
        return (host_ids, host_map, gpu_map);
    }

    for h in hosts {
        let topo_index = h.topo_index.unwrap_or(h.id);
        if topo_index >= topo_hosts.len() {
            panic!(
                "host {} maps to topo_index {} (topo hosts={})",
                h.id,
                topo_index,
                topo_hosts.len()
            );
        }
        host_ids.push(h.id);
        host_map.insert(h.id, topo_hosts[topo_index]);
        gpu_map.insert(h.id, h.gpu.as_ref().map(|g| g.model.clone()));
    }

    host_ids.sort_unstable();
    (host_ids, host_map, gpu_map)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .init();

    let args = Args::parse();
    let raw = fs::read_to_string(&args.workload).expect("read workload.json");
    let workload: WorkloadSpec = serde_json::from_str(&raw).expect("parse workload.json");

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let topo_hosts = build_topology(&mut world, &workload.topology);
    let (host_ids, host_map, gpu_map) = resolve_hosts(&workload.hosts, &topo_hosts);

    let switch_queue_bytes = if let Some(bytes) = args.queue_bytes {
        Some(bytes)
    } else if let Some(pkts) = args.queue_pkts {
        Some(pkts.saturating_mul(DEFAULT_PKT_BYTES))
    } else {
        None
    };
    if let Some(bytes) = switch_queue_bytes {
        world.net.set_switch_egress_queue_capacity_bytes(bytes);
    }

    let host_queue_bytes = if let Some(bytes) = args.host_queue_bytes {
        bytes
    } else if let Some(pkts) = args.host_queue_pkts {
        pkts.saturating_mul(DEFAULT_PKT_BYTES)
    } else {
        DEFAULT_HOST_EGRESS_QUEUE_BYTES
    };
    world
        .net
        .set_host_egress_queue_capacity_bytes(host_queue_bytes);

    let defaults = workload.defaults.clone().unwrap_or(WorkloadDefaults {
        protocol: Some(TransportProtocol::Tcp),
        routing: Some(RoutingMode::PerFlow),
        bytes_per_element: None,
    });

    let protocol = parse_protocol(args.protocol, defaults.protocol);
    let routing = parse_routing(args.routing, defaults.routing);

    world.net.set_ecmp_hash_mode(match routing {
        CcRoutingMode::PerFlow => EcmpHashMode::Flow,
        CcRoutingMode::PerPacket => EcmpHashMode::Packet,
    });

    if args.viz_json.is_some() {
        world.net.viz = Some(VizLogger::default());
        world.net.emit_viz_meta();
    }

    let collective_handles = Arc::new(Mutex::new(Vec::new()));
    let mut rank_state_check: Option<Arc<Mutex<RankWorkloadState>>> = None;

    if workload.schema_version >= 2 && !workload.ranks.is_empty() {
        let mut ranks = HashMap::new();
        for rank in &workload.ranks {
            ranks.insert(
                rank.id,
                RankState {
                    steps: rank.steps.clone(),
                    idx: 0,
                    pending_async_total: 0,
                    pending_async_by_stream: HashMap::new(),
                    waiting_for_async: AsyncWaitKind::None,
                },
            );
        }
        let state = Arc::new(Mutex::new(RankWorkloadState {
            ranks,
            hosts_all: host_ids.clone(),
            host_map,
            gpu_map,
            protocol,
            routing,
            next_flow_id: 1,
            tcp_cfg: default_tcp_cfg(),
            dctcp_cfg: DctcpConfig::default(),
            pending_collectives: HashMap::new(),
            pending_sendrecv: HashMap::new(),
            collective_handles: Arc::clone(&collective_handles),
        }));
        rank_state_check = Some(Arc::clone(&state));

        for rank_id in host_ids {
            sim.schedule(
                SimTime::ZERO,
                StartRankStep {
                    rank_id,
                    state: Arc::clone(&state),
                },
            );
        }
    } else {
        let state = Arc::new(Mutex::new(WorkloadState {
            steps: workload.steps.clone(),
            hosts_all: host_ids,
            host_map,
            gpu_map,
            protocol,
            routing,
            next_flow_id: 1,
            tcp_cfg: default_tcp_cfg(),
            dctcp_cfg: DctcpConfig::default(),
            collective_handles: Arc::clone(&collective_handles),
        }));

        sim.schedule(
            SimTime::ZERO,
            StartWorkloadStep {
                idx: 0,
                state: Arc::clone(&state),
            },
        );
    }

    if let Some(until_ms) = args.until_ms {
        sim.run_until(SimTime::from_millis(until_ms), &mut world);
    } else {
        sim.run(&mut world);
    }

    if args.until_ms.is_none() {
        if let Some(state) = &rank_state_check {
            let st = state.lock().expect("rank workload state lock");
            if !st.pending_collectives.is_empty() {
                let keys = st.pending_collectives.keys().cloned().collect::<Vec<_>>();
                panic!("unresolved collectives at end of sim: {keys:?}");
            }
            if !st.pending_sendrecv.is_empty() {
                let keys = st.pending_sendrecv.keys().cloned().collect::<Vec<_>>();
                panic!("unresolved sendrecv at end of sim: {keys:?}");
            }
            let pending_async = st
                .ranks
                .iter()
                .filter_map(|(rid, rs)| {
                    if rs.pending_async_total > 0 {
                        Some((*rid, rs.pending_async_total))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if !pending_async.is_empty() {
                panic!("unresolved async collectives at end of sim: {pending_async:?}");
            }
        }
    }

    if args.fct_stats {
        if let Ok(list) = collective_handles.lock() {
            for record in list.iter() {
                let stats = record.handle.stats();
                let start = stats.start_at.unwrap_or(SimTime::ZERO);
                let fct_ns = stats
                    .done_at
                    .map(|d| d.0.saturating_sub(start.0))
                    .unwrap_or(0);
                let p99_ns = percentile_ns(&stats.flow_fct_ns, 0.99).unwrap_or(0);
                let max_flow_ns = stats.flow_fct_ns.iter().copied().max().unwrap_or(0);
                let makespan_ms = fct_ns as f64 / 1_000_000.0;
                let p99_ms = p99_ns as f64 / 1_000_000.0;
                let max_flow_ms = max_flow_ns as f64 / 1_000_000.0;
                println!(
                    "collective_fct step_id={:?} label={:?} comm_id={:?} op={:?} hosts={} comm_bytes={} makespan_ms={:.6} p99_flow_fct_ms={:.6} max_flow_fct_ms={:.6} flows={}",
                    record.step_id,
                    record.label,
                    record.comm_id,
                    record.op,
                    record.hosts,
                    record.comm_bytes,
                    makespan_ms,
                    p99_ms,
                    max_flow_ms,
                    stats.flow_fct_ns.len()
                );
            }
        }
    }

    if let Some(path) = args.viz_json {
        if let Some(v) = world.net.viz.take() {
            let json = serde_json::to_string_pretty(&v.events).expect("serialize viz events");
            fs::write(&path, json).expect("write viz json");
            eprintln!("wrote viz events to {}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_two_rank_dumbbell_world() -> (NetWorld, Vec<usize>, HashMap<usize, NodeId>) {
        let mut world = NetWorld::default();
        let opts = DumbbellOpts::default();
        let (h0, h1, _route) = build_dumbbell(&mut world, &opts);

        // Keep queues large to avoid drops that would add variability to timings.
        world
            .net
            .set_host_egress_queue_capacity_bytes(1024 * 1024 * 1024);
        world
            .net
            .set_switch_egress_queue_capacity_bytes(1024 * 1024 * 1024);

        world.net.viz = Some(VizLogger::default());

        let host_ids = vec![0_usize, 1_usize];
        let mut host_map = HashMap::new();
        host_map.insert(0, h0);
        host_map.insert(1, h1);
        (world, host_ids, host_map)
    }

    fn run_two_rank_workload(
        steps0: Vec<RankStepSpec>,
        steps1: Vec<RankStepSpec>,
    ) -> (
        Simulator,
        NetWorld,
        Arc<Mutex<RankWorkloadState>>,
        Arc<Mutex<Vec<CollectiveRecord>>>,
    ) {
        let mut sim = Simulator::default();
        let (mut world, host_ids, host_map) = build_two_rank_dumbbell_world();

        let mut gpu_map = HashMap::new();
        gpu_map.insert(0, None);
        gpu_map.insert(1, None);

        let collective_handles = Arc::new(Mutex::new(Vec::new()));

        let mut ranks = HashMap::new();
        ranks.insert(
            0,
            RankState {
                steps: steps0,
                idx: 0,
                pending_async_total: 0,
                pending_async_by_stream: HashMap::new(),
                waiting_for_async: AsyncWaitKind::None,
            },
        );
        ranks.insert(
            1,
            RankState {
                steps: steps1,
                idx: 0,
                pending_async_total: 0,
                pending_async_by_stream: HashMap::new(),
                waiting_for_async: AsyncWaitKind::None,
            },
        );

        let state = Arc::new(Mutex::new(RankWorkloadState {
            ranks,
            hosts_all: host_ids.clone(),
            host_map,
            gpu_map,
            protocol: TransportProtocol::Tcp,
            routing: CcRoutingMode::PerFlow,
            next_flow_id: 1,
            tcp_cfg: default_tcp_cfg(),
            dctcp_cfg: DctcpConfig::default(),
            pending_collectives: HashMap::new(),
            pending_sendrecv: HashMap::new(),
            collective_handles: Arc::clone(&collective_handles),
        }));

        for rank_id in host_ids {
            sim.schedule(
                SimTime::ZERO,
                StartRankStep {
                    rank_id,
                    state: Arc::clone(&state),
                },
            );
        }

        sim.run(&mut world);

        (sim, world, state, collective_handles)
    }

    fn gpu_busy_events(world: &NetWorld) -> Vec<(u64, usize, u64, Option<String>)> {
        let Some(v) = &world.net.viz else {
            return Vec::new();
        };
        v.events
            .iter()
            .filter_map(|ev| match &ev.kind {
                VizEventKind::GpuBusy {
                    node,
                    duration_ns,
                    label,
                    ..
                } => Some((ev.t_ns, *node, *duration_ns, label.clone())),
                _ => None,
            })
            .collect()
    }

    fn step_collective(op: &str, comm_bytes: u64, comm_id: &str) -> RankStepSpec {
        RankStepSpec {
            id: None,
            label: Some(format!("{comm_id}:{op}")),
            kind: Some(RankStepKind::Collective),
            op: Some(op.to_string()),
            compute_ms: None,
            comm_bytes: Some(comm_bytes),
            comm_id: Some(comm_id.to_string()),
            comm_stream: None,
            hosts: Some(vec![0, 1]),
            peer: None,
            direction: None,
        }
    }

    fn step_compute(label: &str, compute_ms: f64) -> RankStepSpec {
        RankStepSpec {
            id: None,
            label: Some(label.to_string()),
            kind: Some(RankStepKind::Compute),
            op: None,
            compute_ms: Some(compute_ms),
            comm_bytes: None,
            comm_id: None,
            comm_stream: None,
            hosts: None,
            peer: None,
            direction: None,
        }
    }

    fn step_wait(label: &str) -> RankStepSpec {
        RankStepSpec {
            id: None,
            label: Some(label.to_string()),
            kind: Some(RankStepKind::CollectiveWait),
            op: None,
            compute_ms: None,
            comm_bytes: None,
            comm_id: None,
            comm_stream: None,
            hosts: None,
            peer: None,
            direction: None,
        }
    }

    fn step_sendrecv(
        comm_id: &str,
        direction: SendRecvDirection,
        peer: Option<usize>,
        comm_bytes: u64,
    ) -> RankStepSpec {
        RankStepSpec {
            id: None,
            label: Some(format!("{comm_id}:sendrecv")),
            kind: Some(RankStepKind::Sendrecv),
            op: None,
            compute_ms: None,
            comm_bytes: Some(comm_bytes),
            comm_id: Some(comm_id.to_string()),
            comm_stream: None,
            hosts: None,
            peer,
            direction: Some(direction),
        }
    }

    #[test]
    fn async_collective_overlaps_compute_until_collective_wait() {
        let steps = vec![
            step_collective("allreduce_async", 1, "c0"),
            step_compute("overlap_compute", 0.001), // 1us
            step_wait("wait_for_async"),
            step_compute("after_wait_compute", 0.001),
        ];
        let (_sim, world, state, handles) = run_two_rank_workload(steps.clone(), steps.clone());

        let list = handles.lock().expect("handles lock");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].comm_id.as_deref(), Some("c0"));
        let stats = list[0].handle.stats();
        let comm_start = stats.start_at.expect("collective start_at missing").0;
        let comm_done = stats.done_at.expect("collective done_at missing").0;
        assert_eq!(comm_start, 0);
        assert!(comm_done > 0);

        let busy = gpu_busy_events(&world);
        let overlap = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("overlap_compute"))
            .collect::<Vec<_>>();
        let after = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("after_wait_compute"))
            .collect::<Vec<_>>();
        assert_eq!(
            overlap.len(),
            2,
            "expected 2 overlap compute events (one per rank)"
        );
        assert_eq!(
            after.len(),
            2,
            "expected 2 after-wait compute events (one per rank)"
        );

        for (t_ns, node, _dur_ns, _) in overlap {
            assert!(matches!(*node, 0 | 1));
            assert_eq!(*t_ns, 0);
            assert!(*t_ns < comm_done);
        }
        for (t_ns, node, _dur_ns, _) in after {
            assert!(matches!(*node, 0 | 1));
            assert!(*t_ns >= comm_done);
        }

        let st = state.lock().expect("state lock");
        assert!(st.pending_collectives.is_empty());
        assert!(st.pending_sendrecv.is_empty());
        for (rid, rs) in &st.ranks {
            assert_eq!(rs.pending_async_total, 0, "rank {rid} still pending");
        }
    }

    #[test]
    fn async_collective_allows_following_collective_to_overlap() {
        let steps = vec![
            // Large async collective, so the follow-up collective has a chance to overlap.
            step_collective("allreduce_async", 1_000_000, "c0"),
            step_collective("allgather", 1, "c1"),
        ];
        let (_sim, _world, _state, handles) = run_two_rank_workload(steps.clone(), steps.clone());

        let list = handles.lock().expect("handles lock");
        assert_eq!(list.len(), 2);

        let mut by_id = HashMap::new();
        for record in list.iter() {
            let id = record.comm_id.clone().expect("comm_id missing");
            by_id.insert(id, record.handle.stats());
        }

        let c0 = by_id.get("c0").expect("missing c0 stats");
        let c1 = by_id.get("c1").expect("missing c1 stats");
        let c0_done = c0.done_at.expect("c0 done_at missing");
        let c1_start = c1.start_at.expect("c1 start_at missing");
        assert_eq!(c1_start.0, 0, "expected c1 to start immediately");
        assert!(
            c1_start < c0_done,
            "expected c1 to start before c0 completes (overlap)"
        );
    }

    #[test]
    fn async_collective_blocks_following_collective_on_same_comm_stream() {
        let mut c0 = step_collective("allreduce_async", 1_000_000, "c0");
        c0.comm_stream = Some(0);
        let mut c1 = step_collective("allgather", 1, "c1");
        c1.comm_stream = Some(0);

        let steps = vec![c0, c1];
        let (_sim, _world, _state, handles) = run_two_rank_workload(steps.clone(), steps.clone());

        let list = handles.lock().expect("handles lock");
        assert_eq!(list.len(), 2);

        let mut by_id = HashMap::new();
        for record in list.iter() {
            let id = record.comm_id.clone().expect("comm_id missing");
            by_id.insert(id, record.handle.stats());
        }

        let c0 = by_id.get("c0").expect("missing c0 stats");
        let c1 = by_id.get("c1").expect("missing c1 stats");
        let c0_done = c0.done_at.expect("c0 done_at missing");
        let c1_start = c1.start_at.expect("c1 start_at missing");
        assert!(
            c1_start >= c0_done,
            "expected same-stream comm to be serialized"
        );
    }

    #[test]
    fn collective_wait_is_noop_without_pending_async() {
        let steps = vec![
            step_compute("first", 0.001),
            step_wait("wait_should_not_block"),
            step_compute("second", 0.001),
        ];
        let (_sim, world, _state, handles) = run_two_rank_workload(steps.clone(), steps.clone());
        assert!(handles.lock().expect("handles lock").is_empty());

        let busy = gpu_busy_events(&world);
        let first = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("first"))
            .collect::<Vec<_>>();
        let second = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("second"))
            .collect::<Vec<_>>();
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);

        let expected_start_second = compute_duration_ns_from_ms(0.001);
        assert!(expected_start_second > 0);
        for (t_ns, node, _dur_ns, _) in second {
            assert!(matches!(*node, 0 | 1));
            assert_eq!(*t_ns, expected_start_second);
        }
    }

    #[test]
    fn async_collective_starts_when_last_rank_arrives() {
        let rank0 = vec![
            step_collective("allreduce_async", 1, "c0"),
            step_compute("r0_after_launch", 0.001), // 1us
            step_wait("r0_wait"),
            step_compute("r0_after_wait", 0.001),
        ];
        let rank1 = vec![
            step_compute("r1_pre_launch", 0.005), // 5us
            step_collective("allreduce_async", 1, "c0"),
            step_compute("r1_after_launch", 0.001),
            step_wait("r1_wait"),
            step_compute("r1_after_wait", 0.001),
        ];

        let (_sim, world, _state, handles) = run_two_rank_workload(rank0, rank1);

        let list = handles.lock().expect("handles lock");
        assert_eq!(list.len(), 1);
        let stats = list[0].handle.stats();
        let comm_start = stats.start_at.expect("collective start_at missing").0;
        let comm_done = stats.done_at.expect("collective done_at missing").0;

        let expected_start = compute_duration_ns_from_ms(0.005);
        assert_eq!(comm_start, expected_start);
        assert!(comm_done > comm_start);

        let busy = gpu_busy_events(&world);

        // Rank0 starts its post-launch compute immediately, even though the collective has
        // not started yet (rank1 hasn't arrived).
        let r0_after_launch = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("r0_after_launch"))
            .collect::<Vec<_>>();
        assert_eq!(r0_after_launch.len(), 1);
        assert_eq!(r0_after_launch[0].0, 0);

        // Rank1's compute before launch begins at t=0.
        let r1_pre_launch = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("r1_pre_launch"))
            .collect::<Vec<_>>();
        assert_eq!(r1_pre_launch.len(), 1);
        assert_eq!(r1_pre_launch[0].0, 0);

        // Both ranks should start their "after wait" compute exactly at collective completion.
        for lbl in ["r0_after_wait", "r1_after_wait"] {
            let events = busy
                .iter()
                .filter(|(_, _, _, label)| label.as_deref() == Some(lbl))
                .collect::<Vec<_>>();
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0].0, comm_done,
                "label {lbl} did not start at done_at"
            );
        }
    }

    #[test]
    #[should_panic]
    fn collective_comm_id_op_mismatch_panics() {
        let rank0 = vec![step_collective("allreduce", 1, "c0")];
        let rank1 = vec![step_collective("allgather", 1, "c0")];
        let _ = run_two_rank_workload(rank0, rank1);
    }

    #[test]
    #[should_panic]
    fn collective_comm_id_async_mismatch_panics() {
        let rank0 = vec![step_collective("allreduce", 1, "c0")];
        let rank1 = vec![step_collective("allreduce_async", 1, "c0")];
        let _ = run_two_rank_workload(rank0, rank1);
    }

    #[test]
    #[should_panic]
    fn collective_comm_id_comm_bytes_mismatch_panics() {
        let rank0 = vec![step_collective("allreduce", 1, "c0")];
        let rank1 = vec![step_collective("allreduce", 2, "c0")];
        let _ = run_two_rank_workload(rank0, rank1);
    }

    #[test]
    #[should_panic]
    fn collective_comm_id_hosts_mismatch_panics() {
        let rank0 = vec![step_collective("allreduce", 1, "c0")];
        let mut step1 = step_collective("allreduce", 1, "c0");
        step1.hosts = Some(vec![1]);
        let rank1 = vec![step1];
        let _ = run_two_rank_workload(rank0, rank1);
    }

    #[test]
    fn sendrecv_completes_and_unblocks_both_ranks_when_both_arrive() {
        let rank0 = vec![
            step_sendrecv("p0", SendRecvDirection::Send, Some(1), 10_000),
            step_compute("after", 0.001),
        ];
        // Peer is optional on the recv side; we should still match and start exactly once.
        let rank1 = vec![
            step_sendrecv("p0", SendRecvDirection::Recv, None, 10_000),
            step_compute("after", 0.001),
        ];

        let (_sim, world, state, _handles) = run_two_rank_workload(rank0, rank1);

        let busy = gpu_busy_events(&world);
        let after = busy
            .iter()
            .filter(|(_, _, _, label)| label.as_deref() == Some("after"))
            .collect::<Vec<_>>();
        assert_eq!(after.len(), 2, "expected one after-compute per rank");

        let st = state.lock().expect("state lock");
        assert!(st.pending_sendrecv.is_empty());
        assert_eq!(
            st.next_flow_id, 2,
            "expected exactly one sendrecv flow to be started"
        );
    }

    #[test]
    #[should_panic]
    fn sendrecv_comm_bytes_mismatch_panics() {
        let rank0 = vec![step_sendrecv("p0", SendRecvDirection::Send, Some(1), 1)];
        let rank1 = vec![step_sendrecv("p0", SendRecvDirection::Recv, Some(0), 2)];
        let _ = run_two_rank_workload(rank0, rank1);
    }

    #[test]
    #[should_panic]
    fn sendrecv_direction_mismatch_panics() {
        // Both ranks think they're the sender.
        let rank0 = vec![step_sendrecv("p0", SendRecvDirection::Send, Some(1), 1)];
        let rank1 = vec![step_sendrecv("p0", SendRecvDirection::Send, Some(0), 1)];
        let _ = run_two_rank_workload(rank0, rank1);
    }
}
