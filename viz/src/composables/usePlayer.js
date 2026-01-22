import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { fmtBytes, fmtCapBytes, fmtGbps, fmtMs } from "../utils/format";
import { createNetRenderer } from "./player/net";
import { createPlayerState } from "./player/state";
import { createTcpController } from "./player/tcp";

export function usePlayer() {
    const state = createPlayerState();
    const net = createNetRenderer(state);
    const tcp = createTcpController(state);
    let rafId = 0;
    const cwndReasonCatalog = [
        { reason: "init", label: "初始化" },
        { reason: "ack_slow_start", label: "慢启动" },
        { reason: "ack_congestion_avoidance", label: "拥塞避免" },
        { reason: "fast_recovery_enter", label: "快速恢复进入" },
        { reason: "fast_recovery_dup_ack", label: "快速恢复 dupACK" },
        { reason: "fast_recovery_partial_ack", label: "快速恢复部分 ACK" },
        { reason: "fast_recovery_exit", label: "快速恢复退出" },
        { reason: "dup_ack_3", label: "3 次 dupACK" },
        { reason: "dup_ack_more", label: "更多 dupACK" },
        { reason: "rto_timeout", label: "RTO 超时" },
        { reason: "dctcp_ecn_window", label: "DCTCP ECN 窗口" },
        { reason: "sample", label: "采样" },
    ];
    const eventTypeCatalog = [
        { kind: "dctcp_cwnd", label: "窗口调整（总）", group: "cwnd" },
        ...cwndReasonCatalog.map((item) => ({
            kind: `cwnd_reason:${item.reason}`,
            label: `窗口调整/${item.label}`,
            group: "cwnd_reason",
            reason: item.reason,
        })),
        { kind: "base_all", label: "基础事件（总）", group: "base" },
        { kind: "drop", label: "链路丢包", group: "base" },
        { kind: "enqueue", label: "链路入队", group: "base" },
        { kind: "tx_start", label: "链路发送", group: "base" },
        { kind: "node_rx", label: "节点接收", group: "base" },
        { kind: "node_forward", label: "节点转发", group: "base" },
        { kind: "delivered", label: "交付完成", group: "base" },
        { kind: "tcp_send_data", label: "TCP 发送数据", group: "base" },
        { kind: "tcp_send_ack", label: "TCP 发送 ACK", group: "base" },
        { kind: "tcp_recv_ack", label: "TCP 接收 ACK", group: "base" },
        { kind: "tcp_rto", label: "TCP RTO", group: "base" },
        { kind: "arrive_node", label: "到达节点", group: "base" },
    ];
    const baseKinds = eventTypeCatalog.filter((item) => item.group === "base").map((item) => item.kind);
    const baseKindsNoAll = baseKinds.filter((kind) => kind !== "base_all");
    const cwndReasonKinds = cwndReasonCatalog.map((item) => `cwnd_reason:${item.reason}`);
    for (const item of eventTypeCatalog) {
        if (state.eventTypeFilter[item.kind] == null) {
            state.eventTypeFilter[item.kind] = false;
        }
    }

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
    const focusView = computed(() => buildFocusView());
    const eventTypeFilters = computed(() => state.eventTypeFilter);
    const tcpAdjustMarks = computed(() => {
        if (!state.filtered.length) return [];
        if (state.eventTypeFilter.dctcp_cwnd) return [];
        const t0 = state.t0;
        const t1 = state.t1;
        const span = Math.max(1, t1 - t0);
        const events = state.filtered.filter((e) => e && e.kind === "dctcp_cwnd" && isEventVisible(e));
        if (!events.length) return [];
        const maxMarks = 360;
        const step = Math.max(1, Math.ceil(events.length / maxMarks));
        const marks = [];
        for (let i = 0; i < events.length; i += step) {
            const t = Number(events[i].t_ns ?? t0);
            const pos = (t - t0) / span;
            marks.push(Math.min(1, Math.max(0, pos)));
        }
        return marks;
    });

    function setTime(t) {
        state.curTime = Math.max(state.t0, Math.min(state.t1, t));
        const p = (state.curTime - state.t0) / Math.max(1, state.t1 - state.t0);
        state.slider = Math.floor(p * 1000);
    }

    function resetRuntimeStats() {
        state.inflight = new Map();
        state.nodeHighlight = new Map();
        state.dropMarks = [];
        state.lastEventsText = [];
        state.nodeStats = new Map();
        state.linkStats = new Map();
        tcp.resetTracking();
        net.initStatsFromMeta();
    }

    function applyUntil(t) {
        if (!state.filtered.length) return;
        if (state.cursor > 0 && state.cursor < state.filtered.length && state.filtered[state.cursor - 1].t_ns > t) {
            resetRuntimeStats();
            state.cursor = 0;
        }
        while (state.cursor < state.filtered.length && state.filtered[state.cursor].t_ns <= t) {
            const ev = state.filtered[state.cursor++];
            applyEvent(ev);
        }
        updateFocusEvents();
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

    function updateFocusEvents() {
        if (!state.filtered.length) {
            state.focusEvents = [];
            return;
        }
        const window = Math.max(0, Number(state.focusWindowNs || 0));
        const tEnd = state.curTime;
        const tStart = Math.max(state.t0, tEnd - window);
        let start = Math.min(state.cursor, state.filtered.length);
        while (start > 0 && (state.filtered[start - 1].t_ns ?? 0) >= tStart) {
            start -= 1;
        }
        let end = Math.min(state.cursor, state.filtered.length);
        while (end < state.filtered.length && (state.filtered[end].t_ns ?? 0) <= tEnd) {
            end += 1;
        }
        state.focusEvents = state.filtered.slice(start, end);
    }

    function cwndReasonKey(reason) {
        return `cwnd_reason:${reason || "sample"}`;
    }

    function isEventVisible(ev) {
        if (!ev || !ev.kind) return true;
        if (ev.kind === "dctcp_cwnd") {
            const reasonKey = cwndReasonKey(ev.reason);
            if (state.eventTypeFilter.dctcp_cwnd) {
                return !state.eventTypeFilter[reasonKey];
            }
            if (state.eventTypeFilter[reasonKey]) return false;
            return true;
        }
        return !state.eventTypeFilter[ev.kind];
    }

    function nodeLabel(id) {
        const n = state.nodeById.get(Number(id));
        if (!n) return `节点${id}`;
        return `${n.name}(${n.kind})`;
    }

    function linkLabel(from, to) {
        const a = state.nodeById.get(Number(from));
        const b = state.nodeById.get(Number(to));
        const left = a ? a.name : String(from);
        const right = b ? b.name : String(to);
        return `${left}→${right}`;
    }

    function fmtQueue(ev) {
        if (ev.q_bytes == null && ev.q_cap_bytes == null) return null;
        const q = ev.q_bytes != null ? fmtBytes(ev.q_bytes) : "-";
        const cap = ev.q_cap_bytes != null ? fmtCapBytes(ev.q_cap_bytes) : "-";
        return `队列 ${q}/${cap}`;
    }

    function pickPrevPoint(pts, t) {
        let lo = 0;
        let hi = pts.length - 1;
        let ans = null;
        while (lo <= hi) {
            const mid = (lo + hi) >> 1;
            if (pts[mid].t < t) {
                ans = pts[mid];
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        return ans;
    }

    function fmtDelta(label, prevVal, curVal) {
        if (curVal == null) return null;
        if (prevVal == null || prevVal === curVal) return `${label}=${fmtBytes(curVal)}`;
        return `${label} ${fmtBytes(prevVal)} → ${fmtBytes(curVal)}`;
    }

    function cwndReasonText(reason) {
        const map = {
            init: "初始化拥塞窗口",
            ack_slow_start: "慢启动：收到新 ACK，增大 cwnd",
            ack_congestion_avoidance: "拥塞避免：ACK 驱动 AIMD 增长",
            fast_recovery_enter: "3 次 dupACK，进入快速恢复",
            fast_recovery_dup_ack: "快速恢复中 dupACK，膨胀窗口",
            fast_recovery_partial_ack: "快速恢复中部分 ACK，调整窗口",
            fast_recovery_exit: "快速恢复结束，窗口收敛",
            dup_ack_3: "3 次 dupACK，触发快速重传",
            dup_ack_more: "更多 dupACK，继续膨胀窗口",
            rto_timeout: "RTO 超时，窗口回到慢启动",
            dctcp_ecn_window: "DCTCP 采样窗口末端，依据 ECN 比例收缩",
            sample: "窗口采样",
        };
        return map[reason] || "";
    }

    function cwndReasonLabel(reason) {
        const entry = cwndReasonCatalog.find((item) => item.reason === reason);
        return entry ? entry.label : "";
    }

    function describeEvent(ev) {
        const kind = ev.kind || "unknown";
        const time = fmtMs(ev.t_ns ?? state.curTime);
        const pkt = ev.pkt_id != null ? `pkt=${ev.pkt_id}` : null;
        const flow = ev.flow_id != null ? `flow=${ev.flow_id}` : null;
        const conn = ev.conn_id != null ? `conn=${ev.conn_id}` : ev.flow_id != null ? `conn=${ev.flow_id}` : null;
        const parts = (items) => items.filter(Boolean).join("，");
        let title = `事件 ${kind}`;
        let detail = parts([pkt, flow]);
        let category = "other";
        let severity = "";
        let note = "";
        let reasonLabel = "";

        if (kind === "tx_start") {
            title = "链路开始发送";
            category = "link";
            const link = `链路 ${linkLabel(ev.link_from, ev.link_to)}`;
            const depart = ev.depart_ns != null ? `出发=${fmtMs(ev.depart_ns)}` : null;
            const arrive = ev.arrive_ns != null ? `到达=${fmtMs(ev.arrive_ns)}` : null;
            detail = parts([link, pkt, flow, depart, arrive]);
        } else if (kind === "enqueue") {
            title = "链路入队";
            category = "link";
            const link = `链路 ${linkLabel(ev.link_from, ev.link_to)}`;
            detail = parts([link, pkt, flow, fmtQueue(ev)]);
        } else if (kind === "drop") {
            title = "链路丢包";
            category = "link";
            severity = "critical";
            const link = `链路 ${linkLabel(ev.link_from, ev.link_to)}`;
            detail = parts([link, pkt, flow, fmtQueue(ev)]);
        } else if (kind === "node_rx") {
            title = "节点接收数据包";
            category = "node";
            const node = nodeLabel(ev.node);
            const bytes = ev.pkt_bytes != null ? `大小=${fmtBytes(ev.pkt_bytes)}` : null;
            detail = parts([node, pkt, flow, bytes]);
        } else if (kind === "node_forward") {
            title = "节点转发决策";
            category = "node";
            const node = nodeLabel(ev.node);
            const next = ev.next != null ? `下一跳=${ev.next}` : null;
            detail = parts([node, pkt, flow, next]);
        } else if (kind === "delivered") {
            title = "目的节点交付";
            category = "node";
            const node = nodeLabel(ev.node);
            detail = parts([node, pkt, flow]);
        } else if (kind === "tcp_send_data") {
            title = ev.retrans ? "TCP 数据段重传" : "TCP 发送数据段";
            category = "tcp";
            severity = ev.retrans ? "warn" : "";
            const seq = ev.seq != null ? `seq=${ev.seq}` : null;
            const len = ev.len != null ? `len=${ev.len}` : null;
            detail = parts([conn, seq, len]);
        } else if (kind === "tcp_send_ack") {
            title = "TCP 发送 ACK";
            category = "tcp";
            const ack = ev.ack != null ? `ack=${ev.ack}` : null;
            detail = parts([conn, ack]);
        } else if (kind === "tcp_recv_ack") {
            title = "TCP 收到 ACK";
            category = "tcp";
            const ack = ev.ack != null ? `ack=${ev.ack}` : null;
            const ecn = ev.ecn_echo ? "ecn_echo=1" : null;
            detail = parts([conn, ack, ecn]);
        } else if (kind === "tcp_rto") {
            title = "TCP RTO 超时";
            category = "tcp";
            severity = "critical";
            const seq = ev.seq != null ? `seq=${ev.seq}` : null;
            detail = parts([conn, seq]);
        } else if (kind === "dctcp_cwnd") {
            title = "拥塞窗口更新";
            category = "cwnd";
            const cid = Number(ev.conn_id ?? ev.flow_id ?? 0) || null;
            const cwnd = ev.cwnd_bytes != null ? Number(ev.cwnd_bytes) : null;
            const ssthresh = ev.ssthresh_bytes != null ? Number(ev.ssthresh_bytes) : null;
            const inflight = ev.inflight_bytes != null ? Number(ev.inflight_bytes) : null;
            const alpha = ev.alpha != null ? Number(ev.alpha) : null;
            let prev = null;
            if (cid && state.tcpSeries.has(cid)) {
                const pts = state.tcpSeries.get(cid)?.points || [];
                prev = pickPrevPoint(pts, Number(ev.t_ns ?? 0));
            }
            const cwndText = fmtDelta("cwnd", prev?.cwnd ?? null, cwnd);
            const ssthreshText = fmtDelta("ssthresh", prev?.ssthresh ?? null, ssthresh);
            const inflightText = fmtDelta("inflight", prev?.inflight ?? null, inflight);
            const alphaText = alpha != null ? `α=${alpha.toFixed(3)}` : null;
            detail = parts([conn, cwndText, ssthreshText, inflightText, alphaText]);
            const reasonText = cwndReasonText(ev.reason);
            reasonLabel = cwndReasonLabel(ev.reason);
            const extra = [];
            if (ev.acked_bytes != null) extra.push(`新确认 ${fmtBytes(ev.acked_bytes)}`);
            if (ev.dup_acks != null) extra.push(`dupACK=${ev.dup_acks}`);
            if (ev.ecn_frac != null) extra.push(`ECN比例=${Number(ev.ecn_frac).toFixed(3)}`);
            const suffix = extra.length ? `（${extra.join("，")}）` : "";
            note = reasonText ? `依据：${reasonText}${suffix}` : "";
        } else if (kind.startsWith("tcp_") || kind.startsWith("dctcp_")) {
            title = "TCP 状态更新";
            category = "tcp";
            const seq = ev.seq != null ? `seq=${ev.seq}` : null;
            const ack = ev.ack != null ? `ack=${ev.ack}` : null;
            detail = parts([conn, seq, ack]);
        }

        return { kind, time, title, detail: detail || "（无细节）", category, severity, note, reasonLabel };
    }

    function buildFocusView() {
        const windowLabel = `t=${fmtMs(state.curTime)} · 最近 ${fmtMs(state.focusWindowNs)}`;
        if (!hasEvents.value) {
            return {
                windowLabel,
                total: 0,
                highlights: ["尚未加载事件数据。"],
                groups: [],
                empty: true,
            };
        }
        const visibleEvents = state.focusEvents.filter((ev) => isEventVisible(ev));
        if (!visibleEvents.length) {
            return {
                windowLabel,
                total: 0,
                highlights: ["当前时间片没有事件或已被过滤。"],
                groups: [],
                empty: true,
            };
        }

        const items = visibleEvents.map((ev) => ({
            ...describeEvent(ev),
            isPrimary: (ev.t_ns ?? 0) === state.curTime,
        }));
        const groups = {
            cwnd: [],
            link: [],
            node: [],
            tcp: [],
            other: [],
        };
        for (const item of items) {
            groups[item.category]?.push(item);
        }

        const dropCount = visibleEvents.filter((e) => e.kind === "drop").length;
        const rtoCount = visibleEvents.filter((e) => e.kind === "tcp_rto").length;
        const retransCount = visibleEvents.filter((e) => e.kind === "tcp_send_data" && e.retrans === true).length;
        const deliveredCount = visibleEvents.filter((e) => e.kind === "delivered").length;
        const nodeRxCount = visibleEvents.filter((e) => e.kind === "node_rx").length;

        const highlights = [];
        if (dropCount) highlights.push(`链路丢包 ${dropCount} 次`);
        if (rtoCount) highlights.push(`TCP RTO 超时 ${rtoCount} 次`);
        if (retransCount) highlights.push(`TCP 重传 ${retransCount} 次`);
        if (deliveredCount) highlights.push(`成功交付 ${deliveredCount} 包`);
        if (nodeRxCount) highlights.push(`节点接收 ${nodeRxCount} 包`);
        if (!highlights.length) {
            highlights.push("暂无丢包/RTO/重传，主要为正常收发与转发。");
        }

        const order = [
            { id: "cwnd", title: "窗口调整" },
            { id: "link", title: "链路事件" },
            { id: "node", title: "节点事件" },
            { id: "tcp", title: "TCP 事件" },
            { id: "other", title: "其他事件" },
        ];

        const groupList = order
            .map((g) => ({ ...g, items: groups[g.id], count: groups[g.id].length }))
            .filter((g) => g.items.length);

        return {
            windowLabel,
            total: items.length,
            highlights,
            groups: groupList,
            empty: false,
        };
    }

    function resetToTime(t, options = {}) {
        const { rebuildSeries = false, apply = true } = options;
        setTime(t);
        resetRuntimeStats();
        if (rebuildSeries) tcp.rebuildSeries(state.events);
        state.cursor = 0;
        if (apply) applyUntil(state.curTime);
        updateFocusEvents();
        net.redraw();
        showCurrentText();
        showStatsText();
        tcp.redrawAll();
    }

    function resetPlayback() {
        resetToTime(state.t0, { rebuildSeries: true, apply: false });
    }

    function applyFilter(options = {}) {
        const { preserveTime = false } = options;
        const flow = state.filterFlow.trim();
        const pkt = state.filterPkt.trim();
        const flowN = flow === "" ? null : Number(flow);
        const pktN = pkt === "" ? null : Number(pkt);
        state.filtered = state.events.filter((e) => {
            if (flowN != null && (e.flow_id == null || Number(e.flow_id) !== flowN)) return false;
            if (pktN != null && (e.pkt_id == null || Number(e.pkt_id) !== pktN)) return false;
            if (!isEventVisible(e)) return false;
            return true;
        });
        if (preserveTime) {
            resetToTime(state.curTime, { rebuildSeries: true });
        } else {
            resetPlayback();
        }
    }

    function applyEvent(ev) {
        const handledNet = net.applyEvent(ev);
        const handledTcp = tcp.applyEvent(ev);
        if (handledNet || handledTcp) return;
        const kind = ev.kind;
        const head = `[${fmtMs(ev.t_ns)}] ${kind}`;
        state.lastEventsText.push(`${head} pkt=${ev.pkt_id ?? "-"}`);
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
                net.applyLayout();
                state.playing = false;
                state.lastWall = 0;
                applyFilter({ preserveTime: false });
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
        resetToTime(t, { rebuildSeries: true });
    }

    function jumpToTcpAdjust() {
        if (!state.filtered.length) return;
        const idx = state.filtered.findIndex((e) => e && e.kind === "dctcp_cwnd" && (e.t_ns ?? 0) > state.curTime);
        if (idx < 0) return;
        state.playing = false;
        const t = state.filtered[idx].t_ns ?? state.t0;
        resetToTime(t, { rebuildSeries: true });
    }

    function jumpToTcpAdjustDifferent() {
        if (!state.filtered.length) return;
        let baseReason = null;
        for (let i = Math.min(state.cursor - 1, state.filtered.length - 1); i >= 0; i -= 1) {
            const ev = state.filtered[i];
            if (ev && ev.kind === "dctcp_cwnd") {
                baseReason = ev.reason || "sample";
                break;
            }
        }
        let idx = -1;
        for (let i = Math.max(0, state.cursor); i < state.filtered.length; i += 1) {
            const ev = state.filtered[i];
            if (!ev || ev.kind !== "dctcp_cwnd") continue;
            const nextReason = ev.reason || "sample";
            if (baseReason == null || nextReason !== baseReason) {
                idx = i;
                break;
            }
        }
        if (idx < 0) return;
        state.playing = false;
        const t = state.filtered[idx].t_ns ?? state.t0;
        resetToTime(t, { rebuildSeries: true });
    }

    function toggleEventKind(kind) {
        if (!kind) return;
        if (kind === "dctcp_cwnd") {
            const next = !state.eventTypeFilter.dctcp_cwnd;
            state.eventTypeFilter.dctcp_cwnd = next;
            for (const k of cwndReasonKinds) {
                state.eventTypeFilter[k] = next;
            }
            applyFilter({ preserveTime: true });
            return;
        }
        if (kind === "base_all") {
            const next = !state.eventTypeFilter.base_all;
            state.eventTypeFilter.base_all = next;
            for (const k of baseKindsNoAll) {
                state.eventTypeFilter[k] = next;
            }
            applyFilter({ preserveTime: true });
            return;
        }
        state.eventTypeFilter[kind] = !state.eventTypeFilter[kind];
        if (baseKindsNoAll.includes(kind)) {
            const allOff = baseKindsNoAll.every((k) => state.eventTypeFilter[k]);
            state.eventTypeFilter.base_all = allOff;
        }
        applyFilter({ preserveTime: true });
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
        net.redraw();
        showCurrentText();
        showStatsText();
        tcp.redrawAll();
    }

    function onSlider() {
        if (!state.filtered.length) return;
        state.playing = false;
        const p = Number(state.slider) / 1000;
        const t = state.t0 + (state.t1 - state.t0) * p;
        setTime(t);
        applyUntil(state.curTime);
        net.redraw();
        showCurrentText();
        showStatsText();
        tcp.redrawAll();
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
        net.redraw();
        showCurrentText();
        showStatsText();
        tcp.redrawAll();
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
            resetToTime(t);
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

    watch(() => state.filterFlow, () => applyFilter());
    watch(() => state.filterPkt, () => applyFilter());
    watch(() => state.layoutChoice, () => {
        if (!state.events.length) return;
        net.applyLayout();
        resetPlayback();
    });
    watch(() => state.connPick, () => tcp.redrawAll());
    watch(() => state.windowMode, () => tcp.redrawAll());

    function setNetCanvas(el) {
        if (!el) {
            net.setCanvas(null);
            return;
        }
        net.setCanvas(el);
        net.applyLayout();
        resetPlayback();
    }

    function setTcpCanvas(el) {
        tcp.setTcpCanvas(el);
    }

    function setTcpCardRef(ref) {
        tcp.setTcpCardRef(ref);
    }

    function setTcpModalCanvas(el) {
        tcp.setTcpModalCanvas(el);
    }

    function onTcpModalClose() {
        tcp.onTcpModalClose();
    }

    function setTcpDetailCanvas(el) {
        tcp.setTcpDetailCanvas(el);
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
            focusView,
            eventTypeCatalog,
            eventTypeFilters,
            tcpAdjustMarks,
        },
        actions: {
            onFile,
            play,
            pause,
            step,
            jumpToDrop,
            jumpToTcpAdjust,
            jumpToTcpAdjustDifferent,
            toggleEventKind,
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
