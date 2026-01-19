//! TCP 协议实现（用于仿真实验）

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::net::{NetWorld, Network, NodeId};
use crate::proto::{TcpSegment, Transport};
use crate::sim::{Event, SimTime, Simulator, World};

/// 一个 TCP 连接的唯一标识（复用 `flow_id` 的语义）。
pub type TcpConnId = u64;

#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// MSS（数据段载荷大小，字节）
    pub mss: u32,
    /// ACK 包大小（字节）
    pub ack_bytes: u32,
    /// 初始 cwnd（字节）
    pub init_cwnd_bytes: u64,
    /// 初始 ssthresh（字节）
    pub init_ssthresh_bytes: u64,
    /// 初始 RTO
    pub init_rto: SimTime,
    /// 最小 RTO
    pub min_rto: SimTime,
    /// 最大 RTO（用于退避上限）
    pub max_rto: SimTime,
    /// 是否启用三次握手
    pub handshake: bool,
    /// 应用层限速（包/秒）
    pub app_limited_pps: Option<u64>,
}

impl Default for TcpConfig {
    fn default() -> Self {
        let mss = 1460;
        Self {
            mss,
            ack_bytes: 64,
            init_cwnd_bytes: (mss as u64).saturating_mul(10),
            init_ssthresh_bytes: (mss as u64).saturating_mul(1_000),
            init_rto: SimTime::from_micros(200),
            min_rto: SimTime::from_micros(200),
            max_rto: SimTime::from_millis(200),
            handshake: false,
            app_limited_pps: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TcpRoutingMode {
    Preset,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SenderState {
    SynSent,
    Established,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverState {
    Idle,
    SynReceived,
    Established,
}

#[derive(Debug, Clone)]
struct SentSeg {
    len: u32,
    sent_at: SimTime,
    retransmitted: bool,
}

#[derive(Debug, Clone)]
pub struct TcpConn {
    pub id: TcpConnId,
    pub src: NodeId,
    pub dst: NodeId,
    pub fwd_route: Vec<NodeId>,
    pub rev_route: Vec<NodeId>,
    pub total_bytes: u64,
    pub cfg: TcpConfig,
    pub routing_mode: TcpRoutingMode,

    // sender
    next_seq: u64,
    last_acked: u64,
    cwnd_bytes: u64,
    ssthresh_bytes: u64,
    dup_acks: u32,
    rto: SimTime,
    rto_deadline: Option<SimTime>,
    rto_token: u64,
    srtt: Option<SimTime>,
    rttvar: SimTime,
    inflight: BTreeMap<u64, SentSeg>, // seq -> segment
    recover: u64,
    in_fast_recovery: bool,

    // receiver
    rcv_nxt: u64,
    out_of_order: BTreeMap<u64, u32>,

    // handshake
    sender_state: SenderState,
    receiver_state: ReceiverState,
    syn_sent_at: Option<SimTime>,
    syn_retries: u32,

    // stats
    start_at: Option<SimTime>,
    done_at: Option<SimTime>,
}

impl TcpConn {
    pub fn new(
        id: TcpConnId,
        src: NodeId,
        dst: NodeId,
        fwd_route: Vec<NodeId>,
        total_bytes: u64,
        cfg: TcpConfig,
    ) -> Self {
        let mut rev_route = fwd_route.clone();
        rev_route.reverse();
        let init_rto = cfg.init_rto;
        let cwnd = cfg.init_cwnd_bytes.max(cfg.mss as u64);
        let ssthresh = cfg.init_ssthresh_bytes.max(cfg.mss as u64);
        let sender_state = if cfg.handshake {
            SenderState::SynSent
        } else {
            SenderState::Established
        };
        let receiver_state = if cfg.handshake {
            ReceiverState::Idle
        } else {
            ReceiverState::Established
        };
        Self {
            id,
            src,
            dst,
            fwd_route,
            rev_route,
            total_bytes,
            cfg,
            routing_mode: TcpRoutingMode::Preset,
            next_seq: 0,
            last_acked: 0,
            cwnd_bytes: cwnd,
            ssthresh_bytes: ssthresh,
            dup_acks: 0,
            rto: init_rto,
            rto_deadline: None,
            rto_token: 0,
            srtt: None,
            rttvar: SimTime::ZERO,
            inflight: BTreeMap::new(),
            recover: 0,
            in_fast_recovery: false,
            rcv_nxt: 0,
            out_of_order: BTreeMap::new(),
            sender_state,
            receiver_state,
            syn_sent_at: None,
            syn_retries: 0,
            start_at: None,
            done_at: None,
        }
    }

    pub fn new_dynamic(
        id: TcpConnId,
        src: NodeId,
        dst: NodeId,
        total_bytes: u64,
        cfg: TcpConfig,
    ) -> Self {
        let init_rto = cfg.init_rto;
        let cwnd = cfg.init_cwnd_bytes.max(cfg.mss as u64);
        let ssthresh = cfg.init_ssthresh_bytes.max(cfg.mss as u64);
        let sender_state = if cfg.handshake {
            SenderState::SynSent
        } else {
            SenderState::Established
        };
        let receiver_state = if cfg.handshake {
            ReceiverState::Idle
        } else {
            ReceiverState::Established
        };
        Self {
            id,
            src,
            dst,
            fwd_route: Vec::new(),
            rev_route: Vec::new(),
            total_bytes,
            cfg,
            routing_mode: TcpRoutingMode::Dynamic,
            next_seq: 0,
            last_acked: 0,
            cwnd_bytes: cwnd,
            ssthresh_bytes: ssthresh,
            dup_acks: 0,
            rto: init_rto,
            rto_deadline: None,
            rto_token: 0,
            srtt: None,
            rttvar: SimTime::ZERO,
            inflight: BTreeMap::new(),
            recover: 0,
            in_fast_recovery: false,
            rcv_nxt: 0,
            out_of_order: BTreeMap::new(),
            sender_state,
            receiver_state,
            syn_sent_at: None,
            syn_retries: 0,
            start_at: None,
            done_at: None,
        }
    }

    pub fn bytes_acked(&self) -> u64 {
        self.last_acked.min(self.total_bytes)
    }

    pub fn is_done(&self) -> bool {
        self.done_at.is_some()
    }

    pub fn start_time(&self) -> Option<SimTime> {
        self.start_at
    }

    pub fn done_time(&self) -> Option<SimTime> {
        self.done_at
    }

    fn earliest_unacked_seq(&self) -> Option<u64> {
        self.inflight.keys().next().copied()
    }

    fn make_data_packet(&self, net: &mut Network) -> crate::net::Packet {
        match self.routing_mode {
            TcpRoutingMode::Preset => net.make_packet(self.id, self.cfg.mss, self.fwd_route.clone()),
            TcpRoutingMode::Dynamic => net.make_packet_dynamic(self.id, self.cfg.mss, self.src, self.dst),
        }
    }

    fn make_ack_packet(&self, net: &mut Network) -> crate::net::Packet {
        match self.routing_mode {
            TcpRoutingMode::Preset => net.make_packet(self.id, self.cfg.ack_bytes, self.rev_route.clone()),
            TcpRoutingMode::Dynamic => net.make_packet_dynamic(self.id, self.cfg.ack_bytes, self.dst, self.src),
        }
    }

    fn inflight_bytes(&self) -> u64 {
        self.inflight.values().map(|s| s.len as u64).sum()
    }

    fn effective_cwnd(&self) -> u64 {
        let mut cwnd = self.cwnd_bytes;
        let Some(pps) = self.cfg.app_limited_pps else {
            return cwnd;
        };
        let Some(srtt) = self.srtt else {
            return cwnd;
        };
        let rtt_ns = srtt.0.max(1);
        let pkts = pps.saturating_mul(rtt_ns) / 1_000_000_000;
        let limit = pkts.saturating_mul(self.cfg.mss as u64);
        if limit > 0 {
            cwnd = cwnd.min(limit);
        }
        cwnd
    }

    fn update_rto_with_sample(&mut self, sample: SimTime) {
        if let Some(srtt) = self.srtt {
            let diff = if sample.0 >= srtt.0 {
                sample.0 - srtt.0
            } else {
                srtt.0 - sample.0
            };
            let rttvar = (self.rttvar.0 * 3 / 4).saturating_add(diff / 4);
            let srtt = (srtt.0 * 7 / 8).saturating_add(sample.0 / 8);
            self.srtt = Some(SimTime(srtt));
            self.rttvar = SimTime(rttvar);
        } else {
            self.srtt = Some(sample);
            self.rttvar = SimTime(sample.0 / 2);
        }
        let srtt = self.srtt.unwrap();
        let mut rto = srtt.0.saturating_add(self.rttvar.0.saturating_mul(4));
        rto = rto.max(self.cfg.min_rto.0).min(self.cfg.max_rto.0);
        self.rto = SimTime(rto);
    }

    fn schedule_rto(&mut self, sim: &mut Simulator) {
        let deadline = SimTime(sim.now().0.saturating_add(self.rto.0));
        self.rto_deadline = Some(deadline);
        self.rto_token = self.rto_token.wrapping_add(1);
        let token = self.rto_token;
        sim.schedule(deadline, TcpRto { conn_id: self.id, token });
    }

    fn ensure_rto(&mut self, sim: &mut Simulator) {
        if self.rto_deadline.is_some() {
            return;
        }
        if self.syn_sent_at.is_some() || !self.inflight.is_empty() {
            self.schedule_rto(sim);
        }
    }

    fn restart_rto(&mut self, sim: &mut Simulator) {
        if self.syn_sent_at.is_some() || !self.inflight.is_empty() {
            self.schedule_rto(sim);
        } else {
            self.rto_deadline = None;
        }
    }

    fn stop_rto(&mut self) {
        self.rto_deadline = None;
    }

    fn recv_data(&mut self, seq: u64, len: u32) -> u64 {
        if seq == self.rcv_nxt {
            self.rcv_nxt = self.rcv_nxt.saturating_add(len as u64);
            while let Some(next_len) = self.out_of_order.remove(&self.rcv_nxt) {
                self.rcv_nxt = self.rcv_nxt.saturating_add(next_len as u64);
            }
        } else if seq > self.rcv_nxt {
            self.out_of_order.entry(seq).or_insert(len);
        }
        self.rcv_nxt
    }
}

pub type TcpDoneCallback = Box<dyn Fn(TcpConnId, SimTime, &mut Simulator) + Send>;

#[derive(Default)]
pub struct TcpStack {
    conns: HashMap<TcpConnId, TcpConn>,
    done_callbacks: HashMap<TcpConnId, TcpDoneCallback>,
}

impl fmt::Debug for TcpStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpStack")
            .field("conns", &self.conns.len())
            .field("done_callbacks", &self.done_callbacks.len())
            .finish()
    }
}

impl TcpStack {
    pub fn insert(&mut self, conn: TcpConn) {
        self.conns.insert(conn.id, conn);
    }

    pub fn set_done_callback(&mut self, id: TcpConnId, cb: TcpDoneCallback) {
        self.done_callbacks.insert(id, cb);
    }

    pub fn get(&self, id: TcpConnId) -> Option<&TcpConn> {
        self.conns.get(&id)
    }

    pub fn get_mut(&mut self, id: TcpConnId) -> Option<&mut TcpConn> {
        self.conns.get_mut(&id)
    }

    pub fn start_conn(&mut self, conn: TcpConn, sim: &mut Simulator, net: &mut Network) {
        let id = conn.id;
        self.insert(conn);
        self.send_data_if_possible(id, sim, net);
    }

    pub(crate) fn send_data_if_possible(&mut self, id: TcpConnId, sim: &mut Simulator, net: &mut Network) {
        let Some(conn) = self.conns.get_mut(&id) else {
            return;
        };
        if conn.done_at.is_some() {
            return;
        }

        if conn.sender_state != SenderState::Established {
            if conn.syn_sent_at.is_none() {
                let mut pkt = conn.make_data_packet(net);
                pkt.size_bytes = conn.cfg.ack_bytes;
                pkt.transport = Transport::Tcp(TcpSegment::Syn);
                conn.syn_sent_at = Some(sim.now());
                conn.syn_retries = conn.syn_retries.saturating_add(1);
                if conn.start_at.is_none() {
                    conn.start_at = Some(sim.now());
                }
                net.forward_from(conn.src, pkt, sim);
            }
            conn.ensure_rto(sim);
            return;
        }

        if conn.start_at.is_none() {
            conn.start_at = Some(sim.now());
        }

        // 发送窗口：inflight bytes < cwnd
        let inflight_bytes = conn.inflight_bytes();
        let mut avail = conn.effective_cwnd().saturating_sub(inflight_bytes);

        while avail > 0 && conn.next_seq < conn.total_bytes {
            let remain = conn.total_bytes - conn.next_seq;
            let len = (conn.cfg.mss as u64).min(remain).min(avail) as u32;
            if len == 0 {
                break;
            }
            let seq = conn.next_seq;
            conn.next_seq = conn.next_seq.saturating_add(len as u64);
            avail = avail.saturating_sub(len as u64);

            // 构造 data 包
            let mut pkt = conn.make_data_packet(net);
            pkt.size_bytes = conn.cfg.mss; // 包大小按 mss 计（简化）
            pkt.transport = Transport::Tcp(TcpSegment::Data { seq, len });

            net.viz_tcp_send_data(sim.now().0, conn.id, seq, len);

            conn.inflight.insert(
                seq,
                SentSeg {
                    len,
                    sent_at: sim.now(),
                    retransmitted: false,
                },
            );

            net.forward_from(conn.src, pkt, sim);
        }
        conn.ensure_rto(sim);
    }

    fn send_ack(&mut self, id: TcpConnId, ack: u64, sim: &mut Simulator, net: &mut Network) {
        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let mut pkt = conn.make_ack_packet(net);
        pkt.size_bytes = conn.cfg.ack_bytes;
        pkt.transport = Transport::Tcp(TcpSegment::Ack { ack });
        net.viz_tcp_send_ack(sim.now().0, conn.id, ack);
        net.forward_from(conn.dst, pkt, sim);
    }

    pub fn on_tcp_segment(
        &mut self,
        conn_id: TcpConnId,
        at: NodeId,
        seg: TcpSegment,
        sim: &mut Simulator,
        net: &mut Network,
    ) {
        match seg {
            TcpSegment::Syn => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.dst {
                    return;
                }
                if conn.receiver_state == ReceiverState::Idle {
                    conn.receiver_state = ReceiverState::SynReceived;
                }
                let mut pkt = conn.make_ack_packet(net);
                pkt.size_bytes = conn.cfg.ack_bytes;
                pkt.transport = Transport::Tcp(TcpSegment::SynAck);
                net.forward_from(conn.dst, pkt, sim);
            }
            TcpSegment::SynAck => {
                let start_data = {
                    let Some(conn) = self.conns.get_mut(&conn_id) else {
                        return;
                    };
                    if at != conn.src {
                        return;
                    }
                    conn.sender_state = SenderState::Established;
                    conn.syn_sent_at = None;
                    conn.syn_retries = 0;
                    conn.stop_rto();
                    if conn.cfg.handshake {
                        let mut pkt = conn.make_data_packet(net);
                        pkt.size_bytes = conn.cfg.ack_bytes;
                        pkt.transport = Transport::Tcp(TcpSegment::HandshakeAck);
                        net.forward_from(conn.src, pkt, sim);
                    }
                    true
                };
                if start_data {
                    self.send_data_if_possible(conn_id, sim, net);
                }
            }
            TcpSegment::HandshakeAck => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.dst {
                    return;
                }
                conn.receiver_state = ReceiverState::Established;
            }
            TcpSegment::Data { seq, len } => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.dst {
                    // 不是目的 host：忽略（理论上不会发生，因为只在 delivered 回调中调用）
                    return;
                }
                if conn.cfg.handshake && conn.receiver_state == ReceiverState::Idle {
                    return;
                }
                let ack = conn.recv_data(seq, len);
                let _ = conn;
                // 无论是否乱序，都发累计 ACK（dupACK 体现为 ack 不前进）
                self.send_ack(conn_id, ack, sim, net);
            }
            TcpSegment::Ack { ack } => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.src {
                    return;
                }

                // 记录“收到 ACK”这一事实（无论新 ACK 或 dupACK）
                net.viz_tcp_recv_ack(sim.now().0, conn.id, ack);

                if ack > conn.last_acked {
                    let now = sim.now();
                    let mut rtt_sample = None;
                    for (&s, sent) in conn.inflight.iter() {
                        let end = s.saturating_add(sent.len as u64);
                        if end <= ack {
                            if !sent.retransmitted {
                                let delta = now.0.saturating_sub(sent.sent_at.0);
                                rtt_sample = Some(SimTime(delta));
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(sample) = rtt_sample {
                        conn.update_rto_with_sample(sample);
                    }

                    conn.dup_acks = 0;
                    let newly_acked = ack - conn.last_acked;

                    let mut to_remove = Vec::new();
                    for (&s, sent) in conn.inflight.iter() {
                        let end = s.saturating_add(sent.len as u64);
                        if end <= ack {
                            to_remove.push(s);
                        } else {
                            break;
                        }
                    }
                    for s in to_remove {
                        conn.inflight.remove(&s);
                    }

                    let prev_acked = conn.last_acked;
                    conn.last_acked = ack;

                    let mss = conn.cfg.mss as u64;
                    if conn.in_fast_recovery {
                        if ack >= conn.recover {
                            let flightsize = conn.next_seq.saturating_sub(ack);
                            conn.cwnd_bytes = conn
                                .ssthresh_bytes
                                .min(flightsize.saturating_add(mss));
                            conn.in_fast_recovery = false;
                        } else {
                            let new_data = ack.saturating_sub(prev_acked);
                            if new_data < conn.cwnd_bytes {
                                conn.cwnd_bytes = conn.cwnd_bytes.saturating_sub(new_data);
                            } else {
                                conn.cwnd_bytes = 0;
                            }
                            conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(mss);
                            if let Some(seq0) = conn.earliest_unacked_seq() {
                                let len = conn.inflight.get(&seq0).map(|s| s.len).unwrap_or(conn.cfg.mss);
                                let mut pkt = conn.make_data_packet(net);
                                pkt.size_bytes = conn.cfg.mss;
                                pkt.transport = Transport::Tcp(TcpSegment::Data { seq: seq0, len });
                                net.forward_from(conn.src, pkt, sim);
                                if let Some(sent) = conn.inflight.get_mut(&seq0) {
                                    sent.sent_at = sim.now();
                                    sent.retransmitted = true;
                                }
                            }
                        }
                    } else {
                        // 拥塞控制：慢启动 / 拥塞避免（极简）
                        if conn.cwnd_bytes < conn.ssthresh_bytes {
                            conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(newly_acked);
                        } else {
                            // AIMD：每个 ACK 让 cwnd 以 mss^2/cwnd 增长（至少 +1）
                            let inc = (mss.saturating_mul(mss) / conn.cwnd_bytes).max(1);
                            conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(inc);
                        }
                    }

                    // 移除已确认段
                    // 完成判定：所有数据都被累计确认
                    if conn.last_acked >= conn.total_bytes && conn.done_at.is_none() {
                        conn.done_at = Some(sim.now());
                        conn.stop_rto();
                        let done_cb = self.done_callbacks.remove(&conn_id);
                        if let Some(cb) = done_cb {
                            cb(conn_id, sim.now(), sim);
                        }
                        return;
                    }
                    conn.restart_rto(sim);

                    // 继续发送
                    let id = conn.id;
                    let _ = conn;
                    self.send_data_if_possible(id, sim, net);
                } else if ack == conn.last_acked {
                    // dupACK
                    if conn.in_fast_recovery {
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(conn.cfg.mss as u64);
                        let id = conn.id;
                        let _ = conn;
                        self.send_data_if_possible(id, sim, net);
                        return;
                    }

                    conn.dup_acks = conn.dup_acks.saturating_add(1);
                    let dup = conn.dup_acks;
                    let mss = conn.cfg.mss as u64;
                    if dup == 3 {
                        if conn.last_acked < conn.recover {
                            return;
                        }
                        conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
                        if let Some(seq0) = conn.earliest_unacked_seq() {
                            let len = conn.inflight.get(&seq0).map(|s| s.len).unwrap_or(conn.cfg.mss);
                            let mut pkt = conn.make_data_packet(net);
                            pkt.size_bytes = conn.cfg.mss;
                            pkt.transport = Transport::Tcp(TcpSegment::Data { seq: seq0, len });
                            net.forward_from(conn.src, pkt, sim);
                            if let Some(sent) = conn.inflight.get_mut(&seq0) {
                                sent.sent_at = sim.now();
                                sent.retransmitted = true;
                            }
                        }
                        conn.cwnd_bytes = conn.ssthresh_bytes.saturating_add(3 * mss);
                        conn.in_fast_recovery = true;
                        conn.recover = conn.next_seq;
                        let id = conn.id;
                        let _ = conn;
                        self.send_data_if_possible(id, sim, net);
                    } else if dup > 3 {
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(mss);
                        let id = conn.id;
                        let _ = conn;
                        self.send_data_if_possible(id, sim, net);
                    }
                }
            }
        }
    }
}

/// 启动一个 TCP 流（连接已建立假设）
#[derive(Debug)]
pub struct TcpStart {
    pub conn: TcpConn,
}

impl Event for TcpStart {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let TcpStart { conn } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let id = conn.id;
        // 规避同时借用 `w.net` 与 `w.net.tcp`
        let mut tcp = std::mem::take(&mut w.net.tcp);
        tcp.insert(conn);
        tcp.send_data_if_possible(id, sim, &mut w.net);
        w.net.tcp = tcp;
    }
}

/// TCP RTO 事件：若该 seq 仍是最早未确认段，则触发超时重传
#[derive(Debug)]
pub struct TcpRto {
    pub conn_id: TcpConnId,
    pub token: u64,
}

impl Event for TcpRto {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let TcpRto { conn_id, token } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        // 规避同时借用 `w.net` 与 `w.net.tcp`
        let mut tcp = std::mem::take(&mut w.net.tcp);
        let Some(conn) = tcp.get_mut(conn_id) else {
            w.net.tcp = tcp;
            return;
        };
        if conn.done_at.is_some() {
            w.net.tcp = tcp;
            return;
        }
        if conn.rto_deadline.is_none() || conn.rto_token != token {
            w.net.tcp = tcp;
            return;
        }
        let deadline = conn.rto_deadline.unwrap();
        if sim.now() < deadline {
            w.net.tcp = tcp;
            return;
        }
        conn.rto_deadline = None;

        if conn.sender_state != SenderState::Established {
            // SYN 超时重传
            if conn.syn_sent_at.is_some() {
                let rto = conn.rto.0.saturating_mul(2);
                let rto = rto.max(conn.cfg.min_rto.0).min(conn.cfg.max_rto.0);
                conn.rto = SimTime(rto);
                let mut pkt = conn.make_data_packet(&mut w.net);
                pkt.size_bytes = conn.cfg.ack_bytes;
                pkt.transport = Transport::Tcp(TcpSegment::Syn);
                conn.syn_sent_at = Some(sim.now());
                conn.syn_retries = conn.syn_retries.saturating_add(1);
                w.net.forward_from(conn.src, pkt, sim);
                conn.schedule_rto(sim);
            }
            w.net.tcp = tcp;
            return;
        }

        let Some(seq0) = conn.earliest_unacked_seq() else {
            w.net.tcp = tcp;
            return;
        };
        let Some(sent) = conn.inflight.get(&seq0).cloned() else {
            w.net.tcp = tcp;
            return;
        };

        // 先记录 RTO 事件（即将触发重传）
        w.net.viz_tcp_rto(sim.now().0, conn_id, seq0);

        if conn.in_fast_recovery {
            let flightsize = conn.next_seq.saturating_sub(conn.last_acked);
            conn.cwnd_bytes = conn
                .ssthresh_bytes
                .min(flightsize.saturating_add(conn.cfg.mss as u64));
        }

        // 超时：回到慢启动
        let mss = conn.cfg.mss as u64;
        conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
        conn.cwnd_bytes = mss;
        conn.dup_acks = 0;
        conn.in_fast_recovery = false;
        conn.recover = conn.next_seq;
        let rto = conn.rto.0.saturating_mul(2);
        let rto = rto.max(conn.cfg.min_rto.0).min(conn.cfg.max_rto.0);
        conn.rto = SimTime(rto);

        // 重传 earliest unacked
        let mut pkt = conn.make_data_packet(&mut w.net);
        pkt.size_bytes = conn.cfg.mss;
        pkt.transport = Transport::Tcp(TcpSegment::Data { seq: seq0, len: sent.len });
        w.net.forward_from(conn.src, pkt, sim);
        if let Some(sent) = conn.inflight.get_mut(&seq0) {
            sent.sent_at = sim.now();
            sent.retransmitted = true;
        }

        conn.schedule_rto(sim);

        w.net.tcp = tcp;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::NetWorld;
    use crate::sim::{SimTime, Simulator};

    #[test]
    fn recv_data_reorders_and_advances_ack() {
        let mut cfg = TcpConfig::default();
        cfg.handshake = false;
        let mut conn = TcpConn::new(1, NodeId(0), NodeId(1), vec![NodeId(0), NodeId(1)], 3000, cfg);

        let ack0 = conn.recv_data(0, 1000);
        assert_eq!(ack0, 1000);

        let ack1 = conn.recv_data(2000, 1000);
        assert_eq!(ack1, 1000);

        let ack2 = conn.recv_data(1000, 1000);
        assert_eq!(ack2, 3000);
    }

    #[test]
    fn rto_estimator_updates_on_samples() {
        let mut cfg = TcpConfig::default();
        cfg.min_rto = SimTime::ZERO;
        cfg.max_rto = SimTime::from_secs(10);
        let mut conn = TcpConn::new(1, NodeId(0), NodeId(1), vec![NodeId(0), NodeId(1)], 1000, cfg);

        conn.update_rto_with_sample(SimTime(1_000));
        assert_eq!(conn.srtt.unwrap().0, 1_000);
        assert_eq!(conn.rttvar.0, 500);
        assert_eq!(conn.rto.0, 3_000);

        conn.update_rto_with_sample(SimTime(1_000));
        assert_eq!(conn.srtt.unwrap().0, 1_000);
        assert_eq!(conn.rttvar.0, 375);
        assert_eq!(conn.rto.0, 2_500);
    }

    #[test]
    fn handshake_establishes_sender_state() {
        let mut sim = Simulator::default();
        let mut world = NetWorld::default();
        let h0 = world.net.add_host("h0".to_string());
        let h1 = world.net.add_host("h1".to_string());
        let bw = 1_000_000_000;
        let latency = SimTime::from_micros(1);
        world.net.connect(h0, h1, latency, bw);
        world.net.connect(h1, h0, latency, bw);

        let mut cfg = TcpConfig::default();
        cfg.handshake = true;
        cfg.min_rto = SimTime::from_micros(1);
        cfg.max_rto = SimTime::from_millis(10);

        let route = vec![h0, h1];
        let conn = TcpConn::new(1, h0, h1, route, 1000, cfg);
        sim.schedule(SimTime::ZERO, TcpStart { conn });
        sim.run_until(SimTime::from_micros(10), &mut world);

        let c = world.net.tcp.get(1).expect("tcp conn exists");
        assert_eq!(c.sender_state, SenderState::Established);
        assert!(c.syn_sent_at.is_none());
    }
}
