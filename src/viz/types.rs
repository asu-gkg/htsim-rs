use serde::{Deserialize, Serialize};

/// 可视化事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VizEventKind {
    /// 仿真/拓扑元信息（建议作为 t=0 的第一条事件）
    Meta {
        nodes: Vec<VizNodeInfo>,
        links: Vec<VizLinkInfo>,
    },
    /// 节点开始处理一个到达的数据包（可用于区分 host/switch）
    NodeRx {
        node: usize,
        node_kind: VizNodeKind,
        node_name: String,
    },
    /// 节点决定把包转发到下一跳（发生在入队之前）
    NodeForward { node: usize, next: usize },
    /// packet 入队（发生在某条单向链路的队列上）
    Enqueue {
        link_from: usize,
        link_to: usize,
        q_bytes: u64,
        q_cap_bytes: u64,
    },
    /// packet 出队并开始发送（链路序列化开始）
    TxStart {
        link_from: usize,
        link_to: usize,
        depart_ns: u64,
        arrive_ns: u64,
    },
    /// packet 在某节点“到达事件”触发（DeliverPacket）
    ArriveNode { node: usize },
    /// packet 在目的节点被标记为 delivered（统计+上层处理）
    Delivered { node: usize },
    /// DropTail 丢包
    Drop {
        link_from: usize,
        link_to: usize,
        q_bytes: u64,
        q_cap_bytes: u64,
    },
    /// TCP：发送数据段
    TcpSendData(VizTcp),
    /// TCP：发送 ACK
    TcpSendAck(VizTcp),
    /// TCP：收到 ACK（用于驱动 cwnd/继续发送）
    TcpRecvAck(VizTcp),
    /// TCP：RTO 超时触发重传
    TcpRto(VizTcp),
    /// DCTCP：真实 cwnd 采样（避免前端推断偏差）
    DctcpCwnd {
        conn_id: u64,
        cwnd_bytes: u64,
        ssthresh_bytes: u64,
        inflight_bytes: u64,
        alpha: f64,
    },
}

/// packet 的类别（便于可视化上色）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VizPacketKind {
    Data,
    Ack,
    Other,
}

/// 节点类型（用于可视化区分 host/switch）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VizNodeKind {
    Host,
    Switch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizNodeInfo {
    pub id: usize,
    pub name: String,
    pub kind: VizNodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizLinkInfo {
    pub from: usize,
    pub to: usize,
    /// 单向链路带宽（bps）
    pub bandwidth_bps: u64,
    /// 单向传播时延（ns）
    pub latency_ns: u64,
    /// 队列容量（bytes）
    pub q_cap_bytes: u64,
}

/// 与 TCP 有关的字段（用于展示 seq/ack/cwnd 等）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VizTcp {
    pub conn_id: u64,
    pub seq: Option<u64>,
    pub len: Option<u32>,
    pub ack: Option<u64>,
    /// 是否为重传（RTO/快速重传触发的 resend）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrans: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ecn_echo: Option<bool>,
}

/// 一个可回放的事件（JSON）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VizEvent {
    /// 仿真时间（纳秒，和 `SimTime.0` 同口径）
    pub t_ns: u64,
    pub pkt_id: Option<u64>,
    pub flow_id: Option<u64>,
    pub pkt_bytes: Option<u32>,
    pub pkt_kind: Option<VizPacketKind>,
    #[serde(flatten)]
    pub kind: VizEventKind,
}

/// 一个简单的事件收集器（存内存，仿真结束写 JSON 文件）
#[derive(Debug, Default)]
pub struct VizLogger {
    pub events: Vec<VizEvent>,
}

impl VizLogger {
    pub fn push(&mut self, ev: VizEvent) {
        self.events.push(ev);
    }
}
