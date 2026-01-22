import { Container, Graphics, Text } from "pixi.js";
import { fmtGbps, fmtMs } from "../../utils/format";
import {
    buildLinkPairs,
    defaultLinks,
    defaultNodes,
    layoutCircle,
    layoutDumbbell,
    layoutFatTree,
    linkKey,
} from "../../utils/layout";
import {
    addText,
    beginFill,
    clearTextLayer,
    createPixiApp,
    destroyPixiApp,
    drawDashedLine,
    drawRoundedRect,
    resizePixiApp,
    setLineStyle,
} from "../../utils/pixi";

export function createNetRenderer(state) {
    let app = null;
    let canvas = null;
    let linkLayer = null;
    let nodeLayer = null;
    let packetLayer = null;
    let linkLabelLayer = null;
    let nodeLabelLayer = null;

    function setCanvas(el) {
        if (!el) {
            destroyPixiApp(app);
            app = null;
            canvas = null;
            linkLayer = null;
            nodeLayer = null;
            packetLayer = null;
            linkLabelLayer = null;
            nodeLabelLayer = null;
            return;
        }
        destroyPixiApp(app);
        canvas = el;
        app = createPixiApp(canvas);
        linkLayer = new Graphics();
        nodeLayer = new Graphics();
        packetLayer = new Graphics();
        linkLabelLayer = new Container();
        nodeLabelLayer = new Container();
        app.stage.addChild(linkLayer, linkLabelLayer, nodeLayer, nodeLabelLayer, packetLayer);
    }

    function ensureCanvasSize() {
        if (!app || !canvas) return { width: 0, height: 0 };
        return resizePixiApp(app, canvas);
    }

    function applyLayout() {
        const { width, height } = ensureCanvasSize();
        const layoutW = width || 1100;
        const layoutH = height || 360;
        const metaNodes = state.meta?.nodes;
        const nodesList = metaNodes && Array.isArray(metaNodes) ? metaNodes.slice().sort((a, b) => a.id - b.id) : defaultNodes;
        const linksList = state.meta?.links || defaultLinks;
        let layout = null;
        if (state.layoutChoice === "fat-tree") {
            layout = layoutFatTree(nodesList, layoutW, layoutH) || layoutCircle(nodesList, layoutW, layoutH);
        } else if (state.layoutChoice === "dumbbell") {
            layout = layoutDumbbell(nodesList, linksList, layoutW, layoutH) || layoutCircle(nodesList, layoutW, layoutH);
        } else if (state.layoutChoice === "circle") {
            layout = layoutCircle(nodesList, layoutW, layoutH);
        } else {
            layout = layoutFatTree(nodesList, layoutW, layoutH);
            if (!layout) layout = layoutDumbbell(nodesList, linksList, layoutW, layoutH);
            if (!layout) layout = layoutCircle(nodesList, layoutW, layoutH);
        }
        state.nodes = layout.nodes;
        state.nodeScale = layout.scale;
        state.nodeById = new Map(state.nodes.map((n) => [n.id, n]));
        const pairs = buildLinkPairs(state.meta?.links);
        state.drawLinks = pairs.length ? pairs : defaultLinks.slice();
        state.layoutDetected = layout.kind;
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
            return true;
        }
        if (kind === "delivered" && ev.pkt_id != null) {
            state.inflight.delete(Number(ev.pkt_id));
            state.lastEventsText.push(`${head} pkt=${ev.pkt_id} node=${ev.node}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.delivered = Number(ns.delivered || 0) + 1;
            state.nodeStats.set(Number(ev.node), ns);
            return true;
        }
        if (kind === "drop" && ev.pkt_id != null) {
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
            return true;
        }
        if (kind === "node_rx") {
            state.nodeHighlight.set(Number(ev.node), { node: Number(ev.node), until: ev.t_ns + 200_000 });
            state.lastEventsText.push(`${head} node=${ev.node} (${ev.node_kind}:${ev.node_name}) pkt=${ev.pkt_id}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.rx = Number(ns.rx || 0) + 1;
            if (ev.pkt_bytes != null) ns.bytes = Number(ns.bytes || 0) + Number(ev.pkt_bytes);
            state.nodeStats.set(Number(ev.node), ns);
            return true;
        }
        if (kind === "node_forward") {
            state.lastEventsText.push(`${head} node=${ev.node} -> next=${ev.next} pkt=${ev.pkt_id}`);
            const ns = state.nodeStats.get(Number(ev.node)) || {};
            ns.forward = Number(ns.forward || 0) + 1;
            state.nodeStats.set(Number(ev.node), ns);
            return true;
        }
        if (kind === "enqueue") {
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
            return true;
        }
        return false;
    }

    function redraw() {
        if (!app || !linkLayer || !nodeLayer || !packetLayer) return;
        ensureCanvasSize();
        linkLayer.clear();
        nodeLayer.clear();
        packetLayer.clear();
        clearTextLayer(linkLabelLayer);
        clearTextLayer(nodeLabelLayer);
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

        // 底层阴影
        setLineStyle(linkLayer, baseWidth + 4, "rgba(15,23,42,0.12)");
        linkLayer.moveTo(a.x, a.y);
        linkLayer.lineTo(b.x, b.y);

        // 主链路（颜色随队列深度变化）
        linkLayer.lineStyle({ width: baseWidth, color: linkColor.color, alpha: linkColor.alpha, cap: "round", join: "round" });
        if (isBottleneck) {
            drawDashedLine(linkLayer, a.x, a.y, b.x, b.y, 8, 4);
        } else {
            linkLayer.moveTo(a.x, a.y);
            linkLayer.lineTo(b.x, b.y);
        }

        // 高光
        setLineStyle(linkLayer, Math.max(1, baseWidth * 0.3), "rgba(255,255,255,0.35)");
        linkLayer.moveTo(a.x, a.y);
        linkLayer.lineTo(b.x, b.y);

        // 绘制链路标签（队列深度 + 丢包数 + 瓶颈带宽）
        drawLinkLabel(a, b, fwd, rev, isBottleneck, linkBw);
    }

    function queueRatioColor(ratio) {
        // 0 = 绿色，0.5 = 黄色，1 = 红色
        const r = ratio < 0.5 ? Math.round(255 * (ratio * 2)) : 255;
        const g = ratio < 0.5 ? 200 : Math.round(200 * (1 - (ratio - 0.5) * 2));
        const b = 80;
        const alpha = 0.5 + ratio * 0.4; // 队列越满越不透明
        return { color: (r << 16) + (g << 8) + b, alpha };
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
        if (!text) return;

        const textFill = totalDrop > 0 ? "#dc2626" : isBottleneck ? "#d97706" : "#334155";
        const textObj = new Text(text, {
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 11,
            fill: textFill,
        });
        textObj.anchor.set(0.5, 0.5);
        textObj.x = lx;
        textObj.y = ly;

        const tw = textObj.width + 8;
        const th = Math.max(14, textObj.height + 4);
        const bg = new Graphics();
        const borderColor = totalDrop > 0 ? "rgba(239,68,68,0.7)" : isBottleneck ? "rgba(245,158,11,0.7)" : "rgba(15,23,42,0.25)";
        setLineStyle(bg, 1, borderColor);
        beginFill(bg, "rgba(255,255,255,0.9)");
        drawRoundedRect(bg, lx - tw / 2, ly - th / 2, tw, th, 4);
        bg.endFill();

        linkLabelLayer.addChild(bg);
        linkLabelLayer.addChild(textObj);
    }

    function drawNode(n) {
        const hl = state.nodeHighlight.get(n.id);
        const isHl = hl && hl.until >= state.curTime;
        const r = (n.kind === "switch" ? 24 : 20) * state.nodeScale;
        const fontSize = Math.max(9, Math.round(13 * state.nodeScale));
        beginFill(nodeLayer, isHl ? "rgba(14,116,144,0.25)" : "rgba(255,255,255,0.85)");
        setLineStyle(nodeLayer, 2, isHl ? "rgba(14,116,144,0.8)" : "rgba(15,23,42,0.25)");
        nodeLayer.drawCircle(n.x, n.y, r);
        nodeLayer.endFill();

        addText(
            nodeLabelLayer,
            n.name,
            {
                fontFamily: "JetBrains Mono, monospace",
                fontSize,
                fill: "#0f172a",
            },
            n.x,
            n.y,
            0.5,
            0.5
        );
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

        beginFill(packetLayer, pktColor(p.pkt_kind));
        setLineStyle(packetLayer, 2, "rgba(0,0,0,0.25)");
        packetLayer.drawCircle(x, y, Math.max(3, 7 * state.nodeScale));
        packetLayer.endFill();
    }

    function pktColor(kind) {
        if (kind === "data") return "#0ea5e9";
        if (kind === "ack") return "#22c55e";
        return "#1f2937";
    }

    return {
        setCanvas,
        applyLayout,
        initStatsFromMeta,
        applyEvent,
        redraw,
    };
}
