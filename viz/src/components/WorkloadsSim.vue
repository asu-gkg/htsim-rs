<template>
    <div class="editor-shell">
        <header class="editor-hero">
            <div>
                <h1>仿真运行（多租户）</h1>
                <p class="editor-subtitle">
                    运行 <span class="kbd">workloads_sim</span>：选择多个 <span class="kbd">workload.json</span>，自动分配到拓扑的 hosts 上并生成回放事件（<span class="kbd">out.json</span>）。
                </p>
            </div>
        </header>

        <div class="editor-grid">
            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>输入 workloads</h2>
                        <span class="chip">viz/workloads</span>
                    </div>

                    <div class="grid">
                        <label>文件（可多选）</label>
                        <select v-model="localWorkloadKeys" multiple size="10">
                            <option v-for="item in localWorkloads" :key="item.key" :value="item.key">
                                {{ item.label }}
                            </option>
                        </select>
                    </div>

                    <div class="row">
                        <button type="button" @click="loadWorkloads" :disabled="!localWorkloadKeys.length">加载</button>
                        <button class="secondary" type="button" @click="clearSelection" :disabled="!localWorkloadKeys.length">
                            清空选择
                        </button>
                    </div>

                    <div class="small status-line">{{ loadStatus }}</div>
                    <div class="small">将文件放到 <span class="kbd">viz/workloads/</span>，重启 dev server 刷新列表。</div>
                    <div class="small">fat_tree 拓扑会按 pod（视作数据中心）轮询分配 workload。</div>

                    <div v-if="loadedSummaryChips.length" class="chips">
                        <span v-for="(text, idx) in loadedSummaryChips" :key="`multi-summary-${idx}`" class="chip">
                            {{ text }}
                        </span>
                    </div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>参数</h2>
                        <span class="chip">workloads_sim</span>
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
                    <button type="button" @click="runWorkloadsSim" :disabled="!canRun">一键生成 out.json</button>
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
                        <span class="chip">{{ loadedWorkloads.length }}</span>
                    </div>

                    <div v-if="!loadedWorkloads.length" class="small">
                        请选择文件并点击加载。
                    </div>

                    <div v-else class="editor-table">
                        <div v-for="item in loadedWorkloads" :key="item.key" class="editor-table-note">
                            <div class="chips">
                                <span class="chip">{{ item.label }}</span>
                                <span v-for="(chip, idx) in item.chips" :key="`${item.key}-chip-${idx}`" class="chip">
                                    {{ chip }}
                                </span>
                            </div>
                        </div>
                    </div>
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

function chipsForWorkload(data) {
    if (!data) return [];
    const chips = [];
    if (data.schema_version != null) chips.push(`schema:v${data.schema_version}`);
    if (data.topology?.kind) chips.push(`topo:${data.topology.kind}`);
    if (Array.isArray(data.hosts)) chips.push(`hosts:${data.hosts.length}`);
    if (Array.isArray(data.ranks)) chips.push(`ranks:${data.ranks.length}`);
    if (Array.isArray(data.steps)) chips.push(`steps:${data.steps.length}`);
    return chips;
}

const localWorkloadFiles = import.meta.glob("../../workloads/*.json", { eager: true, as: "raw" });
const localWorkloads = Object.entries(localWorkloadFiles)
    .map(([key, raw]) => ({
        key,
        label: key.split("/").slice(-1)[0] || key,
        raw,
    }))
    .sort((a, b) => a.label.localeCompare(b.label));

const localWorkloadKeys = ref([]);
const loadedWorkloads = ref([]);
const loadStatus = ref(localWorkloads.length ? "请选择 workload.json（可多选）并点击加载。" : "未发现 workload.json。");

watch(localWorkloadKeys, () => {
    loadedWorkloads.value = [];
    loadStatus.value = localWorkloads.length ? "请选择 workload.json（可多选）并点击加载。" : "未发现 workload.json。";
});

const loadedSummaryChips = computed(() => {
    if (!loadedWorkloads.value.length) return [];
    const totalRanks = loadedWorkloads.value.reduce((sum, w) => sum + (w.data?.ranks?.length || 0), 0);
    const kinds = new Set(loadedWorkloads.value.map((w) => w.data?.topology?.kind).filter(Boolean));
    const topoKind = kinds.size === 1 ? Array.from(kinds)[0] : "mixed";
    return [`workloads:${loadedWorkloads.value.length}`, `total_ranks:${totalRanks}`, `topo:${topoKind}`];
});

function clearSelection() {
    localWorkloadKeys.value = [];
}

function loadWorkloads() {
    const keys = localWorkloadKeys.value || [];
    if (!keys.length) {
        loadStatus.value = "请选择 workload.json（可多选）并点击加载。";
        return;
    }
    const list = [];
    const errors = [];
    for (const key of keys) {
        const entry = localWorkloads.find((item) => item.key === key);
        if (!entry) {
            errors.push(`未找到: ${key}`);
            continue;
        }
        try {
            const data = JSON.parse(entry.raw);
            list.push({
                key: entry.key,
                label: entry.label,
                data,
                chips: chipsForWorkload(data),
            });
        } catch (err) {
            errors.push(`解析失败: ${entry.label}`);
        }
    }
    loadedWorkloads.value = list;
    if (!list.length) {
        loadStatus.value = errors.length ? errors[0] : "加载失败。";
        return;
    }
    loadStatus.value = errors.length ? `已加载 ${list.length} 个（${errors.length} 个失败）` : `已加载 ${list.length} 个 workload。`;
}

const simOutputName = ref("out.json");
const simUntilMs = ref("");
const simFctStats = ref(true);
const simStatus = ref("");
const simCommand = ref("");
const simLog = ref("");
const simRunning = ref(false);

const canRun = computed(() => {
    if (simRunning.value) return false;
    return loadedWorkloads.value.length > 0;
});

async function runWorkloadsSim() {
    if (!canRun.value) return;

    const outputName = simOutputName.value.trim() || "out.json";
    const until = parseNumber(simUntilMs.value, NaN);
    const workloadPaths = loadedWorkloads.value.map((w) => `viz/workloads/${w.label}`);
    const payload = {
        output_name: outputName,
        fct_stats: simFctStats.value,
        workload_paths: workloadPaths,
    };

    if (Number.isFinite(until)) {
        payload.until_ms = Math.max(0, Math.floor(until));
    }

    const commandParts = ["cargo run --bin workloads_sim --"];
    for (const p of workloadPaths) {
        commandParts.push(`--workload ${p}`);
    }
    commandParts.push("--viz-json", `viz/outputs/${outputName}`);
    if (simFctStats.value) commandParts.push("--fct-stats");
    if (Number.isFinite(until)) commandParts.push(`--until-ms ${Math.max(0, Math.floor(until))}`);
    simCommand.value = commandParts.join(" ");

    simStatus.value = "运行中，请稍候...";
    simLog.value = "";
    simRunning.value = true;
    try {
        const resp = await fetch("/api-sim/run-multi", {
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
