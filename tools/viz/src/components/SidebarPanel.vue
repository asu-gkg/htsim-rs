<template>
    <aside>
        <h1>htsim-rs 可视化回放</h1>

        <div class="grid">
            <label>1) 选择 JSON 事件文件</label>
            <div
                class="dropzone"
                :class="{ 'dropzone-active': isDragging }"
                @dragover.prevent="onDragOver"
                @dragleave.prevent="onDragLeave"
                @drop.prevent="onDrop"
                @click="triggerFileInput"
            >
                <div class="dropzone-content">
                    <span class="dropzone-icon">📁</span>
                    <span v-if="isDragging">松开以上传文件</span>
                    <span v-else>拖放 JSON 文件到这里<br />或点击选择</span>
                </div>
                <input
                    ref="fileInput"
                    type="file"
                    accept=".json,application/json"
                    @change="actions.onFile"
                    style="display: none"
                />
            </div>
            <div class="small">
                示例：<span class="kbd">cargo run --bin dumbbell_tcp -- --viz-json out.json</span>
            </div>
        </div>

        <div class="section grid">
            <h2>Topology</h2>
            <label>拓扑渲染模式</label>
            <select v-model="state.layoutChoice" :disabled="!hasEvents">
                <option value="auto">自动识别</option>
                <option value="fat-tree">Fat-tree</option>
                <option value="dumbbell">Dumbbell</option>
                <option value="circle">Circle</option>
            </select>
            <div class="small">
                自动识别：<strong>{{ layoutDetectedLabel }}</strong>
            </div>
            <div class="chips">
                <span class="chip">Nodes: {{ metaNodesCount }}</span>
                <span class="chip">Links: {{ metaLinksCount }}</span>
                <span class="chip">Events: {{ state.events.length }}</span>
            </div>
        </div>

        <div class="section grid">
            <h2>过滤</h2>
            <label>flow_id</label>
            <input type="text" v-model="state.filterFlow" placeholder="例如 1" :disabled="!hasEvents" />
            <label>pkt_id</label>
            <input type="text" v-model="state.filterPkt" placeholder="例如 42" :disabled="!hasEvents" />
            <div class="small">输入即生效，过滤后会重置回放状态。</div>
        </div>

        <div class="section grid">
            <h2>TCP 连接</h2>
            <label>cwnd 曲线</label>
            <select v-model="state.connPick" :disabled="state.connOptions.length === 0">
                <option value="auto">auto</option>
                <option v-for="cid in state.connOptions" :key="cid" :value="String(cid)">{{ cid }}</option>
            </select>
            <div class="small">优先 flow_id，其次自动选当前时间点最近的连接。</div>
        </div>

        <div class="section grid">
            <h2>控制</h2>
            <div class="btns">
                <button @click="actions.play" :disabled="!canPlay">播放</button>
                <button class="secondary" @click="actions.pause" :disabled="!canPlay">暂停</button>
                <button class="secondary" @click="actions.step" :disabled="!canPlay">单步</button>
            </div>
            <button class="secondary" @click="actions.jumpToDrop" :disabled="!canPlay">跳到下一次丢包</button>
            <div class="row">
                <div>
                    <label>速度</label>
                    <select v-model.number="state.speed" :disabled="!canPlay">
                        <option :value="0.25">0.25x</option>
                        <option :value="0.5">0.5x</option>
                        <option :value="1">1x</option>
                        <option :value="2">2x</option>
                        <option :value="5">5x</option>
                        <option :value="10">10x</option>
                    </select>
                </div>
                <div>
                    <label>回放时长（秒）</label>
                    <select v-model.number="state.targetWallSec" :disabled="!canPlay">
                        <option :value="3">3s</option>
                        <option :value="6">6s</option>
                        <option :value="12">12s</option>
                        <option :value="20">20s</option>
                        <option :value="40">40s</option>
                    </select>
                </div>
            </div>
        </div>

        <div class="section timeline">
            <h2>时间轴</h2>
            <input type="range" min="0" max="1000" v-model.number="state.slider" @input="actions.onSlider" :disabled="!canPlay" />
            <div class="small">
                <span class="kbd">空格</span>播放/暂停 <span class="kbd">←/→</span>单步
            </div>
            <div class="small status-line" :title="statusText">{{ statusText }}</div>
        </div>

        <div class="section grid">
            <h2>当前事件</h2>
            <pre class="log">{{ state.curText }}</pre>
        </div>

        <div class="section grid">
            <h2>节点 / 链路状态</h2>
            <pre class="log">{{ state.statsText }}</pre>
        </div>
    </aside>
</template>

<script setup>
import { inject, ref } from "vue";

const player = inject("player");
if (!player) {
    throw new Error("player store not provided");
}

const state = player.state;
const { hasEvents, canPlay, metaNodesCount, metaLinksCount, layoutDetectedLabel, statusText } = player.computed;
const actions = player.actions;

const fileInput = ref(null);
const isDragging = ref(false);

function triggerFileInput() {
    fileInput.value?.click();
}

function onDragOver() {
    isDragging.value = true;
}

function onDragLeave() {
    isDragging.value = false;
}

function onDrop(e) {
    isDragging.value = false;
    const files = e.dataTransfer?.files;
    if (files?.length) {
        // 创建一个模拟的 event 对象传给 onFile
        actions.onFile({ target: { files } });
    }
}
</script>

<style scoped>
.dropzone {
    border: 2px dashed #cbd5e1;
    border-radius: 8px;
    padding: 20px 16px;
    text-align: center;
    cursor: pointer;
    transition: all 0.2s ease;
    background: #f8fafc;
}

.dropzone:hover {
    border-color: #94a3b8;
    background: #f1f5f9;
}

.dropzone-active {
    border-color: #3b82f6;
    background: #eff6ff;
    border-style: solid;
}

.dropzone-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    color: #64748b;
    font-size: 12px;
    line-height: 1.4;
}

.dropzone-icon {
    font-size: 24px;
}

.dropzone-active .dropzone-content {
    color: #3b82f6;
}
</style>
