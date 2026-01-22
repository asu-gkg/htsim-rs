import { Application, Text } from "pixi.js";

export function createPixiApp(canvas) {
    const width = canvas?.width || canvas?.clientWidth || 0;
    const height = canvas?.height || canvas?.clientHeight || 0;
    return new Application({
        view: canvas,
        width,
        height,
        antialias: true,
        backgroundAlpha: 0,
    });
}

export function destroyPixiApp(app) {
    if (!app) return;
    app.destroy(true, { children: true, texture: true, baseTexture: true });
}

export function parseColor(input, fallback = 0x000000, fallbackAlpha = 1) {
    if (input == null) return { color: fallback, alpha: fallbackAlpha };
    if (typeof input === "number") return { color: input, alpha: fallbackAlpha };
    const value = String(input).trim();
    if (!value) return { color: fallback, alpha: fallbackAlpha };
    if (value === "transparent") return { color: fallback, alpha: 0 };
    if (value.startsWith("#")) {
        const hex = value.slice(1);
        if (hex.length === 3) {
            const r = parseInt(hex[0] + hex[0], 16);
            const g = parseInt(hex[1] + hex[1], 16);
            const b = parseInt(hex[2] + hex[2], 16);
            return { color: (r << 16) + (g << 8) + b, alpha: fallbackAlpha };
        }
        if (hex.length === 6 || hex.length === 8) {
            const r = parseInt(hex.slice(0, 2), 16);
            const g = parseInt(hex.slice(2, 4), 16);
            const b = parseInt(hex.slice(4, 6), 16);
            const alpha = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : fallbackAlpha;
            return { color: (r << 16) + (g << 8) + b, alpha };
        }
    }
    const match = value.match(/rgba?\(([^)]+)\)/i);
    if (match) {
        const parts = match[1].split(",").map((p) => p.trim());
        const r = Number(parts[0] ?? 0);
        const g = Number(parts[1] ?? 0);
        const b = Number(parts[2] ?? 0);
        const a = parts[3] != null ? Number(parts[3]) : fallbackAlpha;
        return {
            color: ((r & 255) << 16) + ((g & 255) << 8) + (b & 255),
            alpha: Number.isFinite(a) ? a : fallbackAlpha,
        };
    }
    return { color: fallback, alpha: fallbackAlpha };
}

export function setLineStyle(graphics, width, colorStr) {
    const { color, alpha } = parseColor(colorStr);
    graphics.lineStyle({ width, color, alpha, cap: "round", join: "round" });
}

export function beginFill(graphics, colorStr) {
    const { color, alpha } = parseColor(colorStr);
    graphics.beginFill(color, alpha);
}

export function drawRoundedRect(graphics, x, y, w, h, r) {
    if (typeof graphics.roundRect === "function") {
        graphics.roundRect(x, y, w, h, r);
    } else {
        graphics.drawRoundedRect(x, y, w, h, r);
    }
}

export function clearTextLayer(container) {
    const children = container.removeChildren();
    for (const child of children) {
        child.destroy();
    }
}

export function addText(container, text, style, x, y, anchorX = 0, anchorY = 0, rotation = 0) {
    const label = new Text(text, style);
    label.x = x;
    label.y = y;
    label.anchor.set(anchorX, anchorY);
    if (rotation) label.rotation = rotation;
    container.addChild(label);
    return label;
}

export function drawDashedLine(graphics, x1, y1, x2, y2, dash = 6, gap = 4) {
    const dx = x2 - x1;
    const dy = y2 - y1;
    const len = Math.sqrt(dx * dx + dy * dy);
    if (!len) return;
    const ux = dx / len;
    const uy = dy / len;
    let dist = 0;
    while (dist < len) {
        const seg = Math.min(dash, len - dist);
        const sx = x1 + ux * dist;
        const sy = y1 + uy * dist;
        const ex = x1 + ux * (dist + seg);
        const ey = y1 + uy * (dist + seg);
        graphics.moveTo(sx, sy);
        graphics.lineTo(ex, ey);
        dist += dash + gap;
    }
}

export function drawDashedPolyline(graphics, points, dash = 6, gap = 4) {
    if (!points.length) return;
    for (let i = 1; i < points.length; i += 1) {
        const [x1, y1] = points[i - 1];
        const [x2, y2] = points[i];
        drawDashedLine(graphics, x1, y1, x2, y2, dash, gap);
    }
}
