use clap::Parser;
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpDoneCallback};
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpDoneCallback};
use htsim_rs::queue::DEFAULT_PKT_BYTES;
use htsim_rs::sim::{
    HostSpec, RankStepKind, RankStepSpec, RoutingMode, SendRecvDirection, SimTime, Simulator,
    StepSpec, TopologySpec, TransportProtocol, WorkloadDefaults, WorkloadSpec,
};
use htsim_rs::topo::dumbbell::{build_dumbbell, DumbbellOpts};
use htsim_rs::topo::fat_tree::{build_fat_tree, FatTreeOpts};
use htsim_rs::viz::{VizEvent, VizEventKind, VizLogger};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Parser)]
#[command(name = "workload-sim", about = "Run workload.json on htsim-rs network simulator")]
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

    /// Print per-allreduce flow completion time (FCT) stats
    #[arg(long)]
    fct_stats: bool,

    /// Override all link queue capacity in bytes
    #[arg(long)]
    queue_bytes: Option<u64>,

    /// Override all link queue capacity in packets (1500B each)
    #[arg(long)]
    queue_pkts: Option<u64>,
}

struct AllreduceRecord {
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
    allreduce_handles: Arc<Mutex<Vec<AllreduceRecord>>>,
}

struct StartWorkloadStep {
    idx: usize,
    state: Arc<Mutex<WorkloadState>>,
}

struct RankState {
    steps: Vec<RankStepSpec>,
    idx: usize,
}

struct CollectiveWait {
    hosts: Vec<usize>,
    comm_bytes: u64,
    op: String,
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
    allreduce_handles: Arc<Mutex<Vec<AllreduceRecord>>>,
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
            CcRoutingMode::PerPacket => TcpConn::new_dynamic(
                flow_id,
                src,
                dst,
                chunk_bytes,
                self.cfg.clone(),
            ),
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
            CcRoutingMode::PerPacket => DctcpConn::new_dynamic(
                flow_id,
                src,
                dst,
                chunk_bytes,
                self.cfg.clone(),
            ),
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

impl htsim_rs::sim::Event for StartWorkloadStep {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn htsim_rs::sim::World) {
        let StartWorkloadStep { idx, state } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let (step, hosts, protocol, routing, next_flow_id, gpu_map, tcp_cfg, dctcp_cfg) = {
            let mut st = state.lock().expect("workload state lock");
            if idx >= st.steps.len() {
                return;
            }
            let step = st.steps[idx].clone();
            let hosts = step
                .hosts
                .clone()
                .unwrap_or_else(|| st.hosts_all.clone());
            let protocol = step.protocol.unwrap_or(st.protocol);
            let routing = st.routing;
            let next_flow_id = st.next_flow_id;
            let gpu_map = st.gpu_map.clone();
            let tcp_cfg = st.tcp_cfg.clone();
            let dctcp_cfg = st.dctcp_cfg.clone();
            (step, hosts, protocol, routing, next_flow_id, gpu_map, tcp_cfg, dctcp_cfg)
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
            Arc::clone(&st.allreduce_handles)
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
        let record = AllreduceRecord {
            step_id: step.id,
            label: step.label.clone(),
            comm_id: None,
            op: None,
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

        let (step, host_node, gpu, protocol, routing, tcp_cfg, dctcp_cfg, hosts_all) = {
            let mut st = state.lock().expect("rank workload state lock");
            let rank_state = match st.ranks.get_mut(&rank_id) {
                Some(entry) => entry,
                None => return,
            };
            if rank_state.idx >= rank_state.steps.len() {
                return;
            }
            let step = rank_state.steps[rank_state.idx].clone();
            rank_state.idx = rank_state.idx.saturating_add(1);
            let host_node = *st.host_map.get(&rank_id).expect("unknown host id");
            let gpu = st.gpu_map.get(&rank_id).and_then(|g| g.clone());
            (
                step,
                host_node,
                gpu,
                st.protocol,
                st.routing,
                st.tcp_cfg.clone(),
                st.dctcp_cfg.clone(),
                st.hosts_all.clone(),
            )
        };

        match rank_step_kind(&step) {
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
                let op = step.op.clone().unwrap_or_else(|| "allreduce".to_string());
                let hosts = step.hosts.clone().unwrap_or_else(|| hosts_all.clone());

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
                            arrived: Vec::new(),
                        });
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
                            ));
                        } else {
                            let host_nodes = entry
                                .hosts
                                .iter()
                                .map(|hid| *st.host_map.get(hid).expect("unknown host id"))
                                .collect::<Vec<_>>();
                            let ranks = host_nodes.len();
                            let total_steps = ranks.saturating_sub(1) * 2;
                            let flow_span = (ranks as u64)
                                .saturating_mul(total_steps as u64)
                                .max(1);
                            let start_flow_id = st.next_flow_id;
                            st.next_flow_id = st.next_flow_id.saturating_add(flow_span);
                            start_cfg = Some((
                                Some((host_nodes, start_flow_id)),
                                entry.hosts,
                                entry.comm_bytes,
                                Some(comm_id.clone()),
                                Some(entry.op),
                            ));
                        }
                    }
                }

                if let Some((maybe_hosts, hosts, bytes, comm_id, op)) = start_cfg {
                    let done_state = Arc::clone(&state);
                    let done_hosts = hosts.clone();
                    let done_cb: ring::RingAllreduceDoneCallback = Box::new(move |now, sim| {
                        for hid in &done_hosts {
                            sim.schedule(
                                now,
                                StartRankStep {
                                    rank_id: *hid,
                                    state: Arc::clone(&done_state),
                                },
                            );
                        }
                    });
                    if bytes == 0 || hosts.len() <= 1 {
                        done_cb(sim.now(), sim);
                        return;
                    }
                    let (host_nodes, start_flow_id) =
                        maybe_hosts.expect("collective config missing");
                    let chunk_bytes = (bytes + hosts.len() as u64 - 1) / hosts.len() as u64;
                    let transport: Box<dyn RingTransport> = match protocol {
                        TransportProtocol::Tcp => Box::new(TcpRingTransport { cfg: tcp_cfg }),
                        TransportProtocol::Dctcp => Box::new(DctcpRingTransport { cfg: dctcp_cfg }),
                    };
                    let handles = {
                        let st = state.lock().expect("rank workload state lock");
                        Arc::clone(&st.allreduce_handles)
                    };
                    let handle = ring::start_ring_allreduce_at(
                        sim,
                        RingAllreduceConfig {
                            ranks: host_nodes.len(),
                            hosts: host_nodes,
                            chunk_bytes,
                            routing,
                            start_flow_id,
                            transport,
                            done_cb: Some(done_cb),
                        },
                        sim.now(),
                    );
                    let record = AllreduceRecord {
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
                    if !entry.arrived.contains(&rank_id) {
                        entry.arrived.push(rank_id);
                    }
                    match direction {
                        SendRecvDirection::Send => {
                            entry.sender = Some(rank_id);
                            if entry.receiver.is_none() {
                                entry.receiver = peer;
                            }
                        }
                        SendRecvDirection::Recv => {
                            entry.receiver = Some(rank_id);
                            if entry.sender.is_none() {
                                entry.sender = peer;
                            }
                        }
                    }
                    if let (Some(sender), Some(receiver)) = (entry.sender, entry.receiver) {
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
                        sim,
                        w,
                        protocol,
                        routing,
                        &tcp_cfg,
                        &dctcp_cfg,
                        flow_id,
                        src,
                        dst,
                        bytes,
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

fn resolve_hosts(hosts: &[HostSpec], topo_hosts: &[NodeId]) -> (Vec<usize>, HashMap<usize, NodeId>, HashMap<usize, Option<String>>) {
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

    let queue_bytes = if let Some(bytes) = args.queue_bytes {
        Some(bytes)
    } else if let Some(pkts) = args.queue_pkts {
        Some(pkts.saturating_mul(DEFAULT_PKT_BYTES))
    } else {
        None
    };
    if let Some(bytes) = queue_bytes {
        world.net.set_all_link_queue_capacity_bytes(bytes);
    }

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

    let allreduce_handles = Arc::new(Mutex::new(Vec::new()));

    if workload.schema_version >= 2 && !workload.ranks.is_empty() {
        let mut ranks = HashMap::new();
        for rank in &workload.ranks {
            ranks.insert(
                rank.id,
                RankState {
                    steps: rank.steps.clone(),
                    idx: 0,
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
            tcp_cfg: TcpConfig::default(),
            dctcp_cfg: DctcpConfig::default(),
            pending_collectives: HashMap::new(),
            pending_sendrecv: HashMap::new(),
            allreduce_handles: Arc::clone(&allreduce_handles),
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
    } else {
        let state = Arc::new(Mutex::new(WorkloadState {
            steps: workload.steps.clone(),
            hosts_all: host_ids,
            host_map,
            gpu_map,
            protocol,
            routing,
            next_flow_id: 1,
            tcp_cfg: TcpConfig::default(),
            dctcp_cfg: DctcpConfig::default(),
            allreduce_handles: Arc::clone(&allreduce_handles),
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

    if args.fct_stats {
        if let Ok(list) = allreduce_handles.lock() {
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
                    "allreduce_fct step_id={:?} label={:?} comm_id={:?} op={:?} hosts={} comm_bytes={} makespan_ms={:.6} p99_flow_fct_ms={:.6} max_flow_fct_ms={:.6} flows={}",
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
