<template>
    <div class="canvas-card">
        <div class="canvas-header">
            <div class="canvas-title">TCP / DCTCP 时序</div>
            <div class="canvas-meta">cwnd、ssthresh、inflight</div>
        </div>
        <canvas ref="canvas" class="tcp" width="1100" height="200"></canvas>
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
