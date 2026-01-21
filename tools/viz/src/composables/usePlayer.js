import { computed, onBeforeUnmount, onMounted, reactive, watch } from "vue";
import { fmtBytes, fmtCapBytes, fmtGbps, fmtMs } from "../utils/format";
import {
    buildLinkPairs,
    defaultLinks,
    defaultNodes,
    layoutCircle,
    layoutDumbbell,
    layoutFatTree,
    linkKey,
} from "../utils/layout";
import { buildTcpSeries, pickAutoConn, pickPointAt } from "../utils/tcp";

export function usePlayer() {
    let netCtx = null;
    let tcpCtx = null;
    let netCanvas = null;
    let tcpCanvas = null;
    let tcpDetailCtx = null;
    let tcpDetailCanvas = null;
    let tcpModalCtx = null;
    let tcpModalCanvas = null;
    let tcpCardRef = null;
    let rafId = 0;

    const state = reactive({
        events: [],
        filtered: [],
        meta: null,
        layoutChoice: "auto",
        layoutDetected: "-",
        filterFlow: "",
        filterPkt: "",
        connPick: "auto",
        connOptions: [],
        speed: 1,
        targetWallSec: 12,
        playing: false,
        t0: 0,
        t1: 0,
        curTime: 0,
        cursor: 0,
        lastWall: 0,
        slider: 0,
        inflight: new Map(),
        nodeHighlight: new Map(),
        dropMarks: [],
        lastEventsText: [],
        nodes: defaultNodes.map((n) => ({ ...n })),
        nodeById: new Map(),
        drawLinks: defaultLinks.slice(),
        nodeScale: 1,
        nodeStats: new Map(),
        linkStats: new Map(),
        tcpStats: { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 },
        // 兼容旧日志：用于“推断重传次数”（同一 end=seq+len 在未被 ack 前再次发送）
        tcpTrack: new Map(),
        tcpSeries: new Map(),
        curText: "（空）",
        statsText: "（空）",
        maxLinkBandwidth: 0, // 用于判断瓶颈链路
    });

    const hasEvents = computed(() => state.events.length > 0);
    const canPlay = computed(() => state.filtered.length > 0);
    const metaNodesCount = computed(() => state.meta?.nodes?.length ?? 0);
    const metaLinksCount = computed(() => state.meta?.links?.length ?? 0);
    const layoutDetectedLabel = computed(() => state.layoutDetected || "-");
    const topologyStatus = computed(() => {
        if (!hasEvents.value) return "未加载数据";
        return `拓扑=${state.layoutDetected} · 时间范围 ${fmtMs(state.t0)} → ${fmtMs(state.t1)}`;
    });
    const statusText = computed(() => {
        if (!hasEvents.value) return "未加载";
        if (!state.filtered.length) return `已加载 ${state.events.length} 条事件（过滤后 0）`;
        return `已加载 ${state.events.length} 条事件（过滤后 ${state.filtered.length}），当前：${fmtMs(state.curTime)}`;
    });

    function setTime(t) {
        state.curTime = Math.max(state.t0, Math.min(state.t1, t));
        const p = (state.curTime - state.t0) / Math.max(1, state.t1 - state.t0);
        state.slider = Math.floor(p * 1000);
    }

    function applyUntil(t) {
        if (!state.filtered.length) return;
        if (state.cursor > 0 && state.cursor < state.filtered.length && state.filtered[state.cursor - 1].t_ns > t) {
            state.inflight = new Map();
            state.nodeHighlight = new Map();
            state.dropMarks = [];
            state.lastEventsText = [];
            state.nodeStats = new Map();
            state.linkStats = new Map();
            state.tcpStats = { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 };
            state.tcpTrack = new Map();
            initStatsFromMeta();
            state.cursor = 0;
        }
        while (state.cursor < state.filtered.length && state.filtered[state.cursor].t_ns <= t) {
            const ev = state.filtered[state.cursor++];
            applyEvent(ev);
        }
    }

    function showCurrentText() {
        state.curText = state.lastEventsText.slice(-80).join("\n") || "（空）";
    }

    function showStatsText() {
        const lines = [];
        lines.push(`时间：${fmtMs(state.curTime)} / ${fmtMs(state.t1)}`);
        lines.push("说明：drop 发生在链路队列（link_from->link_to），不是 host。\n");
        lines.push("节点状态（计数随时间推进）：");
        lines.push("- rx：节点收到并开始处理（node_rx）");
        lines.push("- forward：节点决定下一跳（node_forward）");
        lines.push("- delivered：到达目的节点并交付（delivered）\n");
        for (const n of state.nodes) {
            const s = state.nodeStats.get(n.id) || {};
            lines.push(
                `- ${n.name}(${n.kind}) id=${n.id}: rx=${s.rx ?? 0}, forward=${s.forward ?? 0}, delivered=${s.delivered ?? 0}, rx_bytes=${fmtBytes(s.bytes ?? 0)}`
            );
        }
        lines.push("\n链路状态（队列/带宽/时延）：");
        lines.push("- q：队列字节数（enqueue/drop 事件里的 q_bytes）");
        lines.push("- q_peak：队列峰值（整个回放过程中观察到的最大 q）");
        lines.push("- tx：链路开始发送次数（tx_start；视为出队）");
        lines.push("- drop：丢包次数（drop；发生在该链路队列入队时）");
        const keys = Array.from(state.linkStats.keys()).sort();
        for (const k of keys) {
            const s = state.linkStats.get(k);
            if (!s) continue;
            const q = `${fmtBytes(s.q_bytes)}/${s.q_cap != null ? fmtCapBytes(s.q_cap) : "-"}`;
            const qp = `${fmtBytes(s.q_peak ?? 0)}/${s.q_cap != null ? fmtCapBytes(s.q_cap) : "-"}`;
            const lat = s.latency_ns != null ? fmtMs(s.latency_ns) : "-";
            const fd = s.first_drop_t != null ? fmtMs(s.first_drop_t) : "-";
            lines.push(
                `- ${k}: q=${q}, q_peak=${qp}, tx=${s.tx_pkts}, drop=${s.drop_pkts} (first_drop=${fd}), bw=${fmtGbps(s.bandwidth_bps)}, lat=${lat}`
            );
        }
        lines.push("\nTCP（全局事件计数）：");
        lines.push(
            `- send_data=${state.tcpStats.send_data}, send_ack=${state.tcpStats.send_ack}, recv_ack=${state.tcpStats.recv_ack}, rto=${state.tcpStats.rto}, retrans=${state.tcpStats.retrans}`
        );
        state.statsText = lines.join("\n");
    }

    function initStatsFromMeta() {
        for (const n of state.nodes) {
            state.nodeStats.set(n.id, { rx: 0, forward: 0, delivered: 0, bytes: 0 });
        }
        state.maxLinkBandwidth = 0;
        if (state.meta?.links) {
            for (const l of state.meta.links) {
                const bw = l.bandwidth_bps ?? 0;
                if (bw > state.maxLinkBandwidth) state.maxLinkBandwidth = bw;
                state.linkStats.set(linkKey(l.from, l.to), {
                    from: l.from,
                    to: l.to,
                    q_bytes: 0,
                    q_peak: 0,
                    q_pkts: 0,
                    q_pkts_peak: 0,
                    q_cap: l.q_cap_bytes ?? null,
                    tx_pkts: 0,
                    drop_pkts: 0,
                    first_drop_t: null,
                    bandwidth_bps: bw,
                    latency_ns: l.latency_ns ?? null,
                });
            }
        }
    }

    function updateConnPick() {
        const conns = Array.from(state.tcpSeries.keys()).sort((a, b) => a - b);
        state.connOptions = conns;
        if (state.connPick !== "auto" && !conns.includes(Number(state.connPick))) {
            state.connPick = "auto";
        }
    }

    function resetPlayback() {
        state.inflight = new Map();
        state.nodeHighlight = new Map();
        state.dropMarks = [];
        state.lastEventsText = [];
        state.nodeStats = new Map();
        state.linkStats = new Map();
        state.tcpStats = { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 };
        state.tcpTrack = new Map();
        state.tcpSeries = buildTcpSeries(state.events);
        updateConnPick();
        initStatsFromMeta();
        state.cursor = 0;
        state.curTime = state.t0;
        state.slider = 0;
        redraw();
        showCurrentText();
        showStatsText();
        redrawTcpAll();
    }

    function applyFilter() {
        const flow = state.filterFlow.trim();
        const pkt = state.filterPkt.trim();
        const flowN = flow === "" ? null : Number(flow);
        const pktN = pkt === "" ? null : Number(pkt);
        state.filtered = state.events.filter((e) => {
            if (flowN != null && (e.flow_id == null || Number(e.flow_id) !== flowN)) return false;
            if (pktN != null && (e.pkt_id == null || Number(e.pkt_id) !== pktN)) return false;
            return true;
        });
        resetPlayback();
    }

    function applyEvent(ev) {
        const kind = ev.kind;
        const head = `[${fmtMs(ev.t_ns)}] ${kind}`;
        if (kind === "tx_start" && ev.pkt_id != null) {
            state.inflight.set(Number(ev.pkt_id), {
                pkt_id: Number(ev.pkt_id),
                flow_id: ev.flow_id != null ? Number(ev.flow_id) : null,
                from: Number(ev.link_from),
                to: Number(ev.link_to),
                depart: Number(ev.depart_ns),
                arrive: Number(ev.arrive_ns),
                pkt_kind: ev.pkt_kind || "other",
            });
            state.lastEventsText.push(
                `${head} pkt=${ev.pkt_id} ${ev.link_from}->${ev.link_to} depart=${fmtMs(ev.depart_ns)} arrive=${fmtMs(ev.arrive_ns)}`
            );
            const lk = linkKey(Number(ev.link_from), Number(ev.link_to));
            const ls =
                state.linkStats.get(lk) || { q_bytes: 0, q_peak: 0, q_pkts: 0, q_pkts_peak: 0, q_cap: null, tx_pkts: 0, drop_pkts: 0, first_drop_t: null };
            const pb = ev.pkt_bytes != null ? Number(ev.pkt_bytes) : 0;
            ls.q_bytes = Math.max(0, Number(ls.q_bytes || 0) - pb);
            ls.q_pkts = Math.max(0, Number(ls.q_pkts || 0) - 1); // 出队，包数 -1
            ls.tx_pkts = Number(ls.tx_pkts || 0) + 1;
            ls.q_peak = Math.max(Number(ls.q_peak || 0), Number(ls.q_bytes || 0));
            state.linkStats.set(lk, ls);
        } else if (kind === "delivered" && ev.pkt_id != null) {
            state.inflight.delete(Number(ev.pkt_id));
            state.lastEventsText.push(`${head} pkt=${ev.pkt_id} node=${ev.node}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.delivered = Number(ns.delivered || 0) + 1;
            state.nodeStats.set(Number(ev.node), ns);
        } else if (kind === "drop" && ev.pkt_id != null) {
            state.inflight.delete(Number(ev.pkt_id));
            state.dropMarks.push({
                pkt_id: Number(ev.pkt_id),
                flow_id: ev.flow_id != null ? Number(ev.flow_id) : null,
                from: Number(ev.link_from),
                to: Number(ev.link_to),
                at: Number(ev.t_ns),
                until: Number(ev.t_ns) + 2_000_000,
            });
            state.lastEventsText.push(
                `${head} pkt=${ev.pkt_id} link=${ev.link_from}->${ev.link_to} q=${ev.q_bytes}/${ev.q_cap_bytes}`
            );
            const lk = linkKey(Number(ev.link_from), Number(ev.link_to));
            const ls =
                state.linkStats.get(lk) || { q_bytes: 0, q_peak: 0, q_pkts: 0, q_pkts_peak: 0, q_cap: null, tx_pkts: 0, drop_pkts: 0, first_drop_t: null };
            ls.drop_pkts = Number(ls.drop_pkts || 0) + 1;
            if (ev.q_bytes != null) ls.q_bytes = Number(ev.q_bytes);
            if (ev.q_cap_bytes != null) ls.q_cap = Number(ev.q_cap_bytes);
            ls.q_peak = Math.max(Number(ls.q_peak || 0), Number(ls.q_bytes || 0));
            // drop 时包没入队，不更新 q_pkts
            if (ls.first_drop_t == null) ls.first_drop_t = Number(ev.t_ns ?? 0);
            state.linkStats.set(lk, ls);
        } else if (kind === "node_rx") {
            state.nodeHighlight.set(Number(ev.node), { node: Number(ev.node), until: ev.t_ns + 200_000 });
            state.lastEventsText.push(`${head} node=${ev.node} (${ev.node_kind}:${ev.node_name}) pkt=${ev.pkt_id}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.rx = Number(ns.rx || 0) + 1;
            if (ev.pkt_bytes != null) ns.bytes = Number(ns.bytes || 0) + Number(ev.pkt_bytes);
            state.nodeStats.set(Number(ev.node), ns);
        } else if (kind === "node_forward") {
            state.lastEventsText.push(`${head} node=${ev.node} -> next=${ev.next} pkt=${ev.pkt_id}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.forward = Number(ns.forward || 0) + 1;
            state.nodeStats.set(Number(ev.node), ns);
        } else if (kind === "enqueue") {
            state.lastEventsText.push(`${head} pkt=${ev.pkt_id} link=${ev.link_from}->${ev.link_to} q=${ev.q_bytes}/${ev.q_cap_bytes}`);
            const lk = linkKey(Number(ev.link_from), Number(ev.link_to));
            const ls =
                state.linkStats.get(lk) || { q_bytes: 0, q_peak: 0, q_pkts: 0, q_pkts_peak: 0, q_cap: null, tx_pkts: 0, drop_pkts: 0, first_drop_t: null };
            if (ev.q_bytes != null) ls.q_bytes = Number(ev.q_bytes);
            if (ev.q_cap_bytes != null) ls.q_cap = Number(ev.q_cap_bytes);
            ls.q_pkts = Number(ls.q_pkts || 0) + 1; // 入队，包数 +1
            ls.q_pkts_peak = Math.max(Number(ls.q_pkts_peak || 0), ls.q_pkts);
            ls.q_peak = Math.max(Number(ls.q_peak || 0), Number(ls.q_bytes || 0));
            state.linkStats.set(lk, ls);
        } else if (kind.startsWith("tcp_")) {
            const extra = [];
            if (ev.conn_id != null) extra.push(`conn=${ev.conn_id}`);
            if (ev.seq != null) extra.push(`seq=${ev.seq}`);
            if (ev.len != null) extra.push(`len=${ev.len}`);
            if (ev.ack != null) extra.push(`ack=${ev.ack}`);
            if (ev.ecn_echo) extra.push("ecn_echo=1");
            state.lastEventsText.push(`${head} ${extra.join(" ")}`.trim());
        } else {
            state.lastEventsText.push(`${head} pkt=${ev.pkt_id ?? "-"}`);
        }

        // TCP 事件计数（随时间推进）
        if (kind === "tcp_send_data") {
            state.tcpStats.send_data += 1;
            // 新日志：Rust 会在真正重传时发出 tcp_send_data 且带 retrans=true
            if (ev.retrans === true) {
                state.tcpStats.retrans += 1;
                return;
            }
            const cid = Number(ev.conn_id ?? ev.flow_id ?? 0);
            const seq = ev.seq != null ? Number(ev.seq) : null;
            const len = ev.len != null ? Number(ev.len) : null;
            if (cid && seq != null && len != null) {
                let tr = state.tcpTrack.get(cid);
                if (!tr) {
                    tr = { lastAck: 0, sentEnds: new Set() };
                    state.tcpTrack.set(cid, tr);
                }
                const end = seq + len;
                const isRetrans = tr.sentEnds.has(end) && end > tr.lastAck;
                if (isRetrans) state.tcpStats.retrans += 1;
                tr.sentEnds.add(end);
            }
        }
        if (kind === "tcp_send_ack") state.tcpStats.send_ack += 1;
        if (kind === "tcp_recv_ack") {
            state.tcpStats.recv_ack += 1;
            const cid = Number(ev.conn_id ?? ev.flow_id ?? 0);
            const ack = ev.ack != null ? Number(ev.ack) : null;
            if (cid && ack != null) {
                let tr = state.tcpTrack.get(cid);
                if (!tr) {
                    tr = { lastAck: 0, sentEnds: new Set() };
                    state.tcpTrack.set(cid, tr);
                }
                if (ack > tr.lastAck) {
                    tr.lastAck = ack;
                    // 删除已累计确认的数据段 end（用于后续重传识别）
                    for (const end of Array.from(tr.sentEnds)) {
                        if (end <= ack) tr.sentEnds.delete(end);
                    }
                }
            }
        }
        if (kind === "tcp_rto") state.tcpStats.rto += 1;
    }

    function redraw() {
        if (!netCtx || !netCanvas) return;
        netCtx.clearRect(0, 0, netCanvas.width, netCanvas.height);
        for (const l of state.drawLinks) drawLink(l);
        for (const n of state.nodes) drawNode(n);
        for (const p of state.inflight.values()) drawPacket(p);
        // 不再绘制红色叉标记（因为链路标签已显示 drop 数量和颜色）
        state.dropMarks = state.dropMarks.filter((m) => m.until >= state.curTime);
    }

    function drawLink(l) {
        const a = state.nodeById.get(l.from);
        const b = state.nodeById.get(l.to);
        if (!a || !b) return;

        // 获取两个方向的链路统计
        const fwd = state.linkStats.get(linkKey(l.from, l.to));
        const rev = state.linkStats.get(linkKey(l.to, l.from));

        // 计算队列占用比例（取两个方向的最大值）
        const fwdRatio = fwd && fwd.q_cap > 0 ? fwd.q_bytes / fwd.q_cap : 0;
        const revRatio = rev && rev.q_cap > 0 ? rev.q_bytes / rev.q_cap : 0;
        const maxRatio = Math.max(fwdRatio, revRatio);

        // 判断是否为瓶颈链路（带宽低于最大带宽）
        const fwdBw = fwd?.bandwidth_bps ?? 0;
        const revBw = rev?.bandwidth_bps ?? 0;
        const linkBw = Math.min(fwdBw || Infinity, revBw || Infinity);
        const isBottleneck = linkBw < state.maxLinkBandwidth && linkBw > 0;

        // 根据队列深度计算链路颜色（绿→黄→红）
        const linkColor = queueRatioColor(maxRatio);
        // 链路粗细随队列深度增加（4~10）
        const baseWidth = 4 + Math.min(6, maxRatio * 8);

        netCtx.save();
        // 底层阴影
        netCtx.strokeStyle = "rgba(15,23,42,0.12)";
        netCtx.lineWidth = baseWidth + 4;
        netCtx.lineCap = "round";
        netCtx.beginPath();
        netCtx.moveTo(a.x, a.y);
        netCtx.lineTo(b.x, b.y);
        netCtx.stroke();

        // 瓶颈链路用虚线
        if (isBottleneck) {
            netCtx.setLineDash([8, 4]);
        }

        // 主链路（颜色随队列深度变化）
        netCtx.strokeStyle = linkColor;
        netCtx.lineWidth = baseWidth;
        netCtx.beginPath();
        netCtx.moveTo(a.x, a.y);
        netCtx.lineTo(b.x, b.y);
        netCtx.stroke();

        // 高光
        netCtx.setLineDash([]);
        netCtx.strokeStyle = "rgba(255,255,255,0.35)";
        netCtx.lineWidth = Math.max(1, baseWidth * 0.3);
        netCtx.beginPath();
        netCtx.moveTo(a.x, a.y);
        netCtx.lineTo(b.x, b.y);
        netCtx.stroke();
        netCtx.restore();

        // 绘制链路标签（队列深度 + 丢包数 + 瓶颈带宽）
        drawLinkLabel(a, b, fwd, rev, isBottleneck, linkBw);
    }

    function queueRatioColor(ratio) {
        // 0 = 绿色，0.5 = 黄色，1 = 红色
        const r = ratio < 0.5 ? Math.round(255 * (ratio * 2)) : 255;
        const g = ratio < 0.5 ? 200 : Math.round(200 * (1 - (ratio - 0.5) * 2));
        const b = 80;
        const alpha = 0.5 + ratio * 0.4; // 队列越满越不透明
        return `rgba(${r},${g},${b},${alpha})`;
    }

    function drawLinkLabel(a, b, fwd, rev, isBottleneck, linkBw) {
        // 计算链路中点和法向量（用于偏移标签位置）
        const mx = (a.x + b.x) / 2;
        const my = (a.y + b.y) / 2;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const len = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        const nx = -dy / len;
        const ny = dx / len;

        // 汇总两个方向的统计（包数）
        const fwdPkts = fwd?.q_pkts ?? 0;
        const revPkts = rev?.q_pkts ?? 0;
        const fwdPeak = fwd?.q_pkts_peak ?? 0;
        const revPeak = rev?.q_pkts_peak ?? 0;
        const fwdDrop = fwd?.drop_pkts ?? 0;
        const revDrop = rev?.drop_pkts ?? 0;
        const totalPkts = fwdPkts + revPkts;
        const totalPeak = fwdPeak + revPeak;
        const totalDrop = fwdDrop + revDrop;

        // 如果没有任何队列/丢包且没有带宽信息，不显示标签
        if (totalPkts === 0 && totalPeak === 0 && totalDrop === 0 && linkBw <= 0) return;

        const offsetY = 14;
        const lx = mx + nx * offsetY;
        const ly = my + ny * offsetY;

        netCtx.save();
        netCtx.font = "10px JetBrains Mono, monospace";
        netCtx.textAlign = "center";
        netCtx.textBaseline = "middle";

        // 构建标签文本：带宽 | q:当前/峰值 | drop:丢包数
        const parts = [];
        // 所有链路都显示带宽
        if (linkBw > 0) {
            parts.push(`${fmtGbps(linkBw)}`);
        }
        if (totalPeak > 0) {
            parts.push(`q:${totalPkts}/${totalPeak}`);
        }
        if (totalDrop > 0) {
            parts.push(`drop:${totalDrop}`);
        }
        const text = parts.join(" ");
        if (!text) {
            netCtx.restore();
            return;
        }

        // 绘制背景（瓶颈链路用橙色边框）
        const tw = netCtx.measureText(text).width + 8;
        const th = 14;
        netCtx.fillStyle = "rgba(255,255,255,0.9)";
        const borderColor = totalDrop > 0 ? "rgba(239,68,68,0.7)" : isBottleneck ? "rgba(245,158,11,0.7)" : "rgba(15,23,42,0.25)";
        netCtx.strokeStyle = borderColor;
        netCtx.lineWidth = 1;
        netCtx.beginPath();
        netCtx.roundRect(lx - tw / 2, ly - th / 2, tw, th, 4);
        netCtx.fill();
        netCtx.stroke();

        // 绘制文本（有丢包时用红色，瓶颈用橙色）
        netCtx.fillStyle = totalDrop > 0 ? "#dc2626" : isBottleneck ? "#d97706" : "#334155";
        netCtx.fillText(text, lx, ly);
        netCtx.restore();
    }

    function drawNode(n) {
        const hl = state.nodeHighlight.get(n.id);
        const isHl = hl && hl.until >= state.curTime;
        const r = (n.kind === "switch" ? 24 : 20) * state.nodeScale;
        const fontSize = Math.max(8, Math.round(12 * state.nodeScale));
        netCtx.save();
        netCtx.fillStyle = isHl ? "rgba(14,116,144,0.25)" : "rgba(255,255,255,0.85)";
        netCtx.strokeStyle = isHl ? "rgba(14,116,144,0.8)" : "rgba(15,23,42,0.25)";
        netCtx.lineWidth = 2;
        netCtx.beginPath();
        netCtx.arc(n.x, n.y, r, 0, Math.PI * 2);
        netCtx.fill();
        netCtx.stroke();

        netCtx.fillStyle = "#0f172a";
        netCtx.font = `${fontSize}px JetBrains Mono, monospace`;
        netCtx.textAlign = "center";
        netCtx.textBaseline = "middle";
        netCtx.fillText(n.name, n.x, n.y);
        netCtx.restore();
    }

    function drawPacket(p) {
        const a = state.nodeById.get(p.from);
        const b = state.nodeById.get(p.to);
        if (!a || !b) return;
        const t = (() => {
            if (state.curTime <= p.depart) return 0;
            if (state.curTime >= p.arrive) return 1;
            return (state.curTime - p.depart) / Math.max(1, p.arrive - p.depart);
        })();
        const x = a.x + (b.x - a.x) * t;
        const y = a.y + (b.y - a.y) * t;

        netCtx.save();
        netCtx.fillStyle = pktColor(p.pkt_kind);
        netCtx.strokeStyle = "rgba(0,0,0,0.25)";
        netCtx.lineWidth = 2;
        netCtx.beginPath();
        netCtx.arc(x, y, Math.max(3, 7 * state.nodeScale), 0, Math.PI * 2);
        netCtx.fill();
        netCtx.stroke();
        netCtx.restore();
    }

    function drawDropMark(m, stackIdx, showLabel) {
        const a = state.nodeById.get(m.from);
        const b = state.nodeById.get(m.to);
        if (!a || !b) return;
        const mx = (a.x + b.x) / 2;
        const my = (a.y + b.y) / 2;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const len = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        const nx = -dy / len;
        const ny = dx / len;
        const offset = (stackIdx + 1) * 10;
        const x = mx + nx * offset;
        const y = my + ny * offset;

        netCtx.save();
        netCtx.strokeStyle = "#ef4444";
        netCtx.lineWidth = 3;
        netCtx.beginPath();
        netCtx.moveTo(x - 6, y - 6);
        netCtx.lineTo(x + 6, y + 6);
        netCtx.moveTo(x + 6, y - 6);
        netCtx.lineTo(x - 6, y + 6);
        netCtx.stroke();
        if (showLabel) {
            netCtx.fillStyle = "#ef4444";
            netCtx.font = "11px JetBrains Mono, monospace";
            netCtx.fillText(`drop@${fmtMs(m.at)}`, x + 8, y - 8);
        }
        netCtx.restore();
    }

    function pktColor(kind) {
        if (kind === "data") return "#0ea5e9";
        if (kind === "ack") return "#22c55e";
        return "#1f2937";
    }

    function selectTcpConn() {
        const conns = Array.from(state.tcpSeries.keys()).sort((a, b) => a - b);
        if (!conns.length) return { cid: null, ser: null };
        const prefer = state.filterFlow.trim() ? Number(state.filterFlow.trim()) : null;
        let cid = null;
        if (state.connPick && state.connPick !== "auto") {
            const pickN = Number(state.connPick);
            if (state.tcpSeries.has(pickN)) cid = pickN;
        }
        if (cid == null && prefer != null && state.tcpSeries.has(prefer)) {
            cid = prefer;
        }
        if (cid == null) {
            cid = pickAutoConn(state.tcpSeries, state.curTime);
        }
        if (cid == null) {
            cid = conns[0];
        }
        if (cid == null) return { cid: null, ser: null };
        return { cid, ser: state.tcpSeries.get(cid) || null };
    }

    function redrawTcp() {
        if (!tcpCtx || !tcpCanvas) return;
        tcpCtx.clearRect(0, 0, tcpCanvas.width, tcpCanvas.height);

        const sel = selectTcpConn();
        const cid = sel.cid;
        const ser = sel.ser;
        if (cid == null || !ser) {
            drawTcpBoxAt(tcpCanvas.width / 2, tcpCanvas.height / 2, "无 tcp_* / dctcp_cwnd 事件");
            return;
        }
        const pts = ser?.points || [];
        const mss = ser?.mss || 1460;
        if (!pts.length) {
            drawTcpBoxAt(tcpCanvas.width / 2, tcpCanvas.height / 2, `conn=${cid} 无可用数据点`);
            return;
        }

        // 2x2 子图布局
        const gap = 12;
        const subW = Math.floor((tcpCanvas.width - gap) / 2);
        const subH = Math.floor((tcpCanvas.height - gap) / 2);
        const areas = [
            { x: 0, y: 0, w: subW, h: subH, title: "cwnd（拥塞窗口）", field: "cwnd", color: "#0ea5e9", fill: false },
            { x: subW + gap, y: 0, w: subW, h: subH, title: "ssthresh（慢启动阈值）", field: "ssthresh", color: "#ef4444", fill: false },
            { x: 0, y: subH + gap, w: subW, h: subH, title: "inflight（在途数据）", field: "inflight", color: "#22c55e", fill: true },
            { x: subW + gap, y: subH + gap, w: subW, h: subH, title: "三者对比", field: "all", color: "", fill: false },
        ];

        const curP = pickPointAt(pts, state.curTime);

        for (const area of areas) {
            drawTcpSubChart(pts, mss, area, cid, curP);
        }
    }

    function drawTcpSubChart(pts, mss, area, cid, curP) {
        drawTcpSubChartOnCtx(tcpCtx, null, pts, mss, area, cid, curP, true);
    }

    // 计算 ssthresh 的稳态范围（排除初始极大值）
    function computeStableSsthresh(ssVals, maxCwnd) {
        if (!ssVals.length) return maxCwnd;
        const firstVal = ssVals[0];
        // 找第一次下降：ssthresh 变小的位置
        let firstDropIdx = -1;
        for (let i = 1; i < ssVals.length; i++) {
            if (ssVals[i] < firstVal * 0.9) {
                firstDropIdx = i;
                break;
            }
        }
        if (firstDropIdx === -1) {
            // 没有下降过，说明没发生拥塞，用 cwnd 最大值的 1.5 倍
            return maxCwnd * 1.5;
        }
        // 取下降后的最大值
        const afterDropVals = ssVals.slice(firstDropIdx);
        const maxAfterDrop = Math.max(...afterDropVals);
        // 返回下降后最大值的 1.2 倍，留点余量
        return maxAfterDrop * 1.2;
    }

    function drawTcpSubChartOnCtx(ctx, canvas, pts, mss, area, cid, curP, isSmall = false) {
        const { x: ax, y: ay, w: aw, h: ah, title, field, color, fill } = area;
        // 放大版用更大的 padding
        const pad = isSmall ? { l: 50, r: 8, t: 18, b: 6 } : { l: 80, r: 20, t: 40, b: 30 };
        const chartX = ax + pad.l;
        const chartY = ay + pad.t;
        const chartW = aw - pad.l - pad.r;
        const chartH = ah - pad.t - pad.b;

        // 背景
        ctx.save();
        rr(ctx, ax + 0.5, ay + 0.5, aw - 1, ah - 1, isSmall ? 8 : 12);
        ctx.fillStyle = isSmall ? "rgba(15,23,42,0.03)" : "rgba(255,255,255,1)";
        ctx.fill();
        ctx.strokeStyle = "rgba(15,23,42,0.1)";
        ctx.stroke();
        ctx.restore();

        // 计算该字段的 Y 轴范围
        let maxVal;
        if (field === "all" || field === "ssthresh") {
            const maxCwnd = Math.max(...pts.map((p) => p.cwnd ?? 0));
            const maxInflight = Math.max(...pts.map((p) => p.inflight ?? 0));
            // ssthresh：找第一次下降后的稳态值
            const ssVals = pts.map((p) => p.ssthresh ?? 0);
            const stableSsthresh = computeStableSsthresh(ssVals, maxCwnd);
            if (field === "all") {
                maxVal = Math.max(maxCwnd, maxInflight, stableSsthresh);
            } else {
                maxVal = stableSsthresh;
            }
        } else {
            maxVal = Math.max(...pts.map((p) => p[field] ?? 0));
        }
        const maxPkts = Math.max(2, Math.ceil(maxVal / mss) + 1);

        const xOf = (t) => chartX + ((t - state.t0) / Math.max(1, state.t1 - state.t0)) * chartW;
        const yOf = (v) => chartY + (1 - v / mss / maxPkts) * chartH;

        // 网格线
        const gridLines = isSmall ? 2 : 5;
        const fontSize = isSmall ? 9 : 12;
        ctx.save();
        ctx.strokeStyle = "rgba(15,23,42,0.08)";
        ctx.fillStyle = "rgba(15,23,42,0.6)";
        ctx.font = `${fontSize}px JetBrains Mono, monospace`;
        for (let k = 0; k <= gridLines; k++) {
            const pk = Math.round((maxPkts * k) / gridLines);
            const y = chartY + (1 - k / gridLines) * chartH;
            ctx.beginPath();
            ctx.moveTo(chartX, y);
            ctx.lineTo(chartX + chartW, y);
            ctx.stroke();
            ctx.textAlign = "right";
            ctx.textBaseline = "middle";
            ctx.fillText(`${pk}`, chartX - 6, y);
        }
        ctx.restore();

        // 标题
        ctx.save();
        ctx.fillStyle = "rgba(15,23,42,0.8)";
        ctx.font = `${isSmall ? 10 : 16}px JetBrains Mono, monospace`;
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        ctx.fillText(title, ax + (isSmall ? 6 : 16), ay + (isSmall ? 4 : 12));
        ctx.restore();

        // Y 轴标签
        if (!isSmall) {
            ctx.save();
            ctx.fillStyle = "rgba(15,23,42,0.5)";
            ctx.font = "11px JetBrains Mono, monospace";
            ctx.textAlign = "center";
            ctx.save();
            ctx.translate(ax + 16, chartY + chartH / 2);
            ctx.rotate(-Math.PI / 2);
            ctx.fillText("pkts", 0, 0);
            ctx.restore();
            ctx.restore();
        }

        // 绘制曲线
        const lineWidth = isSmall ? 1.5 : 2.5;
        if (field === "all") {
            drawTcpLineInAreaOnCtx(ctx, pts, "inflight", "rgba(34,197,94,0.15)", "#22c55e", false, true, xOf, yOf, mss, chartX, chartY, chartW, chartH, lineWidth);
            drawTcpLineInAreaOnCtx(ctx, pts, "ssthresh", "#ef4444", "#ef4444", true, false, xOf, yOf, mss, chartX, chartY, chartW, chartH, lineWidth);
            drawTcpLineInAreaOnCtx(ctx, pts, "cwnd", "#0ea5e9", "#0ea5e9", false, false, xOf, yOf, mss, chartX, chartY, chartW, chartH, lineWidth);
        } else {
            drawTcpLineInAreaOnCtx(ctx, pts, field, fill ? `${color}30` : color, color, false, fill, xOf, yOf, mss, chartX, chartY, chartW, chartH, lineWidth);
        }

        // 图例（仅放大版且是 all 时显示）
        if (!isSmall && field === "all") {
            const legendX = chartX + chartW - 180;
            const legendY = chartY + 10;
            ctx.save();
            ctx.fillStyle = "rgba(255,255,255,0.9)";
            ctx.strokeStyle = "rgba(15,23,42,0.15)";
            rr(ctx, legendX, legendY, 170, 70, 6);
            ctx.fill();
            ctx.stroke();

            const items = [
                { label: "cwnd", color: "#0ea5e9", dashed: false },
                { label: "ssthresh", color: "#ef4444", dashed: true },
                { label: "inflight", color: "#22c55e", dashed: false, fill: true },
            ];
            items.forEach((item, i) => {
                const y = legendY + 15 + i * 18;
                ctx.beginPath();
                if (item.fill) {
                    ctx.fillStyle = "rgba(34,197,94,0.3)";
                    ctx.fillRect(legendX + 10, y - 5, 30, 10);
                }
                ctx.strokeStyle = item.color;
                ctx.lineWidth = 2;
                if (item.dashed) ctx.setLineDash([4, 3]);
                else ctx.setLineDash([]);
                ctx.moveTo(legendX + 10, y);
                ctx.lineTo(legendX + 40, y);
                ctx.stroke();
                ctx.setLineDash([]);
                ctx.fillStyle = "rgba(15,23,42,0.8)";
                ctx.font = "11px JetBrains Mono, monospace";
                ctx.textAlign = "left";
                ctx.textBaseline = "middle";
                ctx.fillText(item.label, legendX + 50, y);
            });
            ctx.restore();
        }

        // 当前时刻指示线
        if (curP) {
            const xNow = xOf(curP.t);
            ctx.save();
            ctx.strokeStyle = "rgba(245,158,11,0.7)";
            ctx.lineWidth = isSmall ? 1 : 2;
            ctx.beginPath();
            ctx.moveTo(xNow, chartY);
            ctx.lineTo(xNow, chartY + chartH);
            ctx.stroke();
            ctx.restore();

            // 当前值
            const val = field === "all" ? curP.cwnd : curP[field];
            if (val != null) {
                ctx.save();
                ctx.fillStyle = "rgba(15,23,42,0.7)";
                ctx.font = `${isSmall ? 9 : 13}px JetBrains Mono, monospace`;
                ctx.textAlign = "right";
                ctx.textBaseline = "top";
                const valText = field === "all" 
                    ? `cwnd:${(curP.cwnd/mss).toFixed(1)}  ssthresh:${(curP.ssthresh/mss).toFixed(1)}  inflight:${(curP.inflight/mss).toFixed(1)} pkts`
                    : `${(val / mss).toFixed(1)} pkts`;
                ctx.fillText(valText, ax + aw - (isSmall ? 6 : 16), ay + (isSmall ? 4 : 12));
                ctx.restore();
            }

            // 放大版显示时间
            if (!isSmall) {
                ctx.save();
                ctx.fillStyle = "rgba(245,158,11,0.9)";
                ctx.font = "11px JetBrains Mono, monospace";
                ctx.textAlign = "center";
                ctx.textBaseline = "top";
                ctx.fillText(`t=${fmtMs(curP.t)}`, xNow, chartY + chartH + 6);
                ctx.restore();
            }
        }
    }

    function drawTcpLineInAreaOnCtx(ctx, pts, field, fillColor, strokeColor, dashed, fill, xOf, yOf, mss, cx, cy, cw, ch, lineWidth = 1.5) {
        if (!pts.length) return;
        ctx.save();
        ctx.beginPath();
        ctx.rect(cx, cy, cw, ch);
        ctx.clip();

        if (fill) {
            const baseY = cy + ch;
            ctx.fillStyle = fillColor;
            ctx.beginPath();
            ctx.moveTo(xOf(pts[0].t), baseY);
            for (const p of pts) {
                ctx.lineTo(xOf(p.t), yOf(p[field] ?? 0));
            }
            ctx.lineTo(xOf(pts[pts.length - 1].t), baseY);
            ctx.closePath();
            ctx.fill();
        }

        ctx.strokeStyle = strokeColor;
        ctx.lineWidth = lineWidth;
        if (dashed) ctx.setLineDash([6, 4]);
        ctx.beginPath();
        let first = true;
        for (const p of pts) {
            const x = xOf(p.t);
            const y = yOf(p[field] ?? 0);
            if (first) {
                ctx.moveTo(x, y);
                first = false;
            } else {
                ctx.lineTo(x, y);
            }
        }
        ctx.stroke();
        ctx.restore();
    }

    function drawTcpBoxAt(x, y, text) {
        tcpCtx.save();
        tcpCtx.fillStyle = "rgba(15,23,42,0.6)";
        tcpCtx.font = "12px JetBrains Mono, monospace";
        tcpCtx.textAlign = "center";
        tcpCtx.textBaseline = "middle";
        tcpCtx.fillText(text, x, y);
        tcpCtx.restore();
    }

    function redrawTcpAll() {
        redrawTcp();
        redrawTcpDetails();
    }

    function drawTcpLine(pts, field, color, dashed, xOf, yOfPkts, mss, lineWidth = 2) {
        tcpCtx.save();
        tcpCtx.strokeStyle = color;
        tcpCtx.lineWidth = lineWidth;
        if (dashed) tcpCtx.setLineDash([6, 4]);
        tcpCtx.beginPath();
        let first = true;
        for (const p of pts) {
            const x = xOf(p.t);
            const y = yOfPkts(p[field] / mss);
            if (first) {
                tcpCtx.moveTo(x, y);
                first = false;
            } else {
                tcpCtx.lineTo(x, y);
            }
        }
        tcpCtx.stroke();
        tcpCtx.restore();
    }

    function drawTcpLineWithFill(pts, field, fillColor, strokeColor, xOf, yOfPkts, mss) {
        if (!pts.length) return;
        const baseY = yOfPkts(0);
        tcpCtx.save();
        // 填充区域
        tcpCtx.fillStyle = fillColor;
        tcpCtx.beginPath();
        tcpCtx.moveTo(xOf(pts[0].t), baseY);
        for (const p of pts) {
            tcpCtx.lineTo(xOf(p.t), yOfPkts(p[field] / mss));
        }
        tcpCtx.lineTo(xOf(pts[pts.length - 1].t), baseY);
        tcpCtx.closePath();
        tcpCtx.fill();
        // 边线
        tcpCtx.strokeStyle = strokeColor;
        tcpCtx.lineWidth = 1.5;
        tcpCtx.beginPath();
        let first = true;
        for (const p of pts) {
            const x = xOf(p.t);
            const y = yOfPkts(p[field] / mss);
            if (first) {
                tcpCtx.moveTo(x, y);
                first = false;
            } else {
                tcpCtx.lineTo(x, y);
            }
        }
        tcpCtx.stroke();
        tcpCtx.restore();
    }

    function drawTcpLegend(x, y) {
        const items = [
            { label: "cwnd", color: "#0ea5e9", dashed: false },
            { label: "ssthresh", color: "#ef4444", dashed: true },
            { label: "inflight", color: "#22c55e", dashed: false, fill: true },
        ];
        const lineLen = 20;
        const gap = 8;
        const itemGap = 14;

        tcpCtx.save();
        // 背景
        tcpCtx.fillStyle = "rgba(255,255,255,0.85)";
        tcpCtx.strokeStyle = "rgba(15,23,42,0.2)";
        tcpCtx.lineWidth = 1;
        const boxW = 150;
        const boxH = items.length * itemGap + 8;
        tcpCtx.beginPath();
        tcpCtx.roundRect(x, y, boxW, boxH, 4);
        tcpCtx.fill();
        tcpCtx.stroke();

        tcpCtx.font = "10px JetBrains Mono, monospace";
        tcpCtx.textAlign = "left";
        tcpCtx.textBaseline = "middle";

        items.forEach((item, i) => {
            const ly = y + 8 + i * itemGap;
            const lx = x + 8;
            // 画线
            tcpCtx.strokeStyle = item.color;
            tcpCtx.lineWidth = item.fill ? 1.5 : 2;
            if (item.dashed) tcpCtx.setLineDash([4, 3]);
            else tcpCtx.setLineDash([]);
            tcpCtx.beginPath();
            tcpCtx.moveTo(lx, ly);
            tcpCtx.lineTo(lx + lineLen, ly);
            tcpCtx.stroke();
            // 填充示例
            if (item.fill) {
                tcpCtx.fillStyle = "rgba(34,197,94,0.3)";
                tcpCtx.fillRect(lx, ly - 4, lineLen, 8);
            }
            // 文字
            tcpCtx.setLineDash([]);
            tcpCtx.fillStyle = "#334155";
            tcpCtx.fillText(item.label, lx + lineLen + gap, ly);
        });
        tcpCtx.restore();
    }

    function drawTcpBox(text) {
        tcpCtx.save();
        tcpCtx.fillStyle = "rgba(15,23,42,0.55)";
        tcpCtx.strokeStyle = "rgba(255,255,255,0.12)";
        tcpCtx.lineWidth = 1;
        const x = 14;
        const y = tcpCanvas.height - 52;
        const w = tcpCanvas.width - 28;
        const h = 38;
        rr(tcpCtx, x, y, w, h, 10);
        tcpCtx.fill();
        tcpCtx.stroke();
        tcpCtx.fillStyle = "rgba(255,255,255,0.95)";
        tcpCtx.font = "11px JetBrains Mono, monospace";
        tcpCtx.textAlign = "left";
        tcpCtx.textBaseline = "middle";
        tcpCtx.fillText(text, x + 10, y + h / 2);
        tcpCtx.restore();
    }

    function redrawTcpDetails() {
        if (!tcpDetailCtx || !tcpDetailCanvas) return;
        const ctx = tcpDetailCtx;
        const w = tcpDetailCanvas.width;
        const h = tcpDetailCanvas.height;
        ctx.clearRect(0, 0, w, h);

        rr(ctx, 0.5, 0.5, w - 1, h - 1, 14);
        ctx.fillStyle = "rgba(15,23,42,0.04)";
        ctx.fill();
        ctx.strokeStyle = "rgba(15,23,42,0.12)";
        ctx.stroke();

        const sel = selectTcpConn();
        const cid = sel.cid;
        const ser = sel.ser;
        if (cid == null || !ser) {
            drawDetailBox("Sequence-Time：无 tcp_* / dctcp_cwnd 事件");
            return;
        }
        const seqEvents = ser.seqEvents || [];
        const ackEvents = ser.ackEvents || [];
        const rtoEvents = ser.rtoEvents || [];
        const ackLinks = ser.ackLinks || [];
        const windowPoints = ser.windowPoints || [];
        const rttSeries = ser.rttSeries || [];
        const pts = ser.points || [];
        const mss = ser.mss || 1460;

        if (!seqEvents.length && !ackEvents.length) {
            drawDetailBox(`Sequence-Time：conn=${cid} 无数据`);
            return;
        }

        const pad = { l: 70, r: 18, t: 16, b: 16 };
        const gap = 12;
        const seqHeight = Math.round(h * 0.48);
        const windowHeight = Math.round(h * 0.16);
        const infoHeight = h - pad.t - pad.b - seqHeight - windowHeight - gap * 2;
        const seqArea = { x: pad.l, y: pad.t, w: w - pad.l - pad.r, h: seqHeight };
        const windowArea = { x: pad.l, y: seqArea.y + seqArea.h + gap, w: seqArea.w, h: windowHeight };
        const infoArea = { x: pad.l, y: windowArea.y + windowArea.h + gap, w: seqArea.w, h: infoHeight };

        let minSeq = Infinity;
        let maxSeq = -Infinity;
        for (const s of seqEvents) {
            minSeq = Math.min(minSeq, s.seq);
            maxSeq = Math.max(maxSeq, s.end);
        }
        for (const a of ackEvents) {
            minSeq = Math.min(minSeq, a.ack);
            maxSeq = Math.max(maxSeq, a.ack);
        }
        if (!Number.isFinite(minSeq) || !Number.isFinite(maxSeq)) {
            minSeq = 0;
            maxSeq = mss * 10;
        }
        if (maxSeq - minSeq < mss * 2) {
            maxSeq = minSeq + mss * 4;
        }
        minSeq = Math.max(0, minSeq - mss);
        maxSeq = maxSeq + mss;

        const xOf = (t) => seqArea.x + ((t - state.t0) / Math.max(1, state.t1 - state.t0)) * seqArea.w;
        const yOf = (s) => seqArea.y + (1 - (s - minSeq) / Math.max(1, maxSeq - minSeq)) * seqArea.h;

        ctx.save();
        ctx.strokeStyle = "rgba(15,23,42,0.12)";
        ctx.fillStyle = "rgba(15,23,42,0.75)";
        ctx.font = "10px JetBrains Mono, monospace";
        for (let k = 0; k <= 4; k++) {
            const seq = minSeq + ((maxSeq - minSeq) * k) / 4;
            const y = yOf(seq);
            ctx.beginPath();
            ctx.moveTo(seqArea.x, y);
            ctx.lineTo(seqArea.x + seqArea.w, y);
            ctx.stroke();
            ctx.textAlign = "right";
            ctx.textBaseline = "middle";
            ctx.fillText(fmtBytes(seq), seqArea.x - 10, y);
        }
        for (let k = 0; k <= 4; k++) {
            const t = state.t0 + ((state.t1 - state.t0) * k) / 4;
            const x = xOf(t);
            ctx.beginPath();
            ctx.moveTo(x, seqArea.y);
            ctx.lineTo(x, seqArea.y + seqArea.h);
            ctx.stroke();
            ctx.textAlign = "center";
            ctx.textBaseline = "top";
            ctx.fillText(fmtMs(t), x, seqArea.y + seqArea.h + 4);
        }
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        ctx.fillText(`Sequence-Time（conn=${cid}）`, seqArea.x, Math.max(2, seqArea.y - 12));
        ctx.restore();

        ctx.save();
        ctx.strokeStyle = "rgba(100,116,139,0.55)";
        ctx.lineWidth = 1;
        ctx.setLineDash([4, 4]);
        for (const l of ackLinks) {
            const y = yOf(l.send_seq);
            ctx.beginPath();
            ctx.moveTo(xOf(l.send_t), y);
            ctx.lineTo(xOf(l.ack_t), y);
            ctx.stroke();
        }
        ctx.restore();

        for (const s of seqEvents) {
            const x = xOf(s.t);
            const y1 = yOf(s.seq);
            const y2 = yOf(s.end);
            ctx.save();
            ctx.strokeStyle = s.retrans ? "#f59e0b" : "#0ea5e9";
            ctx.lineWidth = s.retrans ? 3 : 2;
            ctx.beginPath();
            ctx.moveTo(x, y1);
            ctx.lineTo(x, y2);
            ctx.stroke();
            ctx.restore();
        }

        for (const r of rtoEvents) {
            const x = xOf(r.t);
            const y = yOf(r.seq);
            ctx.save();
            ctx.strokeStyle = "#ef4444";
            ctx.lineWidth = 2;
            ctx.beginPath();
            ctx.moveTo(x - 5, y - 5);
            ctx.lineTo(x + 5, y + 5);
            ctx.moveTo(x + 5, y - 5);
            ctx.lineTo(x - 5, y + 5);
            ctx.stroke();
            ctx.restore();
        }

        let lastAckSeen = -Infinity;
        for (const a of ackEvents) {
            const x = xOf(a.t);
            const y = yOf(a.ack);
            const isDup = a.ack <= lastAckSeen;
            if (a.ack > lastAckSeen) lastAckSeen = a.ack;
            ctx.save();
            ctx.fillStyle = a.ecn ? "#ef4444" : "#22c55e";
            ctx.strokeStyle = "rgba(0,0,0,0.25)";
            ctx.lineWidth = 1;
            ctx.beginPath();
            if (isDup) {
                ctx.rect(x - 3, y - 3, 6, 6);
            } else {
                ctx.moveTo(x, y - 5);
                ctx.lineTo(x + 5, y + 5);
                ctx.lineTo(x - 5, y + 5);
                ctx.closePath();
            }
            ctx.fill();
            ctx.stroke();
            ctx.restore();
        }

        const xNow = xOf(state.curTime);
        ctx.save();
        ctx.strokeStyle = "rgba(15,23,42,0.4)";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(xNow, seqArea.y);
        ctx.lineTo(xNow, seqArea.y + seqArea.h);
        ctx.stroke();
        ctx.restore();

        drawWindowBar(windowArea, windowPoints, pts, mss);

        const curP = pickPointAt(pts, state.curTime);
        const stateStr = curP?.state ?? "-";
        const stateArea = { x: infoArea.x, y: infoArea.y, w: Math.min(220, infoArea.w * 0.42), h: infoArea.h };
        const textArea = {
            x: stateArea.x + stateArea.w + 12,
            y: infoArea.y,
            w: Math.max(120, infoArea.w - stateArea.w - 12),
            h: infoArea.h,
        };
        drawStateMachine(stateArea, stateStr);

        const rttP = pickPointAt(rttSeries, state.curTime);
        let lastEcn = null;
        for (const a of ackEvents) {
            if (a.t > state.curTime) break;
            if (a.ecn) lastEcn = a;
        }
        const reasonText = explainTcpReason(stateStr, curP?.reason);
        const alphaText = curP?.alpha != null ? Number(curP.alpha).toFixed(3) : "-";
        const inflightText = curP?.inflight != null ? fmtBytes(curP.inflight) : "-";
        const rttText = rttP ? fmtMs(rttP.rtt) : "-";
        const srttText = rttP ? fmtMs(rttP.srtt) : "-";
        const rtoText = rttP ? fmtMs(rttP.rto) : "-";
        const ecnText = lastEcn ? `ack=${lastEcn.ack} @ ${fmtMs(lastEcn.t)}` : "-";

        ctx.save();
        ctx.fillStyle = "rgba(15,23,42,0.75)";
        ctx.font = "11px JetBrains Mono, monospace";
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        const lines = [
            `state=${stateStr}  inflight=${inflightText}  alpha=${alphaText}`,
            `explain: ${reasonText}`,
            `rtt=${rttText}  srtt=${srttText}  rto=${rtoText}`,
            `ecn_echo: ${ecnText}`,
        ];
        let y = textArea.y;
        for (const line of lines) {
            ctx.fillText(line, textArea.x, y);
            y += 14;
        }
        ctx.restore();
    }

    function drawWindowBar(area, windowPoints, pts, mss) {
        const ctx = tcpDetailCtx;
        const curWin = pickPointAt(windowPoints, state.curTime);
        const curP = pickPointAt(pts, state.curTime);
        if (!curWin || !curP || curP.cwnd == null) {
            ctx.save();
            ctx.fillStyle = "rgba(15,23,42,0.6)";
            ctx.font = "11px JetBrains Mono, monospace";
            ctx.textAlign = "left";
            ctx.textBaseline = "middle";
            ctx.fillText("Send window：无可用数据", area.x, area.y + area.h / 2);
            ctx.restore();
            return;
        }
        const lastAck = Number(curWin.lastAck ?? 0);
        const maxSent = Number(curWin.maxSent ?? lastAck);
        const cwnd = Number(curP.cwnd ?? 0);
        const windowEnd = lastAck + cwnd;
        const inflight = Math.max(0, maxSent - lastAck);
        let minSeq = Math.max(0, lastAck - cwnd * 0.1);
        let maxSeq = Math.max(windowEnd, maxSent, lastAck + mss);
        if (maxSeq <= minSeq) maxSeq = minSeq + mss * 2;
        const range = Math.max(1, maxSeq - minSeq);
        const xOf = (s) => area.x + ((s - minSeq) / range) * area.w;
        const y = area.y + area.h / 2;

        ctx.save();
        ctx.strokeStyle = "rgba(15,23,42,0.25)";
        ctx.lineWidth = 6;
        ctx.lineCap = "round";
        ctx.beginPath();
        ctx.moveTo(area.x, y);
        ctx.lineTo(area.x + area.w, y);
        ctx.stroke();

        ctx.strokeStyle = "rgba(15,23,42,0.15)";
        ctx.lineWidth = 10;
        ctx.beginPath();
        ctx.moveTo(xOf(lastAck), y);
        ctx.lineTo(xOf(windowEnd), y);
        ctx.stroke();

        ctx.strokeStyle = "#0ea5e9";
        ctx.lineWidth = 10;
        ctx.beginPath();
        ctx.moveTo(xOf(lastAck), y);
        ctx.lineTo(xOf(Math.min(maxSent, windowEnd)), y);
        ctx.stroke();

        ctx.strokeStyle = "#0f172a";
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(xOf(lastAck), y - 10);
        ctx.lineTo(xOf(lastAck), y + 10);
        ctx.moveTo(xOf(windowEnd), y - 10);
        ctx.lineTo(xOf(windowEnd), y + 10);
        ctx.stroke();

        ctx.fillStyle = "rgba(15,23,42,0.75)";
        ctx.font = "10px JetBrains Mono, monospace";
        ctx.textAlign = "center";
        ctx.textBaseline = "bottom";
        ctx.fillText("last_ack", xOf(lastAck), y - 12);
        ctx.fillText("win_end", xOf(windowEnd), y - 12);

        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        ctx.fillText(`send window=${fmtBytes(cwnd)}  inflight=${fmtBytes(inflight)}`, area.x, area.y + area.h + 2);
        ctx.restore();
    }

    function drawStateMachine(area, stateStr) {
        const ctx = tcpDetailCtx;
        const nodes = ["SS", "CA", "FR", "RTO"];
        const gap = 10;
        const w = Math.min(70, (area.w - gap) / 2);
        const h = 22;
        const x0 = area.x;
        const y0 = area.y;
        const pos = {
            SS: { x: x0, y: y0 },
            CA: { x: x0 + w + gap, y: y0 },
            FR: { x: x0, y: y0 + h + gap },
            RTO: { x: x0 + w + gap, y: y0 + h + gap },
        };

        ctx.save();
        ctx.strokeStyle = "rgba(15,23,42,0.25)";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(pos.SS.x + w, pos.SS.y + h / 2);
        ctx.lineTo(pos.CA.x, pos.CA.y + h / 2);
        ctx.moveTo(pos.SS.x + w / 2, pos.SS.y + h);
        ctx.lineTo(pos.FR.x + w / 2, pos.FR.y);
        ctx.moveTo(pos.CA.x + w / 2, pos.CA.y + h);
        ctx.lineTo(pos.RTO.x + w / 2, pos.RTO.y);
        ctx.moveTo(pos.FR.x + w, pos.FR.y + h / 2);
        ctx.lineTo(pos.RTO.x, pos.RTO.y + h / 2);
        ctx.stroke();
        ctx.restore();

        for (const k of nodes) {
            const p = pos[k];
            const active = stateStr === k;
            ctx.save();
            ctx.fillStyle = active ? "rgba(14,116,144,0.25)" : "rgba(255,255,255,0.85)";
            ctx.strokeStyle = active ? "rgba(14,116,144,0.85)" : "rgba(15,23,42,0.25)";
            ctx.lineWidth = 1.5;
            rr(ctx, p.x, p.y, w, h, 6);
            ctx.fill();
            ctx.stroke();
            ctx.fillStyle = "rgba(15,23,42,0.8)";
            ctx.font = "11px JetBrains Mono, monospace";
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText(k, p.x + w / 2, p.y + h / 2);
            ctx.restore();
        }

        ctx.save();
        ctx.fillStyle = "rgba(15,23,42,0.65)";
        ctx.font = "10px JetBrains Mono, monospace";
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        const label = stateStr === "DCTCP" ? "DCTCP" : "Reno 状态机";
        ctx.fillText(label, area.x, area.y + h * 2 + gap + 4);
        ctx.restore();
    }

    function explainTcpReason(stateStr, reason) {
        if (stateStr === "DCTCP") return "DCTCP 采样窗口，按 ECN 比例调整 cwnd";
        const map = {
            init: "连接初始窗口",
            send_data: "发送数据段，inflight 增加",
            new_ack: "收到新 ACK，窗口右移 / cwnd 增长",
            dupack: "重复 ACK，可能存在丢包",
            "3_dupack": "3 次重复 ACK，进入快速重传",
            more_dupack: "更多重复 ACK，快速恢复继续增窗",
            rto: "RTO 超时，cwnd 收缩并重传",
            send_ack: "对端发送 ACK",
            sample: "DCTCP 采样点",
        };
        return map[reason] || "-";
    }

    function drawDetailBox(text) {
        const ctx = tcpDetailCtx;
        ctx.save();
        ctx.fillStyle = "rgba(15,23,42,0.55)";
        ctx.strokeStyle = "rgba(255,255,255,0.12)";
        ctx.lineWidth = 1;
        const x = 14;
        const y = tcpDetailCanvas.height - 52;
        const w = tcpDetailCanvas.width - 28;
        const h = 38;
        rr(ctx, x, y, w, h, 10);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = "rgba(255,255,255,0.95)";
        ctx.font = "11px JetBrains Mono, monospace";
        ctx.textAlign = "left";
        ctx.textBaseline = "middle";
        ctx.fillText(text, x + 10, y + h / 2);
        ctx.restore();
    }

    function rr(c, x, y, w, h, r) {
        if (typeof c.roundRect === "function") {
            c.beginPath();
            c.roundRect(x, y, w, h, r);
            return;
        }
        const rr = Math.min(r, w / 2, h / 2);
        c.beginPath();
        c.moveTo(x + rr, y);
        c.arcTo(x + w, y, x + w, y + h, rr);
        c.arcTo(x + w, y + h, x, y + h, rr);
        c.arcTo(x, y + h, x, y, rr);
        c.arcTo(x, y, x + w, y, rr);
        c.closePath();
    }

    function applyLayout() {
        const width = netCanvas?.width || 1100;
        const height = netCanvas?.height || 360;
        const metaNodes = state.meta?.nodes;
        const nodesList = metaNodes && Array.isArray(metaNodes) ? metaNodes.slice().sort((a, b) => a.id - b.id) : defaultNodes;
        const linksList = state.meta?.links || defaultLinks;
        let layout = null;
        if (state.layoutChoice === "fat-tree") {
            layout = layoutFatTree(nodesList, width, height) || layoutCircle(nodesList, width, height);
        } else if (state.layoutChoice === "dumbbell") {
            layout = layoutDumbbell(nodesList, linksList, width, height) || layoutCircle(nodesList, width, height);
        } else if (state.layoutChoice === "circle") {
            layout = layoutCircle(nodesList, width, height);
        } else {
            layout = layoutFatTree(nodesList, width, height);
            if (!layout) layout = layoutDumbbell(nodesList, linksList, width, height);
            if (!layout) layout = layoutCircle(nodesList, width, height);
        }
        state.nodes = layout.nodes;
        state.nodeScale = layout.scale;
        state.nodeById = new Map(state.nodes.map((n) => [n.id, n]));
        const pairs = buildLinkPairs(state.meta?.links);
        state.drawLinks = pairs.length ? pairs : defaultLinks.slice();
        state.layoutDetected = layout.kind;
    }

    function onFile(evt) {
        const f = evt.target.files?.[0];
        if (!f) return;
        f.text().then((text) => {
            try {
                const arr = JSON.parse(text);
                if (!Array.isArray(arr)) throw new Error("JSON 顶层不是数组");
                const metas = arr.filter((e) => e && e.kind === "meta");
                state.meta = metas.length ? metas[0] : null;
                state.events = arr.filter((e) => e && e.kind !== "meta");
                state.events.sort((a, b) => (a.t_ns ?? 0) - (b.t_ns ?? 0));
                state.t0 = state.events.length ? state.events[0].t_ns : 0;
                state.t1 = state.events.length ? state.events[state.events.length - 1].t_ns : 0;
                state.filtered = state.events;
                applyLayout();
                state.playing = false;
                state.lastWall = 0;
                resetPlayback();
            } catch (e) {
                state.curText = "解析失败：" + String(e);
            }
        });
    }

    function jumpToDrop() {
        if (!state.filtered.length) return;
        const flow = state.filterFlow.trim();
        const flowN = flow === "" ? null : Number(flow);
        const idx = state.filtered.findIndex(
            (e) => e && e.kind === "drop" && (e.t_ns ?? 0) > state.curTime && (flowN == null || Number(e.flow_id) === flowN)
        );
        if (idx < 0) return;
        state.playing = false;
        const t = state.filtered[idx].t_ns ?? state.t0;
        setTime(t);
        state.inflight = new Map();
        state.nodeHighlight = new Map();
        state.dropMarks = [];
        state.lastEventsText = [];
        state.nodeStats = new Map();
        state.linkStats = new Map();
        state.tcpStats = { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 };
        state.tcpTrack = new Map();
        state.tcpSeries = buildTcpSeries(state.events);
        updateConnPick();
        initStatsFromMeta();
        state.cursor = 0;
        applyUntil(state.curTime);
        redraw();
        showCurrentText();
        showStatsText();
        redrawTcpAll();
    }

    function play() {
        if (!state.filtered.length) return;
        state.playing = true;
        state.lastWall = 0;
    }

    function pause() {
        state.playing = false;
    }

    function step() {
        state.playing = false;
        if (!state.filtered.length) return;
        const ev = state.filtered[state.cursor];
        if (!ev) return;
        setTime(ev.t_ns);
        applyUntil(ev.t_ns);
        redraw();
        showCurrentText();
        showStatsText();
        redrawTcpAll();
    }

    function onSlider() {
        if (!state.filtered.length) return;
        state.playing = false;
        const p = Number(state.slider) / 1000;
        const t = state.t0 + (state.t1 - state.t0) * p;
        setTime(t);
        applyUntil(state.curTime);
        redraw();
        showCurrentText();
        showStatsText();
        redrawTcpAll();
    }

    function tick(ts) {
        rafId = requestAnimationFrame(tick);
        if (!state.playing || !state.filtered.length) return;
        if (!state.lastWall) state.lastWall = ts;
        const dt = ts - state.lastWall;
        state.lastWall = ts;
        const targetWallMs = Number(state.targetWallSec || 12) * 1000;
        const simDur = Math.max(1, state.t1 - state.t0);
        const advance = (dt * (simDur / targetWallMs)) * Number(state.speed || 1);
        const target = Math.min(state.t1, state.curTime + advance);
        setTime(target);
        applyUntil(state.curTime);
        redraw();
        showCurrentText();
        showStatsText();
        redrawTcpAll();
        if (state.curTime >= state.t1) state.playing = false;
    }

    function handleKeydown(e) {
        if (e.key === " ") {
            e.preventDefault();
            state.playing = !state.playing;
            state.lastWall = 0;
        } else if (e.key === "ArrowRight") {
            e.preventDefault();
            state.playing = false;
            step();
        } else if (e.key === "ArrowLeft") {
            e.preventDefault();
            state.playing = false;
            if (!state.filtered.length) return;
            const idx = Math.max(0, state.cursor - 2);
            const t = state.filtered[idx]?.t_ns ?? state.t0;
            setTime(t);
            state.inflight = new Map();
            state.nodeHighlight = new Map();
            state.dropMarks = [];
            state.lastEventsText = [];
            state.nodeStats = new Map();
            state.linkStats = new Map();
            state.tcpStats = { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 };
            state.tcpTrack = new Map();
            initStatsFromMeta();
            state.cursor = 0;
            applyUntil(state.curTime);
            redraw();
            showCurrentText();
            showStatsText();
            redrawTcpAll();
        }
    }

    onMounted(() => {
        rafId = requestAnimationFrame(tick);
        window.addEventListener("keydown", handleKeydown);
    });

    onBeforeUnmount(() => {
        if (rafId) cancelAnimationFrame(rafId);
        window.removeEventListener("keydown", handleKeydown);
    });

    watch(() => state.filterFlow, applyFilter);
    watch(() => state.filterPkt, applyFilter);
    watch(() => state.layoutChoice, () => {
        if (!state.events.length) return;
        applyLayout();
        resetPlayback();
    });
    watch(() => state.connPick, () => redrawTcpAll());

    function setNetCanvas(el) {
        if (!el) {
            netCanvas = null;
            netCtx = null;
            return;
        }
        netCanvas = el;
        netCtx = el.getContext("2d");
        applyLayout();
        resetPlayback();
    }

    function setTcpCanvas(el) {
        if (!el) {
            if (tcpCanvas) {
                tcpCanvas.removeEventListener("click", onTcpCanvasClick);
            }
            tcpCanvas = null;
            tcpCtx = null;
            return;
        }
        tcpCanvas = el;
        tcpCtx = el.getContext("2d");
        el.addEventListener("click", onTcpCanvasClick);
        el.style.cursor = "pointer";
        redrawTcpAll();
    }

    function setTcpCardRef(ref) {
        tcpCardRef = ref;
    }

    function onTcpCanvasClick(e) {
        if (!tcpCanvas || !tcpCardRef) return;
        // 点击任意位置都打开放大的 4 子图视图
        tcpCardRef.openModal();
    }

    function setTcpModalCanvas(el) {
        if (!el) {
            tcpModalCanvas = null;
            tcpModalCtx = null;
            return;
        }
        tcpModalCanvas = el;
        tcpModalCtx = el.getContext("2d");
        redrawTcpModal();
    }

    function onTcpModalClose() {
        tcpModalCanvas = null;
        tcpModalCtx = null;
    }

    function redrawTcpModal() {
        if (!tcpModalCtx || !tcpModalCanvas) return;
        const ctx = tcpModalCtx;
        const canvas = tcpModalCanvas;
        ctx.clearRect(0, 0, canvas.width, canvas.height);

        const sel = selectTcpConn();
        const cid = sel.cid;
        const ser = sel.ser;
        if (cid == null || !ser) return;
        const pts = ser?.points || [];
        const mss = ser?.mss || 1460;
        if (!pts.length) return;

        const curP = pickPointAt(pts, state.curTime);

        // 2x2 子图布局（放大版）
        const gap = 20;
        const subW = Math.floor((canvas.width - gap) / 2);
        const subH = Math.floor((canvas.height - gap) / 2);
        const areas = [
            { x: 0, y: 0, w: subW, h: subH, title: "cwnd（拥塞窗口）", field: "cwnd", color: "#0ea5e9", fill: false },
            { x: subW + gap, y: 0, w: subW, h: subH, title: "ssthresh（慢启动阈值）", field: "ssthresh", color: "#ef4444", fill: false },
            { x: 0, y: subH + gap, w: subW, h: subH, title: "inflight（在途数据）", field: "inflight", color: "#22c55e", fill: true },
            { x: subW + gap, y: subH + gap, w: subW, h: subH, title: "三者对比", field: "all", color: "", fill: false },
        ];

        for (const area of areas) {
            drawTcpSubChartOnCtx(ctx, canvas, pts, mss, area, cid, curP, false);
        }
    }

    function setTcpDetailCanvas(el) {
        if (!el) {
            tcpDetailCanvas = null;
            tcpDetailCtx = null;
            return;
        }
        tcpDetailCanvas = el;
        tcpDetailCtx = el.getContext("2d");
        redrawTcpDetails();
    }

    return {
        state,
        computed: {
            hasEvents,
            canPlay,
            metaNodesCount,
            metaLinksCount,
            layoutDetectedLabel,
            topologyStatus,
            statusText,
        },
        actions: {
            onFile,
            play,
            pause,
            step,
            jumpToDrop,
            onSlider,
            setNetCanvas,
            setTcpCanvas,
            setTcpCardRef,
            setTcpModalCanvas,
            onTcpModalClose,
            setTcpDetailCanvas,
        },
    };
}
