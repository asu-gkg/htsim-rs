//! TCP（简化版）协议实现
//!
//! 目标：支持一个 dumbbell TCP 实验所需的最小功能：
//! - 数据段/ACK 段
//! - Reno 风格的拥塞控制（慢启动 + AIMD，含 3 dupACK 快速重传）
//! - 超时重传（固定/指数退避的 RTO）
//!
//! 注意：这是仿真用途的“极简 TCP”，不实现握手/窗口通告/选择确认等。

use std::collections::{BTreeMap, HashMap};

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
    /// 最大 RTO（用于退避上限）
    pub max_rto: SimTime,
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
            max_rto: SimTime::from_millis(200),
        }
    }
}

#[derive(Debug, Clone)]
struct SentSeg {
    len: u32,
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

    // sender
    next_seq: u64,
    last_acked: u64,
    cwnd_bytes: u64,
    ssthresh_bytes: u64,
    dup_acks: u32,
    rto: SimTime,
    inflight: BTreeMap<u64, SentSeg>, // seq -> segment

    // receiver
    rcv_nxt: u64,

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

    fn earliest_unacked_seq(&self) -> Option<u64> {
        self.inflight.keys().next().copied()
    }
}

#[derive(Debug, Default)]
pub struct TcpStack {
    conns: HashMap<TcpConnId, TcpConn>,
}

impl TcpStack {
    pub fn insert(&mut self, conn: TcpConn) {
        self.conns.insert(conn.id, conn);
    }

    pub fn get(&self, id: TcpConnId) -> Option<&TcpConn> {
        self.conns.get(&id)
    }

    pub fn get_mut(&mut self, id: TcpConnId) -> Option<&mut TcpConn> {
        self.conns.get_mut(&id)
    }

    pub(crate) fn send_data_if_possible(&mut self, id: TcpConnId, sim: &mut Simulator, net: &mut Network) {
        let Some(conn) = self.conns.get_mut(&id) else {
            return;
        };
        if conn.done_at.is_some() {
            return;
        }

        if conn.start_at.is_none() {
            conn.start_at = Some(sim.now());
        }

        // 发送窗口：inflight bytes < cwnd
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

            // 构造 data 包
            let mut pkt = net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
            pkt.size_bytes = conn.cfg.mss; // 包大小按 mss 计（简化）
            pkt.transport = Transport::Tcp(TcpSegment::Data { seq, len });

            net.viz_tcp_send_data(sim.now().0, conn.id, seq, len);

            conn.inflight.insert(
                seq,
                SentSeg {
                    len,
                },
            );

            // 若这是最早未确认段，启动/刷新 RTO
            if conn.earliest_unacked_seq() == Some(seq) {
                sim.schedule(
                    SimTime(sim.now().0.saturating_add(conn.rto.0)),
                    TcpRto {
                        conn_id: conn.id,
                        seq,
                    },
                );
            }

            net.forward_from(conn.src, pkt, sim);
        }
    }

    fn send_ack(&mut self, id: TcpConnId, ack: u64, sim: &mut Simulator, net: &mut Network) {
        let Some(conn) = self.conns.get(&id) else {
            return;
        };
        let mut pkt = net.make_packet(conn.id, conn.cfg.ack_bytes, conn.rev_route.clone());
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
            TcpSegment::Data { seq, len } => {
                let Some(conn) = self.conns.get_mut(&conn_id) else {
                    return;
                };
                if at != conn.dst {
                    // 不是目的 host：忽略（理论上不会发生，因为只在 delivered 回调中调用）
                    return;
                }

                if seq == conn.rcv_nxt {
                    conn.rcv_nxt = conn.rcv_nxt.saturating_add(len as u64);
                }
                // 无论是否乱序，都发累计 ACK（dupACK 体现为 ack 不前进）
                let ack = conn.rcv_nxt;
                let _ = conn;
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
                    conn.dup_acks = 0;
                    let newly_acked = ack - conn.last_acked;
                    conn.last_acked = ack;

                    // 移除已确认段
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

                    // 拥塞控制：慢启动 / 拥塞避免（极简）
                    if conn.cwnd_bytes < conn.ssthresh_bytes {
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(newly_acked);
                    } else {
                        // AIMD：每个 ACK 让 cwnd 以 mss^2/cwnd 增长（至少 +1）
                        let mss = conn.cfg.mss as u64;
                        let inc = (mss.saturating_mul(mss) / conn.cwnd_bytes).max(1);
                        conn.cwnd_bytes = conn.cwnd_bytes.saturating_add(inc);
                    }

                    // 完成判定：所有数据都被累计确认
                    if conn.last_acked >= conn.total_bytes && conn.done_at.is_none() {
                        conn.done_at = Some(sim.now());
                        return;
                    }

                    // 继续发送
                    let id = conn.id;
                    let _ = conn;
                    self.send_data_if_possible(id, sim, net);
                } else if ack == conn.last_acked {
                    // dupACK
                    conn.dup_acks = conn.dup_acks.saturating_add(1);
                    let dup = conn.dup_acks;
                    let mss = conn.cfg.mss as u64;
                    if dup == 3 {
                        // 快速重传：重传 earliest unacked
                        if let Some(seq0) = conn.earliest_unacked_seq() {
                            conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
                            conn.cwnd_bytes = conn.ssthresh_bytes.saturating_add(3 * mss);
                            let len = conn.inflight.get(&seq0).map(|s| s.len).unwrap_or(conn.cfg.mss);
                            let mut pkt = net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
                            pkt.size_bytes = conn.cfg.mss;
                            pkt.transport = Transport::Tcp(TcpSegment::Data { seq: seq0, len });
                            net.forward_from(conn.src, pkt, sim);
                        }
                    } else if dup > 3 {
                        // 快速恢复：每个额外 dupACK 增加 cwnd 一个 MSS
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
    pub seq: u64,
}

impl Event for TcpRto {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let TcpRto { conn_id, seq } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");

        // 先记录 RTO 事件（即将触发重传）
        w.net.viz_tcp_rto(sim.now().0, conn_id, seq);

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

        // 仅当该 seq 仍是 earliest unacked 且仍未被确认时才处理
        if conn.earliest_unacked_seq() != Some(seq) {
            w.net.tcp = tcp;
            return;
        }
        let Some(sent) = conn.inflight.get(&seq).cloned() else {
            w.net.tcp = tcp;
            return;
        };

        // 超时：回到慢启动
        let mss = conn.cfg.mss as u64;
        conn.ssthresh_bytes = (conn.cwnd_bytes / 2).max(2 * mss);
        conn.cwnd_bytes = mss;
        conn.dup_acks = 0;
        conn.rto = SimTime((conn.rto.0.saturating_mul(2)).min(conn.cfg.max_rto.0));

        // 重传 earliest unacked
        let mut pkt = w.net.make_packet(conn.id, conn.cfg.mss, conn.fwd_route.clone());
        pkt.size_bytes = conn.cfg.mss;
        pkt.transport = Transport::Tcp(TcpSegment::Data { seq, len: sent.len });
        w.net.forward_from(conn.src, pkt, sim);

        // 重新调度 RTO
        sim.schedule(
            SimTime(sim.now().0.saturating_add(conn.rto.0)),
            TcpRto { conn_id, seq },
        );

        w.net.tcp = tcp;
    }
}

