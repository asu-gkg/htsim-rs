<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">TCP 机制拆解</div>
            <div class="canvas-meta">Sequence-Time / Window / RTT-RTO / 状态机</div>
        </div>
        <canvas ref="canvas" class="tcp-detail" width="1100" height="800"></canvas>
        <div class="legend">
            蓝色竖线=发送数据段，橙色=重传，绿色三角=ACK，红色=ECN Echo/RTO。
        </div>
    </div>
</template>

<script setup>
import { onBeforeUnmount, onMounted, ref } from "vue";

defineProps({});

const emit = defineEmits(["ready"]);
const canvas = ref(null);

onMounted(() => {
    emit("ready", canvas.value);
});

onBeforeUnmount(() => {
    emit("ready", null);
});
</script>
