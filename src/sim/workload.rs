use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadSpec {
    pub schema_version: u32,
    #[serde(default)]
    pub meta: Option<WorkloadMeta>,
    pub topology: TopologySpec,
    #[serde(default)]
    pub defaults: Option<WorkloadDefaults>,
    pub hosts: Vec<HostSpec>,
    #[serde(default)]
    pub steps: Vec<StepSpec>,
    #[serde(default)]
    pub ranks: Vec<RankSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadMeta {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub num_layers: Option<u32>,
    #[serde(default)]
    pub device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopologySpec {
    Dumbbell {
        #[serde(default)]
        host_link_gbps: Option<u64>,
        #[serde(default)]
        bottleneck_gbps: Option<u64>,
        #[serde(default)]
        link_latency_us: Option<u64>,
    },
    FatTree {
        k: u64,
        #[serde(default)]
        link_gbps: Option<u64>,
        #[serde(default)]
        link_latency_us: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDefaults {
    #[serde(default)]
    pub protocol: Option<TransportProtocol>,
    #[serde(default)]
    pub routing: Option<RoutingMode>,
    #[serde(default)]
    pub bytes_per_element: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportProtocol {
    Tcp,
    Dctcp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    PerFlow,
    PerPacket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSpec {
    pub id: usize,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub topo_index: Option<usize>,
    #[serde(default)]
    pub gpu: Option<GpuSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSpec {
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub hosts: Option<Vec<usize>>,
    #[serde(default)]
    pub compute_ms: Option<f64>,
    #[serde(default)]
    pub comm_bytes: Option<u64>,
    #[serde(default)]
    pub protocol: Option<TransportProtocol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankSpec {
    pub id: usize,
    #[serde(default)]
    pub steps: Vec<RankStepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankStepKind {
    Compute,
    Collective,
    /// Wait for completion of previously launched async collective(s).
    ///
    /// This is useful when modeling `*_async` collectives that overlap with
    /// compute: ranks can proceed after launching the collective, and only
    /// block when they reach an explicit wait.
    CollectiveWait,
    Sendrecv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendRecvDirection {
    Send,
    Recv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankStepSpec {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub kind: Option<RankStepKind>,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub compute_ms: Option<f64>,
    #[serde(default)]
    pub comm_bytes: Option<u64>,
    #[serde(default)]
    pub comm_id: Option<String>,
    /// Optional per-rank communication stream identifier.
    ///
    /// When present, async collectives on the same stream are ordered: a later
    /// comm step on that stream will wait for prior async comm to complete.
    #[serde(default)]
    pub comm_stream: Option<u32>,
    #[serde(default)]
    pub hosts: Option<Vec<usize>>,
    #[serde(default)]
    pub peer: Option<usize>,
    #[serde(default)]
    pub direction: Option<SendRecvDirection>,
}
