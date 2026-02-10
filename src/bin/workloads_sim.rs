use clap::Parser;
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpDoneCallback};
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpDoneCallback};
use htsim_rs::queue::DEFAULT_PKT_BYTES;
use htsim_rs::sim::{
    RankStepKind, RankStepSpec, RoutingMode, SendRecvDirection, SimTime, Simulator, TopologySpec,
    TransportProtocol, WorkloadDefaults, WorkloadSpec,
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
    name = "workloads-sim",
    about = "Run multiple workload.json files on htsim-rs network simulator"
)]
struct Args {
    /// Path to workload.json (repeatable)
    #[arg(long = "workload", num_args = 1..)]
    workload: Vec<PathBuf>,

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

struct AllreduceRecord {
    step_id: Option<u64>,
    label: Option<String>,
    comm_id: Option<String>,
    op: Option<String>,
    comm_bytes: u64,
    hosts: usize,
    handle: ring::RingAllreduceHandle,
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
                            let flow_span =
                                (ranks as u64).saturating_mul(total_steps as u64).max(1);
                            let start_flow_id = st.next_flow_id;
                            st.next_flow_id = st.next_flow_id.saturating_add(flow_span);
                            start_cfg = Some((
                                Some((start_flow_id, host_nodes)),
                                entry.hosts,
                                entry.comm_bytes,
                                Some(comm_id.clone()),
                                Some(entry.op),
                            ));
                        }
                    }
                }

                if let Some((start_cfg, hosts, bytes, comm_id, op)) = start_cfg {
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
                    let (start_flow_id, host_nodes) =
                        start_cfg.expect("ring allreduce config missing");
                    let ranks = host_nodes.len();
                    let chunk_bytes = (bytes + ranks as u64 - 1) / ranks as u64;
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
                        sim, w, protocol, routing, &tcp_cfg, &dctcp_cfg, flow_id, src, dst, bytes,
                        done_cb,
                    );
                }
            }
        }
    }
}

fn parse_protocol(raw: Option<String>, defaults: TransportProtocol) -> TransportProtocol {
    match raw.as_deref() {
        Some("tcp") => TransportProtocol::Tcp,
        Some("dctcp") => TransportProtocol::Dctcp,
        _ => defaults,
    }
}

fn parse_routing(raw: Option<String>, defaults: RoutingMode) -> CcRoutingMode {
    match raw.as_deref() {
        Some("per_packet") => CcRoutingMode::PerPacket,
        Some("per_flow") => CcRoutingMode::PerFlow,
        _ => match defaults {
            RoutingMode::PerFlow => CcRoutingMode::PerFlow,
            RoutingMode::PerPacket => CcRoutingMode::PerPacket,
        },
    }
}

fn topology_eq(a: &TopologySpec, b: &TopologySpec) -> bool {
    match (a, b) {
        (
            TopologySpec::Dumbbell {
                host_link_gbps: a_host_link_gbps,
                bottleneck_gbps: a_bottleneck_gbps,
                link_latency_us: a_link_latency_us,
            },
            TopologySpec::Dumbbell {
                host_link_gbps: b_host_link_gbps,
                bottleneck_gbps: b_bottleneck_gbps,
                link_latency_us: b_link_latency_us,
            },
        ) => {
            a_host_link_gbps == b_host_link_gbps
                && a_bottleneck_gbps == b_bottleneck_gbps
                && a_link_latency_us == b_link_latency_us
        }
        (
            TopologySpec::FatTree {
                k: a_k,
                link_gbps: a_link_gbps,
                link_latency_us: a_link_latency_us,
            },
            TopologySpec::FatTree {
                k: b_k,
                link_gbps: b_link_gbps,
                link_latency_us: b_link_latency_us,
            },
        ) => a_k == b_k && a_link_gbps == b_link_gbps && a_link_latency_us == b_link_latency_us,
        _ => false,
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

fn build_dc_pools(topo: &TopologySpec, topo_hosts: usize) -> Vec<Vec<usize>> {
    match topo {
        TopologySpec::Dumbbell { .. } => vec![(0..topo_hosts).collect()],
        TopologySpec::FatTree { k, .. } => {
            let k = *k as usize;
            let half = k / 2;
            let hosts_per_pod = half.saturating_mul(half);
            let expected = k.saturating_mul(hosts_per_pod);
            if expected != topo_hosts {
                panic!(
                    "fat_tree k={} expects {} hosts but built {}",
                    k, expected, topo_hosts
                );
            }
            let mut pools = Vec::with_capacity(k);
            for pod in 0..k {
                let mut indices = Vec::with_capacity(hosts_per_pod);
                for slot in 0..hosts_per_pod {
                    let edge = slot / half;
                    let host = slot % half;
                    let idx = (pod * half + edge) * half + host;
                    indices.push(idx);
                }
                pools.push(indices);
            }
            pools
        }
    }
}

fn remap_rank_steps(
    tenant_idx: usize,
    steps: &[RankStepSpec],
    id_map: &HashMap<usize, usize>,
    default_hosts: &[usize],
) -> Vec<RankStepSpec> {
    let mut out = Vec::with_capacity(steps.len());
    for step in steps {
        let mut s = step.clone();
        if let Some(peer) = s.peer {
            let mapped = *id_map
                .get(&peer)
                .unwrap_or_else(|| panic!("tenant {} unknown peer id {}", tenant_idx, peer));
            s.peer = Some(mapped);
        }
        if let Some(hosts) = &s.hosts {
            let mapped = hosts
                .iter()
                .map(|hid| {
                    *id_map.get(hid).unwrap_or_else(|| {
                        panic!(
                            "tenant {} unknown host id {} in step.hosts",
                            tenant_idx, hid
                        )
                    })
                })
                .collect::<Vec<_>>();
            s.hosts = Some(mapped);
        } else if matches!(rank_step_kind(&s), RankStepKind::Collective) {
            s.hosts = Some(default_hosts.to_vec());
        }
        if let Some(comm_id) = &s.comm_id {
            s.comm_id = Some(format!("t{}:{}", tenant_idx, comm_id));
        }
        if let Some(label) = &s.label {
            s.label = Some(format!("t{}:{}", tenant_idx, label));
        }
        out.push(s);
    }
    out
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
    let mut workloads = Vec::with_capacity(args.workload.len());
    for path in &args.workload {
        let raw = fs::read_to_string(path).unwrap_or_else(|_| panic!("read {}", path.display()));
        let spec: WorkloadSpec = serde_json::from_str(&raw)
            .unwrap_or_else(|_| panic!("parse workload.json {}", path.display()));
        workloads.push((path.clone(), spec));
    }
    if workloads.is_empty() {
        panic!("missing --workload");
    }

    let first_topo = workloads[0].1.topology.clone();
    for (path, w) in &workloads {
        if w.schema_version < 2 || w.ranks.is_empty() {
            panic!(
                "workloads_sim only supports schema_version>=2 with ranks; got schema_version={} ranks={} ({})",
                w.schema_version,
                w.ranks.len(),
                path.display()
            );
        }
        if !topology_eq(&first_topo, &w.topology) {
            panic!(
                "all workloads must share the same topology; mismatch at {}",
                path.display()
            );
        }
    }

    let mut sim = Simulator::default();
    let mut world = NetWorld::default();

    let topo_hosts = build_topology(&mut world, &first_topo);

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

    let defaults_first = workloads[0].1.defaults.clone().unwrap_or(WorkloadDefaults {
        protocol: Some(TransportProtocol::Tcp),
        routing: Some(RoutingMode::PerFlow),
        bytes_per_element: None,
    });
    let default_protocol_first = defaults_first.protocol.unwrap_or(TransportProtocol::Tcp);
    let default_routing_first = defaults_first.routing.unwrap_or(RoutingMode::PerFlow);

    // If the user didn't override protocol/routing, require all workloads to agree to
    // avoid surprising mixed runs.
    if args.protocol.is_none() || args.routing.is_none() {
        for (path, w) in &workloads {
            let defaults = w.defaults.clone().unwrap_or(WorkloadDefaults {
                protocol: None,
                routing: None,
                bytes_per_element: None,
            });
            if args.protocol.is_none() {
                let p = defaults.protocol.unwrap_or(default_protocol_first);
                if p != default_protocol_first {
                    panic!(
                        "protocol mismatch (override with --protocol): {} has {:?}, first is {:?}",
                        path.display(),
                        p,
                        default_protocol_first
                    );
                }
            }
            if args.routing.is_none() {
                let r = defaults.routing.unwrap_or(default_routing_first);
                if r != default_routing_first {
                    panic!(
                        "routing mismatch (override with --routing): {} has {:?}, first is {:?}",
                        path.display(),
                        r,
                        default_routing_first
                    );
                }
            }
        }
    }

    let protocol = parse_protocol(args.protocol, default_protocol_first);
    let routing = parse_routing(args.routing, default_routing_first);

    world.net.set_ecmp_hash_mode(match routing {
        CcRoutingMode::PerFlow => EcmpHashMode::Flow,
        CcRoutingMode::PerPacket => EcmpHashMode::Packet,
    });

    if args.viz_json.is_some() {
        world.net.viz = Some(VizLogger::default());
        world.net.emit_viz_meta();
    }

    let pools = build_dc_pools(&first_topo, topo_hosts.len());
    let dc_count = pools.len().max(1);
    let mut dc_next = vec![0usize; dc_count];
    let mut next_dc_start = 0usize;

    let mut ranks = HashMap::new();
    let mut hosts_all = Vec::new();
    let mut host_map = HashMap::new();
    let mut gpu_map = HashMap::new();
    let mut next_rank_id = 0usize;

    let allreduce_handles = Arc::new(Mutex::new(Vec::new()));

    for (tenant_idx, (path, w)) in workloads.iter().enumerate() {
        let old_rank_ids = w.ranks.iter().map(|r| r.id).collect::<Vec<_>>();
        let mut id_map = HashMap::new();
        let mut tenant_hosts_new = Vec::with_capacity(old_rank_ids.len());

        let fallback_gpu = w.meta.as_ref().and_then(|m| m.device.clone());
        let mut gpu_by_old = HashMap::new();
        for h in &w.hosts {
            gpu_by_old.insert(h.id, h.gpu.as_ref().map(|g| g.model.clone()));
        }

        let mut dc_hist = vec![0usize; dc_count];
        let mut dc_cursor = next_dc_start;

        for old_id in &old_rank_ids {
            if next_rank_id >= topo_hosts.len() {
                panic!(
                    "not enough topology hosts: need >= {} ranks but topology has {} hosts",
                    next_rank_id + 1,
                    topo_hosts.len()
                );
            }
            let new_id = next_rank_id;
            next_rank_id += 1;
            id_map.insert(*old_id, new_id);
            tenant_hosts_new.push(new_id);
            hosts_all.push(new_id);

            let mut dc = dc_cursor;
            let mut found = None;
            for _ in 0..dc_count {
                if dc_next[dc] < pools[dc].len() {
                    let topo_index = pools[dc][dc_next[dc]];
                    dc_next[dc] += 1;
                    found = Some((dc, topo_index));
                    break;
                }
                dc = (dc + 1) % dc_count;
            }
            let (dc_used, topo_index) = found.unwrap_or_else(|| {
                panic!(
                    "not enough topology hosts: requested {} ranks but topology has {} hosts",
                    hosts_all.len(),
                    topo_hosts.len()
                )
            });
            dc_cursor = dc_used;
            dc_hist[dc_used] = dc_hist[dc_used].saturating_add(1);

            host_map.insert(new_id, topo_hosts[topo_index]);
            let gpu = gpu_by_old
                .get(old_id)
                .and_then(|g| g.clone())
                .or_else(|| fallback_gpu.clone());
            gpu_map.insert(new_id, gpu);
        }

        let dist = dc_hist
            .iter()
            .enumerate()
            .filter_map(|(dc, count)| {
                if *count == 0 {
                    None
                } else {
                    Some(format!("dc{}:{}", dc, count))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "tenant={} workload={} ranks={} placement=[{}]",
            tenant_idx,
            path.display(),
            old_rank_ids.len(),
            dist
        );

        for rank in &w.ranks {
            let new_rank_id = *id_map
                .get(&rank.id)
                .unwrap_or_else(|| panic!("tenant {} missing rank id {}", tenant_idx, rank.id));
            let steps = remap_rank_steps(tenant_idx, &rank.steps, &id_map, &tenant_hosts_new);
            ranks.insert(new_rank_id, RankState { steps, idx: 0 });
        }

        next_dc_start = (next_dc_start + 1) % dc_count;
    }

    let state = Arc::new(Mutex::new(RankWorkloadState {
        ranks,
        hosts_all: hosts_all.clone(),
        host_map,
        gpu_map,
        protocol,
        routing,
        next_flow_id: 1,
        tcp_cfg: default_tcp_cfg(),
        dctcp_cfg: DctcpConfig::default(),
        pending_collectives: HashMap::new(),
        pending_sendrecv: HashMap::new(),
        allreduce_handles: Arc::clone(&allreduce_handles),
    }));

    for rank_id in hosts_all {
        sim.schedule(
            SimTime::ZERO,
            StartRankStep {
                rank_id,
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
