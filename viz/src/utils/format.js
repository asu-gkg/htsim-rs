export function fmtMs(ns) {
    return (ns / 1e6).toFixed(3) + " ms";
}

export function fmtGbps(bps) {
    if (bps == null) return "-";
    return (Number(bps) / 1e9).toFixed(3) + " Gbps";
}

export function fmtBytes(x) {
    if (x == null) return "-";
    const n = Number(x);
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(2)} KiB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MiB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}

export function fmtCapBytes(x) {
    if (x == null) return "-";
    const n = Number(x);
    if (n === 18446744073709551615) return "∞";
    return fmtBytes(n);
}
