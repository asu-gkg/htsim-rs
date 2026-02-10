<template>
    <div class="editor-shell">
        <header class="editor-hero">
            <div>
                <h1>仿真运行</h1>
                <p class="editor-subtitle">
                    运行 <span class="kbd">workload_sim</span> 生成可回放的事件 JSON（<span class="kbd">out.json</span>）。
                </p>
            </div>
        </header>

        <div class="editor-grid">
            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>输入 workload</h2>
                        <span class="chip">viz/workloads</span>
                    </div>

                    <div class="grid">
                        <label>文件</label>
                        <select v-model="localWorkloadKey">
                            <option value="">未选择</option>
                            <option v-for="item in localWorkloads" :key="item.key" :value="item.key">
                                {{ item.label }}
                            </option>
                        </select>
                    </div>

                    <button type="button" @click="loadWorkload" :disabled="!localWorkloadKey">加载</button>
                    <div class="small status-line">{{ loadStatus }}</div>
                    <div class="small">将文件放到 <span class="kbd">viz/workloads/</span>，重启 dev server 刷新列表。</div>

                    <div v-if="loadedChips.length" class="chips">
                        <span v-for="(text, idx) in loadedChips" :key="`workload-chip-${idx}`" class="chip">{{ text }}</span>
                    </div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>参数</h2>
                        <span class="chip">workload_sim</span>
                    </div>
                    <div class="grid">
                        <label>输出事件</label>
                        <input v-model="simOutputName" type="text" placeholder="out.json" />
                    </div>
                    <div class="row">
                        <label class="checkbox-line">
                            <input v-model="simFctStats" type="checkbox" />
                            统计 FCT
                        </label>
                        <div class="grid">
                            <label>until_ms</label>
                            <input v-model="simUntilMs" type="text" placeholder="可选" />
                        </div>
                    </div>
                    <button type="button" @click="runWorkloadSim" :disabled="!canRun">一键生成 out.json</button>
                    <div class="small status-line">{{ simStatus }}</div>
                    <div v-if="simCommand" class="small status-line">命令: {{ simCommand }}</div>
                    <pre v-if="simLog" class="log">{{ simLog }}</pre>
                    <div class="small">输出写入 <span class="kbd">viz/outputs/</span>，回到回放页选择该 JSON 文件。</div>
                </section>
            </div>

            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>已加载</h2>
                        <span class="chip">{{ loadedLabel || "未加载" }}</span>
                    </div>
                    <div v-if="loadedChips.length" class="chips">
                        <span v-for="(text, idx) in loadedChips" :key="`workload-chip-${idx}`" class="chip">{{ text }}</span>
                    </div>
                    <div v-else class="small">请选择 workload.json 并点击加载。</div>
                </section>
            </div>
        </div>
    </div>
</template>

<script setup>
import { computed, ref, watch } from "vue";

function parseNumber(raw, fallback = NaN) {
    if (raw == null) return fallback;
    const clean = String(raw).replace(/_/g, "").trim();
    if (!clean) return fallback;
    const value = Number(clean);
    return Number.isFinite(value) ? value : fallback;
}

const localWorkloadFiles = import.meta.glob("../../workloads/*.json", { eager: true, as: "raw" });
const localWorkloads = Object.entries(localWorkloadFiles)
    .map(([key, raw]) => ({
        key,
        label: key.split("/").slice(-1)[0] || key,
        raw,
    }))
    .sort((a, b) => a.label.localeCompare(b.label));

const localWorkloadKey = ref("");
const loadedWorkload = ref(null);
const loadedLabel = ref("");
const loadStatus = ref(localWorkloads.length ? "请选择 workload.json 并点击加载。" : "未发现 workload.json。");

watch(localWorkloadKey, () => {
    loadedWorkload.value = null;
    loadedLabel.value = "";
    loadStatus.value = localWorkloads.length ? "请选择 workload.json 并点击加载。" : "未发现 workload.json。";
});

const isLoaded = computed(() => Boolean(loadedWorkload.value));
const loadedChips = computed(() => {
    const data = loadedWorkload.value;
    if (!data) return [];
    const chips = [];
    if (data.schema_version != null) chips.push(`schema:v${data.schema_version}`);
    if (data.topology?.kind) chips.push(`topo:${data.topology.kind}`);
    if (data.meta?.model) chips.push(`model:${data.meta.model}`);
    if (data.meta?.device) chips.push(`gpu:${data.meta.device}`);
    if (Array.isArray(data.hosts)) chips.push(`hosts:${data.hosts.length}`);
    if (Array.isArray(data.ranks)) chips.push(`ranks:${data.ranks.length}`);
    if (Array.isArray(data.steps)) chips.push(`steps:${data.steps.length}`);
    return chips;
});

const simOutputName = ref("out.json");
const simUntilMs = ref("");
const simFctStats = ref(true);
const simStatus = ref("");
const simCommand = ref("");
const simLog = ref("");
const simRunning = ref(false);

const canRun = computed(() => {
    if (simRunning.value) return false;
    return isLoaded.value;
});

function loadWorkload() {
    const entry = localWorkloads.find((item) => item.key === localWorkloadKey.value);
    if (!entry) {
        loadStatus.value = "未找到选择的 workload.json。";
        return;
    }
    try {
        const data = JSON.parse(entry.raw);
        loadedWorkload.value = data;
        loadedLabel.value = entry.label;
        loadStatus.value = `已加载 ${entry.label}。`;
    } catch (err) {
        loadedWorkload.value = null;
        loadedLabel.value = "";
        loadStatus.value = `解析失败：${entry.label}。`;
    }
}

async function runWorkloadSim() {
    if (!canRun.value) return;

    const outputName = simOutputName.value.trim() || "out.json";
    const until = parseNumber(simUntilMs.value, NaN);
    const payload = {
        output_name: outputName,
        fct_stats: simFctStats.value,
    };

    const workloadPath = `viz/workloads/${loadedLabel.value || "workload.json"}`;
    payload.workload_path = workloadPath;

    if (Number.isFinite(until)) {
        payload.until_ms = Math.max(0, Math.floor(until));
    }

    const commandParts = [
        "cargo run --bin workload_sim -- --workload",
        workloadPath,
        "--viz-json",
        `viz/outputs/${outputName}`,
    ];
    if (simFctStats.value) commandParts.push("--fct-stats");
    if (Number.isFinite(until)) commandParts.push(`--until-ms ${Math.max(0, Math.floor(until))}`);
    simCommand.value = commandParts.join(" ");

    simStatus.value = "运行中，请稍候...";
    simLog.value = "";
    simRunning.value = true;
    try {
        const resp = await fetch("/api-sim/run", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
        });
        let data = null;
        try {
            data = await resp.json();
        } catch (parseErr) {
            data = null;
        }
        if (data?.stdout || data?.stderr) {
            simLog.value = [data.stdout, data.stderr].filter(Boolean).join("\n");
        }
        if (!resp.ok || !data?.ok) {
            const errMsg = data?.error || `backend status ${resp.status}`;
            simStatus.value = `运行失败：${errMsg}`;
            return;
        }
        simStatus.value = `完成：${data.output_path || "已生成 out.json"}`;
    } catch (err) {
        const message = err?.message || "unknown error";
        simStatus.value = `运行失败：${message}`;
    } finally {
        simRunning.value = false;
    }
}
</script>
