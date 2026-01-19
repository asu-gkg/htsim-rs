//! DCTCP（简化版）协议实现
//!
//! 目标：支持一个 dumbbell DCTCP 实验所需的最小功能：
//! - 数据段/ACK 段
//! - ECN 标记反馈（ACK 回显）
//! - DCTCP alpha 更新与窗口缩减
//! - 超时重传（固定/指数退避的 RTO）
//!
//! 注意：这是仿真用途的“极简 DCTCP”，不实现握手/窗口通告/选择确认等。

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::net::{Ecn, NetWorld, Network, NodeId};
use crate::proto::{DctcpSegment, Transport};
use crate::sim::{Event, SimTime, Simulator, World};

/// 一个 DCTCP 连接的唯一标识（复用 `flow_id` 的语义）。
pub type DctcpConnId = u64;
pub type DctcpDoneCallback = Box<dyn Fn(DctcpConnId, SimTime, &mut Simulator) + Send>;

#[derive(Debug, Clone)]
pub struct DctcpConfig {
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
    /// 最大 RTO（用于退避上限）
    pub max_rto: SimTime,
    /// DCTCP alpha 更新的增益 g（典型为 1/16）
    pub g: f64,
}

impl Default for DctcpConfig {
    fn default() -> Self {
        let mss = 1460;
        Self {
            mss,
            ack_bytes: 64,
            init_cwnd_bytes: (mss as u64).saturating_mul(10),
            init_ssthresh_bytes: (mss as u64).saturating_mul(1_000),
            init_rto: SimTime::from_micros(200),
            max_rto: SimTime::from_millis(200),
            g: 1.0 / 16.0,
        }
    }
}

#[derive(Debug, Clone)]
struct SentSeg {
    len: u32,
}

#[derive(Debug, Clone)]
pub struct DctcpConn {
    pub id: DctcpConnId,
    pub src: NodeId,
    pub dst: NodeId,
    pub fwd_route: Vec<NodeId>,
    pub rev_route: Vec<NodeId>,
    pub total_bytes: u64,
    pub cfg: DctcpConfig,

    // sender
    next_seq: u64,
    last_acked: u64,
    cwnd_bytes: u64,
    ssthresh_bytes: u64,
    dup_acks: u32,
    rto: SimTime,
    inflight: BTreeMap<u64, SentSeg>, // seq -> segment

    // DCTCP alpha
    alpha: f64,
    window_end: u64,
    acked_in_window: u64,
    marked_in_window: u64,
    cwnd_log: Option<Vec<CwndSample>>,

    // receiver
    rcv_nxt: u64,

    // stats
    start_at: Option<SimTime>,
    done_at: Option<SimTime>,
}

impl DctcpConn {
    pub fn new(
        id: DctcpConnId,
        src: NodeId,
        dst: NodeId,
        fwd_route: Vec<NodeId>,
        total_bytes: u64,
        cfg: DctcpConfig,
    ) -> Self {
        let mut rev_route = fwd_route.clone();
        rev_route.reverse();
        let init_rto = cfg.init_rto;
        let cwnd = cfg.init_cwnd_bytes.max(cfg.mss as u64);
        let ssthresh = cfg.init_ssthresh_bytes.max(cfg.mss as u64);
        let window_end = cwnd;
        Self {
            id,
            src,
            dst,
            fwd_route,
            rev_route,
            total_bytes,
            cfg,
            next_seq: 0,
            last_acked: 0,
            cwnd_bytes: cwnd,
            ssthresh_bytes: ssthresh,
            dup_acks: 0,
            rto: init_rto,
            inflight: BTreeMap::new(),
            alpha: 0.0,
            window_end,
            acked_in_window: 0,
            marked_in_window: 0,
            cwnd_log: None,
            rcv_nxt: 0,
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

    pub fn enable_cwnd_log(&mut self) {
        self.cwnd_log = Some(Vec::new());
    }

    pub fn cwnd_samples(&self) -> Option<&[CwndSample]> {
        self.cwnd_log.as_deref()
    }

    fn earliest_unacked_seq(&self) -> Option<u64> {
        self.inflight.keys().next().copied()
    }

    fn inflight_bytes(&self) -> u64 {
        self.inflight.values().map(|s| s.len as u64).sum()
    }

    pub(crate) fn record_cwnd(&mut self, now: SimTime) {
        let Some(log) = &mut self.cwnd_log else {
            return;
        };
        log.push(CwndSample {
            t_ns: now.0,
            cwnd_bytes: self.cwnd_bytes,
            ssthresh_bytes: self.ssthresh_bytes,
            alpha: self.alpha,
            acked_bytes: self.last_acked,
        });
    }
}

/// DCTCP 拥塞窗口采样（用于离线绘图）
#[derive(Debug, Clone)]
pub struct CwndSample {
    pub t_ns: u64,
    pub cwnd_bytes: u64,
    pub ssthresh_bytes: u64,
    pub alpha: f64,
    pub acked_bytes: u64,
}

#[derive(Default)]
pub struct DctcpStack {
    conns: HashMap<DctcpConnId, DctcpConn>,
    done_callbacks: HashMap<DctcpConnId, DctcpDoneCallback>,
}

impl fmt::Debug for DctcpStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DctcpStack")
            .field("conns", &self.conns)
            .field("done_callbacks", &self.done_callbacks.len())
            .finish()
    }
}

impl DctcpStack {
    pub fn insert(&mut self, conn: DctcpConn) {
        self.conns.insert(conn.id, conn);
    }

    pub fn set_done_callback(&mut self, id: DctcpConnId, cb: DctcpDoneCallback) {
        self.done_callbacks.insert(id, cb);
    }

    /// Insert a connection, record initial cwnd sample, and start sending.
    pub fn start_conn(&mut self, conn: DctcpConn, sim: &mut Simulator, net: &mut Network) {
        let id = conn.id;
        self.insert(conn);
        if let Some(c) = self.get_mut(id) {
            let now = sim.now();
            c.record_cwnd(now);
            net.viz_dctcp_cwnd(
                now.0,
                c.id,
                c.cwnd_bytes,
                c.ssthresh_bytes,
                c.inflight_bytes(),
                c.alpha,
            );
        }
        self.send_data_if_possible(id, sim, net);
    }

    pub fn get(&self, id: DctcpConnId) -> Option<&DctcpConn> {
        self.conns.get(&id)
    }

    pub fn get_mut(&mut self, id: DctcpConnId) -> Option<&mut DctcpConn> {
        self.conns.get_mut(&id)
    }

    pub(crate) fn send_data_if_possible(
        &mut self,
        id: DctcpConnId,
        sim: &mut Simulator,
        net: &mut Network,
    ) {
        let Some(conn) = self.conns.get_mut(&id) else {
            return;
        };
        if conn.done_at.is_some() {
            return;
        }

        if conn.start_at.is_none() {
            conn.start_at = Some(sim.now());
        }

        let inflight_bytes: u64 = conn
            .inflight
            .values()
            .map(|s| s.len as u64)
            .sum();
        let mut avail = conn.cwnd_bytes.saturating_sub(inflight_bytes);

        while avail > 0 && conn.next_seq < conn.total_bytes {
            let remain = conn.total_bytes - conn.next_seq;
            let len = (conn.cfg.mss as u64).min(remain).min(avail) as u32;
            if len == 0 {
                break;
            }
            let seq = conn.next_seq;
            conn.next_seq = conn.next_seq.saturating_add(len as u64);
            avail = avail.saturating_sub(len as u64);

            let mut pkt = net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
            pkt.size_bytes = conn.cfg.mss;
            pkt.transport = Transport::Dctcp(DctcpSegment::Data { seq, len });
            pkt.ecn = Ecn::Ect0;

            net.viz_tcp_send_data(sim.now().0, conn.id, seq, len);

            conn.inflight.insert(seq, SentSeg { len });

            if conn.earliest_unacked_seq() == Some(seq) {
                sim.schedule(
                    SimTime(sim.now().0.saturating_add(conn.rto.0)),
                    DctcpRto {
                        conn_id: conn.id,
                        seq,
                    },
                );
            }

            net.forward_from(conn.src, pkt, sim);
        }
    }

    fn send_ack(
        &mut self,
        id: DctcpConnId,
        ack: u64,
        ecn_echo: bool,
        sim: &mut Simulator,
        net: &mut Network,
    ) {
        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let mut pkt = net.make_packet(conn.id, conn.cfg.ack_bytes, conn.rev_route.clone());
        pkt.size_bytes = conn.cfg.ack_bytes;
        pkt.transport = Transport::Dctcp(DctcpSegment::Ack { ack, ecn_echo });

        net.viz_tcp_send_ack(sim.now().0, conn.id, ack);
        net.forward_from(conn.dst, pkt, sim);
    }

    pub fn on_dctcp_segment(
        &mut self,
        conn_id: DctcpConnId,
        at: NodeId,
        seg: DctcpSegment,
        ecn: Ecn,
        sim: &mut Simulator,
        net: &mut Network,
    ) {
        match seg {
            DctcpSegment::Data { seq, len } => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.dst {
                    return;
                }

                if seq == conn.rcv_nxt {
                    conn.rcv_nxt = conn.rcv_nxt.saturating_add(len as u64);
                }
                let ack = conn.rcv_nxt;
                let ecn_echo = ecn.is_ce();
                let _ = conn;
                self.send_ack(conn_id, ack, ecn_echo, sim, net);
            }
            DctcpSegment::Ack { ack, ecn_echo } => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.src {
                    return;
                }

                net.viz_tcp_recv_ack(sim.now().0, conn.id, ack);

                if ack > conn.last_acked {
                    conn.dup_acks = 0;
                    let newly_acked = ack - conn.last_acked;
                    conn.last_acked = ack;

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

                    // DCTCP：按窗口统计 ECN 标记比例
                    conn.acked_in_window = conn.acked_in_window.saturating_add(newly_acked);
                    if ecn_echo {
                        conn.marked_in_window = conn.marked_in_window.saturating_add(newly_acked);
                    }

                    if conn.last_acked >= conn.window_end {
                        let frac = if conn.acked_in_window == 0 {
                            0.0
                        } else {
                            conn.marked_in_window as f64 / conn.acked_in_window as f64
                        };
                        conn.alpha = (1.0 - conn.cfg.g) * conn.alpha + conn.cfg.g * frac;
                        if conn.marked_in_window > 0 {
                            let factor = 1.0 - conn.alpha / 2.0;
                            let new_cwnd = (conn.cwnd_bytes as f64 * factor)
                                .max(conn.cfg.mss as f64)
                                .floor() as u64;
                            conn.cwnd_bytes = new_cwnd.max(conn.cfg.mss as u64);
                        }
                        conn.acked_in_window = 0;
                        conn.marked_in_window = 0;
                        conn.window_end = conn.last_acked.saturating_add(conn.cwnd_bytes);
                    }

                    // 拥塞控制：慢启动 / 拥塞避免（极简）
                    if conn.cwnd_bytes < conn.ssthresh_bytes {
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(newly_acked);
                    } else {
                        let mss = conn.cfg.mss as u64;
                        let inc = (mss.saturating_mul(mss) / conn.cwnd_bytes).max(1);
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(inc);
                    }

                    let now = sim.now();
                    conn.record_cwnd(now);
                    net.viz_dctcp_cwnd(
                        now.0,
                        conn.id,
                        conn.cwnd_bytes,
                        conn.ssthresh_bytes,
                        conn.inflight_bytes(),
                        conn.alpha,
                    );

                    let done = conn.last_acked >= conn.total_bytes && conn.done_at.is_none();
                    if done {
                        conn.done_at = Some(sim.now());
                        let done_cb = self.done_callbacks.remove(&conn_id);
                        if let Some(cb) = done_cb {
                            cb(conn_id, sim.now(), sim);
                        }
                        return;
                    }

                    let id = conn.id;
                    let _ = conn;
                    self.send_data_if_possible(id, sim, net);
                } else if ack == conn.last_acked {
                    conn.dup_acks = conn.dup_acks.saturating_add(1);
                    let dup = conn.dup_acks;
                    let mss = conn.cfg.mss as u64;
                    if dup == 3 {
                        if let Some(seq0) = conn.earliest_unacked_seq() {
                            conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
                            conn.cwnd_bytes = conn.ssthresh_bytes.saturating_add(3 * mss);
                            let len = conn
                                .inflight
                                .get(&seq0)
                                .map(|s| s.len)
                                .unwrap_or(conn.cfg.mss);
                            let mut pkt = net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
                            pkt.size_bytes = conn.cfg.mss;
                            pkt.transport = Transport::Dctcp(DctcpSegment::Data { seq: seq0, len });
                            pkt.ecn = Ecn::Ect0;
                            net.forward_from(conn.src, pkt, sim);
                        }
                    } else if dup > 3 {
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(mss);
                        let id = conn.id;
                        let _ = conn;
                        let now = sim.now();
                        conn.record_cwnd(now);
                        net.viz_dctcp_cwnd(
                            now.0,
                            conn.id,
                            conn.cwnd_bytes,
                            conn.ssthresh_bytes,
                            conn.inflight_bytes(),
                            conn.alpha,
                        );
                        self.send_data_if_possible(id, sim, net);
                    }
                }
            }
        }
    }
}

/// 启动一个 DCTCP 流（连接已建立假设）
#[derive(Debug)]
pub struct DctcpStart {
    pub conn: DctcpConn,
}

impl Event for DctcpStart {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let DctcpStart { conn } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        let id = conn.id;
        let mut dctcp = std::mem::take(&mut w.net.dctcp);
        dctcp.insert(conn);
        if let Some(c) = dctcp.get_mut(id) {
            let now = sim.now();
            c.record_cwnd(now);
            w.net.viz_dctcp_cwnd(
                now.0,
                c.id,
                c.cwnd_bytes,
                c.ssthresh_bytes,
                c.inflight_bytes(),
                c.alpha,
            );
        }
        dctcp.send_data_if_possible(id, sim, &mut w.net);
        w.net.dctcp = dctcp;
    }
}

/// DCTCP RTO 事件：若该 seq 仍是最早未确认段，则触发超时重传
#[derive(Debug)]
pub struct DctcpRto {
    pub conn_id: DctcpConnId,
    pub seq: u64,
}

impl Event for DctcpRto {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let DctcpRto { conn_id, seq } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        w.net.viz_tcp_rto(sim.now().0, conn_id, seq);

        let mut dctcp = std::mem::take(&mut w.net.dctcp);
        let Some(conn) = dctcp.get_mut(conn_id) else {
            w.net.dctcp = dctcp;
            return;
        };
        if conn.done_at.is_some() {
            w.net.dctcp = dctcp;
            return;
        }

        if conn.earliest_unacked_seq() != Some(seq) {
            w.net.dctcp = dctcp;
            return;
        }
        let Some(sent) = conn.inflight.get(&seq).cloned() else {
            w.net.dctcp = dctcp;
            return;
        };

        let mss = conn.cfg.mss as u64;
        conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
        conn.cwnd_bytes = mss;
        conn.dup_acks = 0;
        conn.rto = SimTime((conn.rto.0.saturating_mul(2)).min(conn.cfg.max_rto.0));
        let now = sim.now();
        conn.record_cwnd(now);
        w.net.viz_dctcp_cwnd(
            now.0,
            conn.id,
            conn.cwnd_bytes,
            conn.ssthresh_bytes,
            conn.inflight_bytes(),
            conn.alpha,
        );

        let mut pkt = w.net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
        pkt.size_bytes = conn.cfg.mss;
        pkt.transport = Transport::Dctcp(DctcpSegment::Data { seq, len: sent.len });
        pkt.ecn = Ecn::Ect0;
        w.net.forward_from(conn.src, pkt, sim);

        sim.schedule(
            SimTime(sim.now().0.saturating_add(conn.rto.0)),
            DctcpRto { conn_id, seq },
        );

        w.net.dctcp = dctcp;
    }
}
