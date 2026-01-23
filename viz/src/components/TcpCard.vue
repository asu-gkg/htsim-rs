<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">TCP / DCTCP 时序</div>
            <div class="canvas-meta">4 子图（点击放大）：cwnd / ssthresh / inflight / 对比</div>
        </div>
        <div class="canvas-wrap">
            <canvas ref="canvas" class="tcp" width="1100" height="320"></canvas>
            <div class="canvas-overlay" aria-hidden="true">
                <span v-for="(label, idx) in state.tcpLabels" :key="`tcp-label-${idx}`" class="canvas-label" :style="labelStyle(label)">
                    {{ label.text }}
                </span>
            </div>
        </div>
    </div>
    <!-- 放大模态框 -->
    <Teleport to="body">
        <div v-if="modalVisible" class="tcp-modal-overlay" @click.self="closeModal">
            <div class="tcp-modal">
                <div class="tcp-modal-header">
                    <span class="tcp-modal-title">TCP / DCTCP 时序（放大视图）</span>
                    <button class="tcp-modal-close" @click="closeModal">✕</button>
                </div>
                <div class="canvas-wrap tcp-modal-wrap">
                    <canvas ref="modalCanvas" class="tcp-modal-canvas" width="1600" height="800"></canvas>
                    <div class="canvas-overlay" aria-hidden="true">
                        <span
                            v-for="(label, idx) in state.tcpModalLabels"
                            :key="`tcp-modal-label-${idx}`"
                            class="canvas-label"
                            :style="labelStyle(label)"
                        >
                            {{ label.text }}
                        </span>
                    </div>
                </div>
            </div>
        </div>
    </Teleport>
</template>

<script setup>
import { inject, onBeforeUnmount, onMounted, ref } from "vue";

defineProps({});

const player = inject("player");
if (!player) {
    throw new Error("player store not provided");
}

const state = player.state;

const emit = defineEmits(["ready", "modal-ready", "modal-close"]);
const canvas = ref(null);
const modalCanvas = ref(null);
const modalVisible = ref(false);

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

function openModal() {
    modalVisible.value = true;
    // 等 DOM 更新后再 emit modal-ready
    setTimeout(() => {
        emit("modal-ready", modalCanvas.value);
    }, 0);
}

function closeModal() {
    modalVisible.value = false;
    emit("modal-close");
}

// 暴露给父组件
defineExpose({ openModal, closeModal });

onMounted(() => {
    emit("ready", canvas.value);
});

onBeforeUnmount(() => {
    emit("ready", null);
});
</script>

<style scoped>
.tcp-modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
}
.tcp-modal {
    background: #fff;
    border-radius: 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.25);
    max-width: 95vw;
    max-height: 95vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
}
.tcp-modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid #e5e7eb;
}
.tcp-modal-title {
    font-weight: 600;
    font-size: 14px;
    color: #1e293b;
}
.tcp-modal-close {
    background: none;
    border: none;
    font-size: 18px;
    cursor: pointer;
    color: #64748b;
    padding: 4px 8px;
    border-radius: 4px;
}
.tcp-modal-close:hover {
    background: #f1f5f9;
    color: #1e293b;
}
.tcp-modal-canvas {
    display: block;
    width: min(1600px, 90vw);
    height: min(800px, 80vh);
}
</style>
