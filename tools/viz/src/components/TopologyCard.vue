<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">拓扑与流量</div>
            <div class="canvas-meta">{{ meta }}</div>
        </div>
        <canvas ref="canvas" class="topology" width="1100" height="220"></canvas>
        <div class="legend">
            蓝色圆点=Data，绿色圆点=ACK，红色 X=Drop。链路上的圆点会按
            <span class="kbd">tx_start.depart_ns → arrive_ns</span> 做线性动画。
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
