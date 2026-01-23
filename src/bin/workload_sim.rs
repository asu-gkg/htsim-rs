use clap::Parser;
use htsim_rs::cc::ring::{self, RingAllreduceConfig, RingTransport, RoutingMode as CcRoutingMode};
use htsim_rs::net::{EcmpHashMode, NetWorld, NodeId};
use htsim_rs::proto::dctcp::{DctcpConfig, DctcpConn, DctcpDoneCallback};
use htsim_rs::proto::tcp::{TcpConfig, TcpConn, TcpDoneCallback};
use htsim_rs::sim::{
    HostSpec, RoutingMode, SimTime, Simulator, StepSpec, TopologySpec, TransportProtocol,
    WorkloadDefaults, WorkloadSpec,
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
}

struct StartWorkloadStep {
    idx: usize,
    state: Arc<Mutex<WorkloadState>>,
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

impl StartWorkloadStep {
    fn compute_duration_ns(step: &StepSpec) -> u64 {
        let ms = step.compute_ms.unwrap_or(0.0);
        if !ms.is_finite() || ms <= 0.0 {
            return 0;
        }
        (ms * 1_000_000.0).round() as u64
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

        {
            let mut st = state.lock().expect("workload state lock");
            st.next_flow_id = st.next_flow_id.saturating_add(flow_span);
        }

        ring::start_ring_allreduce_at(
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
    }));

    sim.schedule(
        SimTime::ZERO,
        StartWorkloadStep {
            idx: 0,
            state: Arc::clone(&state),
        },
    );

    if let Some(until_ms) = args.until_ms {
        sim.run_until(SimTime::from_millis(until_ms), &mut world);
    } else {
        sim.run(&mut world);
    }

    if let Some(path) = args.viz_json {
        if let Some(v) = world.net.viz.take() {
            let json = serde_json::to_string_pretty(&v.events).expect("serialize viz events");
            fs::write(&path, json).expect("write viz json");
            eprintln!("wrote viz events to {}", path.display());
        }
    }
}
