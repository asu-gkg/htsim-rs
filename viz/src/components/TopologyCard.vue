<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">拓扑与流量</div>
            <div class="canvas-meta">{{ meta }}</div>
        </div>
        <div class="canvas-wrap">
            <canvas ref="canvas" class="topology" width="1100" height="220"></canvas>
            <div class="canvas-overlay" aria-hidden="true">
                <span
                    v-for="(label, idx) in state.netLabels"
                    :key="`net-label-${idx}`"
                    class="canvas-label"
                    :style="labelStyle(label)"
                >
                    {{ label.text }}
                </span>
            </div>
        </div>
        <div class="legend">
            <strong>数据包</strong>：蓝色=Data，绿色=ACK。
            <strong>链路</strong>：虚线+橙色标签=瓶颈（低带宽）；颜色绿→黄→红=队列深度。
            标签：<span class="kbd">带宽</span>、<span class="kbd">q:当前/峰值</span>、<span class="kbd">drop:N</span>。
        </div>
    </div>
</template>

<script setup>
import { inject, onBeforeUnmount, onMounted, ref } from "vue";

defineProps({
    meta: {
        type: String,
        default: "",
    },
});

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
