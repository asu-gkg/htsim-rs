export const defaultNodes = [
    { id: 0, name: "h0", kind: "host" },
    { id: 2, name: "s0", kind: "switch" },
    { id: 3, name: "s1", kind: "switch" },
    { id: 1, name: "h1", kind: "host" },
];

export const defaultLinks = [
    { from: 0, to: 2 },
    { from: 2, to: 3 },
    { from: 3, to: 1 },
];

export function linkKey(a, b) {
    return `${a}->${b}`;
}

export function buildLinkPairs(links) {
    if (!links || !Array.isArray(links)) return [];
    const out = [];
    const seen = new Set();
    for (const l of links) {
        const a = Number(l.from);
        const b = Number(l.to);
        if (!Number.isFinite(a) || !Number.isFinite(b)) continue;
        const key = a < b ? `${a}-${b}` : `${b}-${a}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ from: a, to: b });
    }
    return out;
}

export function layoutFatTree(list, width, height) {
    const parsed = list.map((it) => ({ it, info: parseFatTreeName(it.name ?? "") }));
    if (parsed.some((p) => !p.info)) return null;
    let maxPod = -1;
    let maxAgg = -1;
    let maxEdge = -1;
    let maxCoreGroup = -1;
    let maxCoreIndex = -1;
    let maxHost = -1;
    for (const p of parsed) {
        const info = p.info;
        if (info.kind === "core") {
            maxCoreGroup = Math.max(maxCoreGroup, info.group);
            maxCoreIndex = Math.max(maxCoreIndex, info.index);
        } else if (info.kind === "agg") {
            maxPod = Math.max(maxPod, info.pod);
            maxAgg = Math.max(maxAgg, info.index);
        } else if (info.kind === "edge") {
            maxPod = Math.max(maxPod, info.pod);
            maxEdge = Math.max(maxEdge, info.index);
        } else if (info.kind === "host") {
            maxPod = Math.max(maxPod, info.pod);
            maxEdge = Math.max(maxEdge, info.edge);
            maxHost = Math.max(maxHost, info.index);
        }
    }
    const k = maxPod + 1;
    const half = Math.max(maxAgg, maxEdge, maxCoreGroup, maxCoreIndex, maxHost) + 1;
    if (k <= 0 || half <= 0) return null;

    const left = Math.max(18, Math.round(width * 0.03));
    const right = left;
    const top = Math.max(16, Math.round(height * 0.06));
    const bottom = top;
    const spanH = Math.max(1, height - top - bottom);
    const rowGap = spanH / 3;
    const rowY = {
        core: top,
        agg: top + rowGap,
        edge: top + rowGap * 2,
        host: top + rowGap * 3,
    };
    const podWidth = (width - left - right) / Math.max(1, k);
    const coreGroupWidth = (width - left - right) / Math.max(1, half);

    function xForCore(group, index) {
        const groupLeft = left + coreGroupWidth * group;
        return groupLeft + (coreGroupWidth * (index + 0.5)) / Math.max(1, half);
    }

    function xForPod(pod, index) {
        const podLeft = left + podWidth * pod;
        return podLeft + (podWidth * (index + 0.5)) / Math.max(1, half);
    }

    function xForHost(pod, edge, host) {
        const podLeft = left + podWidth * pod;
        const edgeSpan = podWidth / Math.max(1, half);
        const hostSpan = edgeSpan / Math.max(1, half);
        return podLeft + edgeSpan * edge + hostSpan * (host + 0.5);
    }

    const nodes = parsed.map(({ it, info }) => {
        let x = width / 2;
        let y = height / 2;
        if (info.kind === "core") {
            x = xForCore(info.group, info.index);
            y = rowY.core;
        } else if (info.kind === "agg") {
            x = xForPod(info.pod, info.index);
            y = rowY.agg;
        } else if (info.kind === "edge") {
            x = xForPod(info.pod, info.index);
            y = rowY.edge;
        } else if (info.kind === "host") {
            x = xForHost(info.pod, info.edge, info.index);
            y = rowY.host;
        }
        return {
            id: it.id,
            name: it.name ?? `n${it.id}`,
            kind: it.kind ?? "switch",
            x,
            y,
        };
    });
    const scale = Math.max(0.5, Math.min(1, 12 / Math.sqrt(Math.max(1, nodes.length))));
    return { nodes, scale, kind: "fat-tree" };
}

export function layoutCircle(list, width, height) {
    const cx = width / 2;
    const cy = height / 2;
    const r = Math.min(width, height) * 0.46;
    const n = Math.max(1, list.length);
    const nodes = list.map((it, i) => {
        const ang = (Math.PI * 2 * i) / n - Math.PI / 2;
        return {
            id: it.id,
            name: it.name ?? `n${it.id}`,
            kind: it.kind ?? "switch",
            x: cx + r * Math.cos(ang),
            y: cy + r * Math.sin(ang),
        };
    });
    const scale = Math.max(0.5, Math.min(1, 16 / Math.sqrt(n)));
    return { nodes, scale, kind: "circle" };
}

export function layoutDumbbell(list, links, width, height) {
    const nodes = list.map((it) => ({
        id: it.id,
        name: it.name ?? `n${it.id}`,
        kind: it.kind ?? "switch",
        x: width / 2,
        y: height / 2,
    }));
    const linkSet = new Set();
    for (const l of links || []) {
        linkSet.add(`${l.from}-${l.to}`);
        linkSet.add(`${l.to}-${l.from}`);
    }

    const isHost = (n) => String(n.kind).toLowerCase() === "host" || /^h\d+/.test(n.name ?? "");
    const isSwitch = (n) => !isHost(n);
    const hosts = nodes.filter(isHost);
    const switches = nodes.filter(isSwitch);
    if (switches.length < 2) return null;

    const degree = new Map();
    for (const l of links || []) {
        degree.set(l.from, (degree.get(l.from) || 0) + 1);
        degree.set(l.to, (degree.get(l.to) || 0) + 1);
    }
    const swSorted = switches
        .slice()
        .sort((a, b) => (degree.get(b.id) || 0) - (degree.get(a.id) || 0));
    const leftSw = swSorted[0];
    const rightSw = swSorted[1] || swSorted[0];

    const leftHosts = [];
    const rightHosts = [];
    let toggle = true;
    for (const h of hosts) {
        if (linkSet.has(`${h.id}-${leftSw.id}`)) {
            leftHosts.push(h);
        } else if (linkSet.has(`${h.id}-${rightSw.id}`)) {
            rightHosts.push(h);
        } else {
            (toggle ? leftHosts : rightHosts).push(h);
            toggle = !toggle;
        }
    }

    const padX = width * 0.08;
    const leftX = padX;
    const rightX = width - padX;
    const midLeftX = width * 0.44;
    const midRightX = width * 0.56;
    const centerY = height / 2;
    const spanTop = Math.max(16, height * 0.08);
    const spanBottom = Math.min(height - 16, height * 0.92);

    function placeColumn(items, x) {
        if (!items.length) return;
        const gap = (spanBottom - spanTop) / (items.length + 1);
        items.forEach((n, i) => {
            n.x = x;
            n.y = spanTop + gap * (i + 1);
        });
    }

    placeColumn(leftHosts, leftX);
    placeColumn(rightHosts, rightX);

    leftSw.x = midLeftX;
    leftSw.y = centerY;
    rightSw.x = midRightX;
    rightSw.y = centerY;

    const extras = switches.filter((s) => s.id !== leftSw.id && s.id !== rightSw.id);
    if (extras.length) {
        const gap = (spanBottom - spanTop) / (extras.length + 1);
        extras.forEach((n, i) => {
            n.x = width * 0.5;
            n.y = spanTop + gap * (i + 1);
        });
    }

    const scale = Math.max(0.55, Math.min(1, 14 / Math.sqrt(Math.max(1, nodes.length))));
    return { nodes, scale, kind: "dumbbell" };
}

function parseFatTreeName(name) {
    if (!name) return null;
    let m = /^c(\d+)_(\d+)$/.exec(name);
    if (m) return { kind: "core", group: Number(m[1]), index: Number(m[2]) };
    m = /^p(\d+)_a(\d+)$/.exec(name);
    if (m) return { kind: "agg", pod: Number(m[1]), index: Number(m[2]) };
    m = /^p(\d+)_e(\d+)$/.exec(name);
    if (m) return { kind: "edge", pod: Number(m[1]), index: Number(m[2]) };
    m = /^h(\d+)_(\d+)_(\d+)$/.exec(name);
    if (m) return { kind: "host", pod: Number(m[1]), edge: Number(m[2]), index: Number(m[3]) };
    return null;
}
