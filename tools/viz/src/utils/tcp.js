export function pickPointAt(pts, t) {
    let lo = 0;
    let hi = pts.length - 1;
    let ans = null;
    while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        if (pts[mid].t <= t) {
            ans = pts[mid];
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    return ans;
}

export function pickAutoConn(series, t) {
    let bestCid = null;
    let bestPast = -Infinity;
    let bestFuture = Infinity;
    for (const [cid, ser] of series.entries()) {
        const pts = ser?.points || [];
        if (!pts.length) continue;
        const last = pickPointAt(pts, t);
        if (last) {
            const ts = Number(last.t ?? 0);
            if (ts >= bestPast) {
                bestPast = ts;
                bestCid = cid;
            }
        } else if (bestPast === -Infinity) {
            const first = Number(pts[0].t ?? 0);
            if (first < bestFuture) {
                bestFuture = first;
                bestCid = cid;
            }
        }
    }
    return bestCid;
}

export function buildTcpSeries(evts) {
    const byConn = new Map();
    for (const e of evts) {
        if (!e || typeof e.kind !== "string") continue;
        if (!e.kind.startsWith("tcp_") && e.kind !== "dctcp_cwnd") continue;
        const cid = Number(e.conn_id ?? e.flow_id ?? 0);
        if (!cid) continue;
        if (!byConn.has(cid)) byConn.set(cid, []);
        byConn.get(cid).push(e);
    }
    const out = new Map();
    for (const [cid, arr] of byConn.entries()) {
        arr.sort((a, b) => (a.t_ns ?? 0) - (b.t_ns ?? 0));
        const mss = inferMss(arr) || 1460;
        const timeline = buildTcpTimeline(arr);
        let pts = buildCwndSeries(arr, mss);
        if (pts.length && timeline.windowPoints.length) {
            pts = pts.map((p) => {
                if (p.lastAck != null) return p;
                const win = pickPointAt(timeline.windowPoints, p.t);
                return { ...p, lastAck: win?.lastAck ?? null };
            });
        }
        out.set(cid, { mss, points: pts, ...timeline });
    }
    return out;
}

function buildCwndSeries(arr, mss) {
    const cwndSamples = arr.filter((e) => e.kind === "dctcp_cwnd");
    if (cwndSamples.length) {
        return cwndSamples.map((e) => ({
            t: Number(e.t_ns ?? 0),
            cwnd: Number(e.cwnd_bytes ?? 0),
            ssthresh: Number(e.ssthresh_bytes ?? 0),
            inflight: Number(e.inflight_bytes ?? 0),
            alpha: e.alpha != null ? Number(e.alpha) : null,
            lastAck: null,
            dup: null,
            state: "DCTCP",
            reason: "sample",
        }));
    }
    const initCwnd = inferInitCwnd(arr, mss);
    let cwnd = initCwnd;
    let ssthresh = 1000 * mss;
    let lastAck = 0;
    let dup = 0;
    let stateStr = "SS";
    const infl = new Map();
    const pts = [];

    const inflightBytes = () => {
        let s = 0;
        for (const v of infl.values()) s += v;
        return s;
    };
    const rec = (t, reason) =>
        pts.push({ t, cwnd, ssthresh, inflight: inflightBytes(), lastAck, dup, state: stateStr, reason });

    rec(Number(arr[0]?.t_ns ?? 0), "init");

    for (const e of arr) {
        const t = Number(e.t_ns ?? 0);
        if (e.kind === "tcp_send_data") {
            if (e.seq != null && e.len != null) infl.set(Number(e.seq), Number(e.len));
            rec(t, "send_data");
        } else if (e.kind === "tcp_recv_ack") {
            const ack = Number(e.ack ?? 0);
            if (ack > lastAck) {
                dup = 0;
                const newly = ack - lastAck;
                lastAck = ack;
                for (const [seq, len] of Array.from(infl.entries())) {
                    if (seq + len <= ack) infl.delete(seq);
                }
                if (cwnd < ssthresh) {
                    stateStr = "SS";
                    cwnd = cwnd + newly;
                } else {
                    stateStr = "CA";
                    const inc = Math.max(1, Math.floor((mss * mss) / Math.max(1, cwnd)));
                    cwnd = cwnd + inc;
                }
                rec(t, "new_ack");
            } else if (ack === lastAck) {
                dup = dup + 1;
                if (dup === 3) {
                    stateStr = "FR";
                    ssthresh = Math.max(Math.floor(cwnd / 2), 2 * mss);
                    cwnd = ssthresh + 3 * mss;
                    rec(t, "3_dupack");
                } else if (dup > 3) {
                    stateStr = "FR";
                    cwnd = cwnd + mss;
                    rec(t, "more_dupack");
                } else {
                    rec(t, "dupack");
                }
            }
        } else if (e.kind === "tcp_rto") {
            stateStr = "RTO";
            ssthresh = Math.max(Math.floor(cwnd / 2), 2 * mss);
            cwnd = mss;
            dup = 0;
            rec(t, "rto");
        } else if (e.kind === "tcp_send_ack") {
            rec(t, "send_ack");
        }
    }
    return pts;
}

function buildTcpTimeline(arr) {
    const seqEvents = [];
    const ackEvents = [];
    const rtoEvents = [];
    const ackLinks = [];
    const windowPoints = [];
    const rttSeries = [];
    const inflight = new Map();
    const sentEnds = new Map();
    const retransEnds = new Set();
    const ackedEnds = new Set();
    let lastAck = 0;
    let maxSent = 0;
    let srtt = null;
    let rttvar = null;
    let rto = null;

    const recWindow = (t) => {
        windowPoints.push({
            t,
            lastAck,
            maxSent,
            inflight: Math.max(0, maxSent - lastAck),
        });
    };

    recWindow(Number(arr[0]?.t_ns ?? 0));

    for (const e of arr) {
        const t = Number(e.t_ns ?? 0);
        if (e.kind === "tcp_send_data") {
            const seq = Number(e.seq ?? 0);
            const len = Number(e.len ?? 0);
            const end = seq + len;
            // 新格式：Rust 会在真正重传时发出 tcp_send_data 且带 retrans=true
            // 旧格式：没有该字段，只能用“同一 end 在未被 ACK 前再次发送”来推断
            const retrans = e.retrans === true || (sentEnds.has(end) && end > lastAck);
            if (retrans) retransEnds.add(end);
            sentEnds.set(end, t);
            inflight.set(seq, len);
            seqEvents.push({ t, seq, len, end, retrans });
            if (end > maxSent) maxSent = end;
            recWindow(t);
        } else if (e.kind === "tcp_recv_ack") {
            const ack = Number(e.ack ?? 0);
            const ecn = e.ecn_echo === true;
            ackEvents.push({ t, ack, ecn });
            if (ack > lastAck) {
                const match = pickAckMatch(ack, sentEnds, ackedEnds);
                if (match) {
                    const sample = t - match.sentAt;
                    const retrans = retransEnds.has(match.end);
                    if (sample >= 0 && !retrans) {
                        if (srtt == null) {
                            srtt = sample;
                            rttvar = sample / 2;
                        } else {
                            rttvar = 0.75 * rttvar + 0.25 * Math.abs(srtt - sample);
                            srtt = 0.875 * srtt + 0.125 * sample;
                        }
                        rto = srtt + 4 * rttvar;
                    }
                    if (sample >= 0) {
                        rttSeries.push({
                            t,
                            rtt: sample,
                            srtt: srtt ?? sample,
                            rto: rto ?? sample * 2,
                            retrans,
                        });
                    }
                    ackLinks.push({
                        send_t: match.sentAt,
                        send_seq: match.end,
                        ack_t: t,
                        ack_seq: match.end,
                        retrans,
                        ecn,
                    });
                    ackedEnds.add(match.end);
                    retransEnds.delete(match.end);
                }
                lastAck = ack;
                for (const [seq, len] of Array.from(inflight.entries())) {
                    if (seq + len <= ack) inflight.delete(seq);
                }
                for (const [end] of Array.from(sentEnds.entries())) {
                    if (end <= ack) sentEnds.delete(end);
                }
                recWindow(t);
            }
        } else if (e.kind === "tcp_rto") {
            const seq = Number(e.seq ?? 0);
            rtoEvents.push({ t, seq });
            recWindow(t);
        }
    }

    return { seqEvents, ackEvents, rtoEvents, ackLinks, windowPoints, rttSeries };
}

function pickAckMatch(ack, sentEnds, ackedEnds) {
    let matchEnd = null;
    let matchTime = null;
    for (const [end, sentAt] of sentEnds.entries()) {
        if (end > ack || ackedEnds.has(end)) continue;
        if (matchEnd == null || end > matchEnd) {
            matchEnd = end;
            matchTime = sentAt;
        }
    }
    if (matchEnd == null) return null;
    return { end: matchEnd, sentAt: matchTime };
}

function inferMss(arr) {
    let m = 0;
    for (const e of arr) {
        if (e.kind === "tcp_send_data" && e.len != null) m = Math.max(m, Number(e.len));
    }
    return m || null;
}

function inferInitCwnd(arr, mss) {
    const ds = arr.filter((e) => e.kind === "tcp_send_data" && e.t_ns != null && e.len != null);
    if (!ds.length) return 10 * mss;
    const tMin = Math.min(...ds.map((e) => Number(e.t_ns)));
    const sum = ds.filter((e) => Number(e.t_ns) === tMin).reduce((s, e) => s + Number(e.len), 0);
    return Math.max(mss, sum || 10 * mss);
}
