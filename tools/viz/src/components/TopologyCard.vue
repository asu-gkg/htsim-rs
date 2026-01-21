<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">拓扑与流量</div>
            <div class="canvas-meta">{{ meta }}</div>
        </div>
        <canvas ref="canvas" class="topology" width="1100" height="220"></canvas>
        <div class="legend">
            <strong>数据包</strong>：蓝色=Data，绿色=ACK。
            <strong>链路</strong>：虚线+橙色标签=瓶颈（低带宽）；颜色绿→黄→红=队列深度。
            标签：<span class="kbd">带宽</span>、<span class="kbd">q:当前/峰值</span>、<span class="kbd">drop:N</span>。
        </div>
    </div>
</template>

<script setup>
import { onBeforeUnmount, onMounted, ref } from "vue";

defineProps({
    meta: {
        type: String,
        default: "",
    },
});

const emit = defineEmits(["ready"]);
const canvas = ref(null);

onMounted(() => {
    emit("ready", canvas.value);
});

onBeforeUnmount(() => {
    emit("ready", null);
});
</script>
