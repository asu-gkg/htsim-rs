<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">TCP 机制拆解</div>
            <div class="canvas-meta">Sequence-Time / Window / RTT-RTO / 状态机</div>
        </div>
        <div class="canvas-wrap">
            <canvas ref="canvas" class="tcp-detail" width="1100" height="800"></canvas>
            <div class="canvas-overlay" aria-hidden="true">
                <span
                    v-for="(label, idx) in state.tcpDetailLabels"
                    :key="`tcp-detail-label-${idx}`"
                    class="canvas-label"
                    :style="labelStyle(label)"
                >
                    {{ label.text }}
                </span>
            </div>
        </div>
        <div class="legend">
            蓝色竖线=发送数据段，橙色=重传，绿色三角=ACK，红色=ECN Echo/RTO。
        </div>
    </div>
</template>

<script setup>
import { inject, onBeforeUnmount, onMounted, ref } from "vue";

defineProps({});

const player = inject("player");
if (!player) {
    throw new Error("player store not provided");
}

const state = player.state;

const emit = defineEmits(["ready"]);
const canvas = ref(null);
const labelStyle = (label) => {
    const anchorX = Number(label.anchorX ?? 0);
    const anchorY = Number(label.anchorY ?? 0);
    const translate = `translate(${-anchorX * 100}%, ${-anchorY * 100}%)`;
    const rotate = label.rotation ? ` rotate(${label.rotation}rad)` : "";
    const style = {
        left: `${label.x}px`,
        top: `${label.y}px`,
        color: label.color,
        fontFamily: label.fontFamily,
        fontSize: `${label.fontSize}px`,
        fontWeight: label.fontWeight || "normal",
        transform: `${translate}${rotate}`,
    };
    if (label.background) style.background = label.background;
    if (label.borderColor) style.border = `${label.borderWidth || 1}px solid ${label.borderColor}`;
    if (label.borderRadius) style.borderRadius = `${label.borderRadius}px`;
    if (label.padding) style.padding = label.padding;
    if (label.boxShadow) style.boxShadow = label.boxShadow;
    return style;
};

onMounted(() => {
    emit("ready", canvas.value);
});

onBeforeUnmount(() => {
    emit("ready", null);
});
</script>
