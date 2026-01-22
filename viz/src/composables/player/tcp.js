import { Container, Graphics } from "pixi.js";
import { fmtBytes, fmtMs } from "../../utils/format";
import { buildTcpSeries, pickAutoConn, pickPointAt } from "../../utils/tcp";
import {
    addText,
    beginFill,
    clearTextLayer,
    createPixiApp,
    destroyPixiApp,
    drawDashedLine,
    drawDashedPolyline,
    drawRoundedRect,
    resizePixiApp,
    setLineStyle,
} from "../../utils/pixi";

export function createTcpController(state) {
    const fontMono = "JetBrains Mono, monospace";
    let mainSurface = null;
    let detailSurface = null;
    let modalSurface = null;
    let tcpCardRef = null;

    function createSurface(canvas) {
        const app = createPixiApp(canvas);
        const graphics = new Graphics();
        const textLayer = new Container();
        app.stage.addChild(graphics, textLayer);
        return { app, graphics, textLayer, canvas };
    }

    function destroySurface(surface) {
        if (!surface) return;
        destroyPixiApp(surface.app);
    }

    function clearSurface(surface) {
        if (!surface) return;
        surface.graphics.clear();
        clearTextLayer(surface.textLayer);
    }

    function ensureSurfaceSize(surface) {
        if (!surface?.app || !surface?.canvas) return { width: 0, height: 0 };
        return resizePixiApp(surface.app, surface.canvas);
    }

    function resetTracking() {
        state.tcpStats = { send_data: 0, send_ack: 0, recv_ack: 0, rto: 0, retrans: 0 };
    }

    function updateConnPick() {
        const conns = Array.from(state.tcpSeries.keys()).sort((a, b) => a - b);
        state.connOptions = conns;
        if (state.connPick !== "auto" && !conns.includes(Number(state.connPick))) {
            state.connPick = "auto";
        }
    }

    function rebuildSeries(events) {
        state.tcpSeries = buildTcpSeries(events);
        updateConnPick();
    }

    function applyEvent(ev) {
        const kind = ev.kind;
        if (!kind || !kind.startsWith("tcp_")) return false;
        const extra = [];
        if (ev.conn_id != null) extra.push(`conn=${ev.conn_id}`);
        if (ev.seq != null) extra.push(`seq=${ev.seq}`);
        if (ev.len != null) extra.push(`len=${ev.len}`);
        if (ev.ack != null) extra.push(`ack=${ev.ack}`);
        if (ev.ecn_echo) extra.push("ecn_echo=1");
        const head = `[${fmtMs(ev.t_ns)}] ${kind}`;
        state.lastEventsText.push(`${head} ${extra.join(" ")}`.trim());

        // TCP 事件计数（随时间推进）
        if (kind === "tcp_send_data") {
            state.tcpStats.send_data += 1;
            if (ev.retrans === true) {
                state.tcpStats.retrans += 1;
            }
        }
        if (kind === "tcp_send_ack") state.tcpStats.send_ack += 1;
        if (kind === "tcp_recv_ack") state.tcpStats.recv_ack += 1;
        if (kind === "tcp_rto") state.tcpStats.rto += 1;
        return true;
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
        if (!mainSurface) return;
        const { width: canvasW, height: canvasH } = ensureSurfaceSize(mainSurface);
        clearSurface(mainSurface);

        const sel = selectTcpConn();
        const cid = sel.cid;
        const ser = sel.ser;
        if (cid == null || !ser) {
            drawTcpBoxAt(mainSurface, canvasW / 2, canvasH / 2, "无 tcp_* / dctcp_cwnd 事件");
            return;
        }
        const pts = ser?.points || [];
        const mss = ser?.mss || 1460;
        if (!pts.length) {
            drawTcpBoxAt(mainSurface, canvasW / 2, canvasH / 2, `conn=${cid} 无可用数据点`);
            return;
        }

        // 2x2 子图布局
        const gap = 12;
        const subW = Math.floor((canvasW - gap) / 2);
        const subH = Math.floor((canvasH - gap) / 2);
        const areas = [
            { x: 0, y: 0, w: subW, h: subH, title: "cwnd（拥塞窗口）", field: "cwnd", color: "#0ea5e9", fill: false },
            { x: subW + gap, y: 0, w: subW, h: subH, title: "ssthresh（慢启动阈值）", field: "ssthresh", color: "#ef4444", fill: false },
            { x: 0, y: subH + gap, w: subW, h: subH, title: "inflight（在途数据）", field: "inflight", color: "#22c55e", fill: true },
            { x: subW + gap, y: subH + gap, w: subW, h: subH, title: "三者对比", field: "all", color: "", fill: false },
        ];

        const curP = pickPointAt(pts, state.curTime);

        for (const area of areas) {
            drawTcpSubChartOnSurface(mainSurface, pts, mss, area, curP, true);
        }
    }

    function drawTcpSubChartOnSurface(surface, pts, mss, area, curP, isSmall = false) {
        const g = surface.graphics;
        const textLayer = surface.textLayer;
        const { x: ax, y: ay, w: aw, h: ah, title, field, color, fill } = area;
        // 放大版用更大的 padding
        const pad = isSmall ? { l: 50, r: 8, t: 18, b: 6 } : { l: 80, r: 20, t: 40, b: 30 };
        const chartX = ax + pad.l;
        const chartY = ay + pad.t;
        const chartW = aw - pad.l - pad.r;
        const chartH = ah - pad.t - pad.b;

        // 背景
        setLineStyle(g, 1, "rgba(15,23,42,0.1)");
        beginFill(g, isSmall ? "rgba(15,23,42,0.03)" : "rgba(255,255,255,1)");
        drawRoundedRect(g, ax + 0.5, ay + 0.5, aw - 1, ah - 1, isSmall ? 8 : 12);
        g.endFill();

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
        const fontSize = isSmall ? 11 : 13;
        setLineStyle(g, 1, "rgba(15,23,42,0.08)");
        for (let k = 0; k <= gridLines; k++) {
            const pk = Math.round((maxPkts * k) / gridLines);
            const y = chartY + (1 - k / gridLines) * chartH;
            g.moveTo(chartX, y);
            g.lineTo(chartX + chartW, y);
            addText(
                textLayer,
                `${pk}`,
                { fontFamily: fontMono, fontSize, fill: "rgba(15,23,42,0.6)" },
                chartX - 6,
                y,
                1,
                0.5
            );
        }

        // 标题
        addText(
            textLayer,
            title,
            { fontFamily: fontMono, fontSize: isSmall ? 12 : 18, fill: "rgba(15,23,42,0.8)" },
            ax + (isSmall ? 6 : 16),
            ay + (isSmall ? 4 : 12),
            0,
            0
        );

        // Y 轴标签
        if (!isSmall) {
            addText(
                textLayer,
                "pkts",
                { fontFamily: fontMono, fontSize: 12, fill: "rgba(15,23,42,0.5)" },
                ax + 16,
                chartY + chartH / 2,
                0.5,
                0.5,
                -Math.PI / 2
            );
        }

        // 绘制曲线
        const lineWidth = isSmall ? 1.5 : 2.5;
        if (field === "all") {
            drawTcpLineInArea(g, pts, "inflight", "rgba(34,197,94,0.15)", "#22c55e", false, true, xOf, yOf, chartX, chartY, chartW, chartH, lineWidth);
            drawTcpLineInArea(g, pts, "ssthresh", "#ef4444", "#ef4444", true, false, xOf, yOf, chartX, chartY, chartW, chartH, lineWidth);
            drawTcpLineInArea(g, pts, "cwnd", "#0ea5e9", "#0ea5e9", false, false, xOf, yOf, chartX, chartY, chartW, chartH, lineWidth);
        } else {
            drawTcpLineInArea(g, pts, field, fill ? `${color}30` : color, color, false, fill, xOf, yOf, chartX, chartY, chartW, chartH, lineWidth);
        }

        // 图例（仅放大版且是 all 时显示）
        if (!isSmall && field === "all") {
            const legendX = chartX + chartW - 180;
            const legendY = chartY + 10;
            setLineStyle(g, 1, "rgba(15,23,42,0.15)");
            beginFill(g, "rgba(255,255,255,0.9)");
            drawRoundedRect(g, legendX, legendY, 170, 70, 6);
            g.endFill();

            const items = [
                { label: "cwnd", color: "#0ea5e9", dashed: false },
                { label: "ssthresh", color: "#ef4444", dashed: true },
                { label: "inflight", color: "#22c55e", dashed: false, fill: true },
            ];
            items.forEach((item, i) => {
                const y = legendY + 15 + i * 18;
                if (item.fill) {
                    beginFill(g, "rgba(34,197,94,0.3)");
                    g.drawRect(legendX + 10, y - 5, 30, 10);
                    g.endFill();
                }
                setLineStyle(g, 2, item.color);
                if (item.dashed) {
                    drawDashedLine(g, legendX + 10, y, legendX + 40, y, 4, 3);
                } else {
                    g.moveTo(legendX + 10, y);
                    g.lineTo(legendX + 40, y);
                }
                addText(
                    textLayer,
                    item.label,
                    { fontFamily: fontMono, fontSize: 12, fill: "rgba(15,23,42,0.8)" },
                    legendX + 50,
                    y,
                    0,
                    0.5
                );
            });
        }

        // 当前时刻指示线
        if (curP) {
            const xNow = xOf(curP.t);
            setLineStyle(g, isSmall ? 1 : 2, "rgba(245,158,11,0.7)");
            g.moveTo(xNow, chartY);
            g.lineTo(xNow, chartY + chartH);

            // 当前值
            const val = field === "all" ? curP.cwnd : curP[field];
            if (val != null) {
                const valText =
                    field === "all"
                        ? `cwnd:${(curP.cwnd / mss).toFixed(1)}  ssthresh:${(curP.ssthresh / mss).toFixed(1)}  inflight:${(curP.inflight / mss).toFixed(1)} pkts`
                        : `${(val / mss).toFixed(1)} pkts`;
                addText(
                    textLayer,
                    valText,
                    { fontFamily: fontMono, fontSize: isSmall ? 11 : 14, fill: "rgba(15,23,42,0.7)" },
                    ax + aw - (isSmall ? 6 : 16),
                    ay + (isSmall ? 4 : 12),
                    1,
                    0
                );
            }

            // 放大版显示时间
            if (!isSmall) {
                addText(
                    textLayer,
                    `t=${fmtMs(curP.t)}`,
                    { fontFamily: fontMono, fontSize: 12, fill: "rgba(245,158,11,0.9)" },
                    xNow,
                    chartY + chartH + 6,
                    0.5,
                    0
                );
            }
        }
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

    function drawTcpLineInArea(g, pts, field, fillColor, strokeColor, dashed, fill, xOf, yOf, cx, cy, cw, ch, lineWidth = 1.5) {
        if (!pts.length) return;
        const clampY = (y) => Math.min(cy + ch, Math.max(cy, y));
        const points = pts.map((p) => {
            const x = xOf(p.t);
            const y = yOf(p[field] ?? 0);
            return [x, clampY(y)];
        });
        if (fill) {
            const baseY = cy + ch;
            beginFill(g, fillColor);
            const polygon = [points[0][0], baseY];
            for (const [x, y] of points) {
                polygon.push(x, y);
            }
            polygon.push(points[points.length - 1][0], baseY);
            g.drawPolygon(polygon);
            g.endFill();
        }

        setLineStyle(g, lineWidth, strokeColor);
        if (dashed) {
            drawDashedPolyline(g, points, 6, 4);
        } else {
            g.moveTo(points[0][0], points[0][1]);
            for (let i = 1; i < points.length; i += 1) {
                g.lineTo(points[i][0], points[i][1]);
            }
        }
    }

    function drawTcpBoxAt(surface, x, y, text) {
        addText(
            surface.textLayer,
            text,
            { fontFamily: fontMono, fontSize: 12, fill: "rgba(15,23,42,0.6)" },
            x,
            y,
            0.5,
            0.5
        );
    }

    function redrawAll() {
        redrawTcp();
        redrawTcpDetails();
    }

    function redrawTcpDetails() {
        if (!detailSurface) return;
        const { width: canvasW, height: canvasH } = ensureSurfaceSize(detailSurface);
        clearSurface(detailSurface);
        const g = detailSurface.graphics;
        const textLayer = detailSurface.textLayer;
        const w = canvasW;
        const h = canvasH;

        setLineStyle(g, 1, "rgba(15,23,42,0.12)");
        beginFill(g, "rgba(15,23,42,0.04)");
        drawRoundedRect(g, 0.5, 0.5, w - 1, h - 1, 14);
        g.endFill();

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

        const pad = { l: 90, r: 18, t: 16, b: 16 };
        const gap = 20;
        const seqHeight = Math.round(h * 0.48);
        const windowHeight = Math.round(h * 0.16);
        const infoHeight = h - pad.t - pad.b - seqHeight - windowHeight - gap * 2;
        const seqArea = { x: pad.l, y: pad.t, w: w - pad.l - pad.r, h: seqHeight };
        const windowArea = { x: pad.l, y: seqArea.y + seqArea.h + gap, w: seqArea.w, h: windowHeight };
        const infoArea = { x: pad.l, y: windowArea.y + windowArea.h + gap, w: seqArea.w, h: infoHeight };
        const curWin = pickPointAt(windowPoints, state.curTime);
        const curP = pickPointAt(pts, state.curTime);

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

        setLineStyle(g, 1, "rgba(15,23,42,0.12)");
        for (let k = 0; k <= 4; k++) {
            const seq = minSeq + ((maxSeq - minSeq) * k) / 4;
            const y = yOf(seq);
            g.moveTo(seqArea.x, y);
            g.lineTo(seqArea.x + seqArea.w, y);
            addText(
                textLayer,
                fmtBytes(seq),
                { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
                seqArea.x - 10,
                y,
                1,
                0.5
            );
        }
        for (let k = 0; k <= 4; k++) {
            const t = state.t0 + ((state.t1 - state.t0) * k) / 4;
            const x = xOf(t);
            g.moveTo(x, seqArea.y);
            g.lineTo(x, seqArea.y + seqArea.h);
            addText(
                textLayer,
                fmtMs(t),
                { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
                x,
                seqArea.y + seqArea.h + 4,
                0.5,
                0
            );
        }
        addText(
            textLayer,
            `Sequence-Time（conn=${cid}）`,
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
            seqArea.x,
            Math.max(2, seqArea.y - 12),
            0,
            0
        );

        if (curWin && curP?.cwnd != null) {
            const lastAck = Number(curWin.lastAck ?? 0);
            const maxSent = Number(curWin.maxSent ?? lastAck);
            const winEnd = lastAck + Number(curP.cwnd ?? 0);
            const y1 = yOf(lastAck);
            const y2 = yOf(winEnd);
            const top = Math.min(y1, y2);
            const hh = Math.abs(y1 - y2);
            beginFill(g, "rgba(14,165,233,0.06)");
            g.drawRect(seqArea.x, top, seqArea.w, hh);
            g.endFill();
            const ys = yOf(maxSent);
            setLineStyle(g, 1, "rgba(14,165,233,0.35)");
            drawDashedLine(g, seqArea.x, ys, seqArea.x + seqArea.w, ys, 4, 4);
            addText(
                textLayer,
                "window band: [last_ack, win_end]",
                { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.65)" },
                seqArea.x + 4,
                seqArea.y + 4,
                0,
                0
            );
            addText(
                textLayer,
                "dashed: max_sent",
                { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.65)" },
                seqArea.x + 4,
                seqArea.y + 18,
                0,
                0
            );
        }

        setLineStyle(g, 1, "rgba(100,116,139,0.55)");
        for (const l of ackLinks) {
            const y = yOf(l.send_seq);
            drawDashedLine(g, xOf(l.send_t), y, xOf(l.ack_t), y, 4, 4);
        }

        for (const s of seqEvents) {
            const x = xOf(s.t);
            const y1 = yOf(s.seq);
            const y2 = yOf(s.end);
            setLineStyle(g, s.retrans ? 3 : 2, s.retrans ? "#f59e0b" : "#0ea5e9");
            g.moveTo(x, y1);
            g.lineTo(x, y2);
        }

        for (const r of rtoEvents) {
            const x = xOf(r.t);
            const y = yOf(r.seq);
            setLineStyle(g, 2, "#ef4444");
            g.moveTo(x - 5, y - 5);
            g.lineTo(x + 5, y + 5);
            g.moveTo(x + 5, y - 5);
            g.lineTo(x - 5, y + 5);
        }

        let lastAckSeen = -Infinity;
        for (const a of ackEvents) {
            const x = xOf(a.t);
            const y = yOf(a.ack);
            const isDup = a.ack <= lastAckSeen;
            if (a.ack > lastAckSeen) lastAckSeen = a.ack;
            setLineStyle(g, 1, "rgba(0,0,0,0.25)");
            beginFill(g, a.ecn ? "#ef4444" : "#22c55e");
            if (isDup) {
                g.drawRect(x - 3, y - 3, 6, 6);
            } else {
                g.drawPolygon([x, y - 5, x + 5, y + 5, x - 5, y + 5]);
            }
            g.endFill();
        }

        const xNow = xOf(state.curTime);
        setLineStyle(g, 1, "rgba(15,23,42,0.4)");
        g.moveTo(xNow, seqArea.y);
        g.lineTo(xNow, seqArea.y + seqArea.h);

        drawWindowBar(detailSurface, windowArea, windowPoints, pts, mss);

        const stateStr = inferRenoState(curP?.state, curP?.reason);
        const stateArea = { x: infoArea.x, y: infoArea.y, w: Math.min(220, infoArea.w * 0.42), h: infoArea.h };
        const textArea = {
            x: stateArea.x + stateArea.w + 12,
            y: infoArea.y,
            w: Math.max(120, infoArea.w - stateArea.w - 12),
            h: infoArea.h,
        };
        drawStateMachine(detailSurface, stateArea, stateStr);

        const rttP = pickPointAt(rttSeries, state.curTime);
        let lastEcn = null;
        for (const a of ackEvents) {
            if (a.t > state.curTime) break;
            if (a.ecn) lastEcn = a;
        }
        const reasonText = explainTcpReason(stateStr, curP?.reason);
        const alphaText = curP?.alpha != null ? Number(curP.alpha).toFixed(3) : "-";
        let inflightBytesUI = null;
        if (curWin) {
            const la = Number(curWin.lastAck ?? 0);
            const ms = Number(curWin.maxSent ?? la);
            inflightBytesUI = Math.max(0, ms - la);
        }
        const inflightText = inflightBytesUI != null ? fmtBytes(inflightBytesUI) : "-";
        const rttText = rttP ? fmtMs(rttP.rtt) : "-";
        const srttText = rttP ? fmtMs(rttP.srtt) : "-";
        const rtoText = rttP ? fmtMs(rttP.rto) : "-";
        const ecnText = lastEcn ? `ack=${lastEcn.ack} @ ${fmtMs(lastEcn.t)}` : "-";

        const lines = [
            `state=${stateStr}  inflight=${inflightText}  alpha=${alphaText}`,
            `explain: ${reasonText}`,
            `rtt=${rttText}  srtt=${srttText}  rto=${rtoText}`,
            `ecn_echo: ${ecnText}`,
        ];
        let y = textArea.y;
        for (const line of lines) {
            addText(
                textLayer,
                line,
                { fontFamily: fontMono, fontSize: 12, fill: "rgba(15,23,42,0.75)" },
                textArea.x,
                y,
                0,
                0
            );
            y += 14;
        }
    }

    function drawWindowBar(surface, area, windowPoints, pts, mss) {
        const g = surface.graphics;
        const textLayer = surface.textLayer;
        const curWin = pickPointAt(windowPoints, state.curTime);
        const curP = pickPointAt(pts, state.curTime);
        if (!curWin || !curP || curP.cwnd == null) {
            addText(
                textLayer,
                "Send window：无可用数据",
                { fontFamily: fontMono, fontSize: 12, fill: "rgba(15,23,42,0.6)" },
                area.x,
                area.y + area.h / 2,
                0,
                0.5
            );
            return;
        }
        const lastAck = Number(curWin.lastAck ?? 0);
        const maxSent = Number(curWin.maxSent ?? lastAck);
        const cwnd = Number(curP.cwnd ?? 0);
        const inflightRaw = Math.max(0, maxSent - lastAck);
        const inflight = Math.min(inflightRaw, cwnd);
        const avail = Math.max(0, cwnd - inflightRaw);
        const fillPct = cwnd > 0 ? inflight / cwnd : 0;

        const mssBytes = Math.max(1, mss);
        const range = Math.max(16 * mssBytes, cwnd * 1.2, inflightRaw * 1.2, 2 * mssBytes);
        const xOfDelta = (d) => area.x + (d / range) * area.w;
        const y = area.y + area.h * 0.65;

        const cwndMss = mssBytes > 0 ? cwnd / mssBytes : 0;
        const inflightMss = mssBytes > 0 ? inflightRaw / mssBytes : 0;
        const availMss = mssBytes > 0 ? avail / mssBytes : 0;
        const stateStr = inferRenoState(curP?.state, curP?.reason);
        const reasonText = reasonShort(curP?.reason);
        const explainLine = `t=${fmtMs(state.curTime)} | ${stateStr} | ${reasonText} | c=${cwndMss.toFixed(
            0
        )}M i=${inflightMss.toFixed(0)}M a=${availMss.toFixed(0)}M f=${Math.round(fillPct * 100)}%`;
        addText(
            textLayer,
            explainLine,
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
            area.x,
            area.y + 2,
            0,
            0
        );

        const rangeMss = range / mssBytes;
        const targetTicks = 6;
        const roughStep = rangeMss / targetTicks;
        const stepChoices = [1, 2, 4, 8, 16, 32, 64, 128, 256];
        const step = stepChoices.find((s) => s >= roughStep) || stepChoices[stepChoices.length - 1];

        let lastTick = -Infinity;
        for (let k = 0; k <= rangeMss + 1e-6; k += step) {
            const d = k * mssBytes;
            const x = xOfDelta(d);
            setLineStyle(g, 1, "rgba(15,23,42,0.18)");
            g.moveTo(x, y - 8);
            g.lineTo(x, y + 8);
            addText(
                textLayer,
                `${Math.round(k)}M`,
                { fontFamily: fontMono, fontSize: 10, fill: "rgba(15,23,42,0.55)" },
                x,
                y + 12,
                0.5,
                0
            );
            lastTick = k;
        }
        if (rangeMss - lastTick > step * 0.3) {
            const k = rangeMss;
            const x = xOfDelta(k * mssBytes);
            setLineStyle(g, 1, "rgba(15,23,42,0.18)");
            g.moveTo(x, y - 8);
            g.lineTo(x, y + 8);
            addText(
                textLayer,
                `${Math.round(k)}M`,
                { fontFamily: fontMono, fontSize: 10, fill: "rgba(15,23,42,0.55)" },
                x,
                y + 12,
                0.5,
                0
            );
        }

        const barH = 12;
        const x0 = xOfDelta(0);
        const xEnd = xOfDelta(cwnd);
        const xFill = xOfDelta(Math.min(inflightRaw, cwnd));

        beginFill(g, "rgba(14,165,233,0.10)");
        drawRoundedRect(g, x0, y - barH / 2, Math.max(1, xEnd - x0), barH, 6);
        g.endFill();

        beginFill(g, "rgba(14,165,233,0.85)");
        drawRoundedRect(g, x0, y - barH / 2, Math.max(1, xFill - x0), barH, 6);
        g.endFill();

        setLineStyle(g, 1, "rgba(15,23,42,0.25)");
        g.moveTo(area.x, y);
        g.lineTo(area.x + area.w, y);

        if (inflightRaw > cwnd) {
            setLineStyle(g, 4, "#ef4444");
            g.moveTo(xOfDelta(cwnd), y);
            g.lineTo(xOfDelta(inflightRaw), y);
            const xOver = xOfDelta(inflightRaw);
            setLineStyle(g, 2, "#ef4444");
            g.moveTo(xOver - 6, y - 4);
            g.lineTo(xOver, y);
            g.lineTo(xOver - 6, y + 4);
        }

        const markerH = 10;
        setLineStyle(g, 2, "#0f172a");
        const xAck = x0;
        const xSent = xOfDelta(inflightRaw);
        g.moveTo(xAck, y - markerH);
        g.lineTo(xAck, y + markerH);
        g.moveTo(xEnd, y - markerH);
        g.lineTo(xEnd, y + markerH);

        setLineStyle(g, 2, "rgba(14,165,233,0.85)");
        g.moveTo(xSent, y - markerH);
        g.lineTo(xSent, y + markerH);

        const labelBaseY = y - 14;
        const labelStep = 12;
        const minLabelGap = 28;
        let yAckLabel = labelBaseY;
        let ySentLabel = labelBaseY;
        let yEndLabel = labelBaseY;
        if (Math.abs(xEnd - xAck) < minLabelGap) yEndLabel = labelBaseY - labelStep;
        if (Math.abs(xSent - xAck) < minLabelGap) ySentLabel = labelBaseY - labelStep;
        if (Math.abs(xSent - xEnd) < minLabelGap) {
            ySentLabel = yEndLabel === labelBaseY ? labelBaseY - labelStep : labelBaseY - labelStep * 2;
        }

        addText(
            textLayer,
            "ACK",
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
            xAck,
            yAckLabel,
            0.5,
            1
        );
        addText(
            textLayer,
            "SENT",
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
            xSent,
            ySentLabel,
            0.5,
            1
        );
        addText(
            textLayer,
            "END",
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.75)" },
            xEnd,
            yEndLabel,
            0.5,
            1
        );
    }

    function drawStateMachine(surface, area, stateStr) {
        const g = surface.graphics;
        const textLayer = surface.textLayer;
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

        setLineStyle(g, 1, "rgba(15,23,42,0.25)");
        g.moveTo(pos.SS.x + w, pos.SS.y + h / 2);
        g.lineTo(pos.CA.x, pos.CA.y + h / 2);
        g.moveTo(pos.SS.x + w / 2, pos.SS.y + h);
        g.lineTo(pos.FR.x + w / 2, pos.FR.y);
        g.moveTo(pos.CA.x + w / 2, pos.CA.y + h);
        g.lineTo(pos.RTO.x + w / 2, pos.RTO.y);
        g.moveTo(pos.FR.x + w, pos.FR.y + h / 2);
        g.lineTo(pos.RTO.x, pos.RTO.y + h / 2);

        for (const k of nodes) {
            const p = pos[k];
            const active = stateStr === k;
            beginFill(g, active ? "rgba(14,116,144,0.25)" : "rgba(255,255,255,0.85)");
            setLineStyle(g, 1.5, active ? "rgba(14,116,144,0.85)" : "rgba(15,23,42,0.25)");
            drawRoundedRect(g, p.x, p.y, w, h, 6);
            g.endFill();
            addText(
                textLayer,
                k,
                { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.8)" },
                p.x + w / 2,
                p.y + h / 2,
                0.5,
                0.5
            );
        }

        const label = stateStr === "DCTCP" ? "DCTCP" : "Reno 状态机";
        addText(
            textLayer,
            label,
            { fontFamily: fontMono, fontSize: 11, fill: "rgba(15,23,42,0.65)" },
            area.x,
            area.y + h * 2 + gap + 4,
            0,
            0
        );
    }

    function inferRenoState(stateStr, reason) {
        if (stateStr && stateStr !== "-") return stateStr;
        if (!reason) return "-";
        if (reason === "init" || reason === "ack_slow_start") return "SS";
        if (reason === "ack_congestion_avoidance") return "CA";
        if (reason.startsWith("fast_recovery") || reason.startsWith("dup_ack")) return "FR";
        if (reason === "rto_timeout") return "RTO";
        return "-";
    }

    function reasonShort(reason) {
        if (!reason) return "-";
        const map = {
            init: "init",
            ack_slow_start: "new ACK (SS)",
            ack_congestion_avoidance: "new ACK (CA)",
            fast_recovery_enter: "dupACKx3 enter FR",
            fast_recovery_dup_ack: "dupACK inflate",
            fast_recovery_partial_ack: "partial ACK",
            fast_recovery_exit: "exit FR",
            dup_ack_3: "dupACKx3",
            dup_ack_more: "dupACK more",
            rto_timeout: "RTO timeout",
            dctcp_ecn_window: "ECN sample",
            sample: "sample",
        };
        return map[reason] || reason;
    }

    function explainTcpReason(stateStr, reason) {
        if (stateStr === "DCTCP") return "DCTCP 采样窗口，按 ECN 比例调整 cwnd";
        const map = {
            init: "连接初始窗口",
            ack_slow_start: "慢启动：收到新 ACK，窗口增长",
            ack_congestion_avoidance: "拥塞避免：ACK 驱动 AIMD 增长",
            fast_recovery_enter: "3 次 dupACK，进入快速恢复",
            fast_recovery_dup_ack: "快速恢复中 dupACK，窗口膨胀",
            fast_recovery_partial_ack: "快速恢复中部分 ACK，调整窗口",
            fast_recovery_exit: "快速恢复结束，窗口收敛",
            dup_ack_3: "3 次重复 ACK，触发快速重传",
            dup_ack_more: "更多重复 ACK，继续膨胀窗口",
            rto_timeout: "RTO 超时，窗口回到慢启动",
            dctcp_ecn_window: "DCTCP 窗口末端 ECN 采样，调整 cwnd",
            sample: "窗口采样",
        };
        return map[reason] || "-";
    }

    function drawDetailBox(text) {
        if (!detailSurface) return;
        const { width: canvasW, height: canvasH } = ensureSurfaceSize(detailSurface);
        const g = detailSurface.graphics;
        const x = 14;
        const y = canvasH - 52;
        const w = canvasW - 28;
        const h = 38;
        setLineStyle(g, 1, "rgba(255,255,255,0.12)");
        beginFill(g, "rgba(15,23,42,0.55)");
        drawRoundedRect(g, x, y, w, h, 10);
        g.endFill();
        addText(
            detailSurface.textLayer,
            text,
            { fontFamily: fontMono, fontSize: 12, fill: "rgba(255,255,255,0.95)" },
            x + 10,
            y + h / 2,
            0,
            0.5
        );
    }

    function setTcpCanvas(el) {
        if (!el) {
            if (mainSurface?.canvas) {
                mainSurface.canvas.removeEventListener("click", onTcpCanvasClick);
            }
            destroySurface(mainSurface);
            mainSurface = null;
            return;
        }
        if (mainSurface?.canvas) {
            mainSurface.canvas.removeEventListener("click", onTcpCanvasClick);
        }
        destroySurface(mainSurface);
        mainSurface = createSurface(el);
        el.addEventListener("click", onTcpCanvasClick);
        el.style.cursor = "pointer";
        redrawAll();
    }

    function setTcpCardRef(ref) {
        tcpCardRef = ref;
    }

    function onTcpCanvasClick() {
        if (!mainSurface || !tcpCardRef) return;
        // 点击任意位置都打开放大的 4 子图视图
        tcpCardRef.openModal();
    }

    function setTcpModalCanvas(el) {
        if (!el) {
            destroySurface(modalSurface);
            modalSurface = null;
            return;
        }
        destroySurface(modalSurface);
        modalSurface = createSurface(el);
        redrawTcpModal();
    }

    function onTcpModalClose() {
        destroySurface(modalSurface);
        modalSurface = null;
    }

    function redrawTcpModal() {
        if (!modalSurface) return;
        const { width: canvasW, height: canvasH } = ensureSurfaceSize(modalSurface);
        clearSurface(modalSurface);

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
        const subW = Math.floor((canvasW - gap) / 2);
        const subH = Math.floor((canvasH - gap) / 2);
        const areas = [
            { x: 0, y: 0, w: subW, h: subH, title: "cwnd（拥塞窗口）", field: "cwnd", color: "#0ea5e9", fill: false },
            { x: subW + gap, y: 0, w: subW, h: subH, title: "ssthresh（慢启动阈值）", field: "ssthresh", color: "#ef4444", fill: false },
            { x: 0, y: subH + gap, w: subW, h: subH, title: "inflight（在途数据）", field: "inflight", color: "#22c55e", fill: true },
            { x: subW + gap, y: subH + gap, w: subW, h: subH, title: "三者对比", field: "all", color: "", fill: false },
        ];

        for (const area of areas) {
            drawTcpSubChartOnSurface(modalSurface, pts, mss, area, curP, false);
        }
    }

    function setTcpDetailCanvas(el) {
        if (!el) {
            destroySurface(detailSurface);
            detailSurface = null;
            return;
        }
        destroySurface(detailSurface);
        detailSurface = createSurface(el);
        redrawTcpDetails();
    }

    return {
        applyEvent,
        rebuildSeries,
        resetTracking,
        setTcpCanvas,
        setTcpCardRef,
        setTcpModalCanvas,
        onTcpModalClose,
        setTcpDetailCanvas,
        redrawAll,
        updateConnPick,
    };
}
