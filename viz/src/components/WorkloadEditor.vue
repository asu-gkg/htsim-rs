<template>
    <div class="editor-shell">
        <header class="editor-hero">
            <div>
                <h1>{{ heroTitle }}</h1>
                <p class="editor-subtitle">{{ heroSubtitle }}</p>
            </div>
            <div class="editor-hero-actions">
                <button class="secondary" type="button" @click="loadSample">载入样例</button>
                <button type="button" @click="downloadJson">下载 JSON</button>
            </div>
        </header>

        <div class="editor-grid">
            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>本地 workload</h2>
                        <span class="chip">workload.json</span>
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
                    <button type="button" @click="loadLocalWorkload" :disabled="!localWorkloadKey">加载</button>
                    <div class="small status-line">{{ localWorkloadStatus }}</div>
                    <div class="small">将文件放到 `viz/workloads/`，重启 dev server 刷新列表。</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>仿真运行</h2>
                        <span class="chip">workload_sim</span>
                    </div>
                    <div class="grid">
                        <label>保存 workload</label>
                        <input v-model="simWorkloadName" type="text" placeholder="workload.json" />
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
                    <button type="button" @click="runWorkloadSim" :disabled="simRunning">一键生成 out.json</button>
                    <div class="small status-line">{{ simStatus }}</div>
                    <div v-if="simCommand" class="small status-line">命令: {{ simCommand }}</div>
                    <div class="small status-line">来源: {{ simSource }}</div>
                    <pre v-if="simLog" class="log">{{ simLog }}</pre>
                    <div class="small">会写入 `viz/workloads/` 与 `viz/outputs/`。</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>模型</h2>
                        <span class="chip">DLmodel_configs</span>
                    </div>
                    <div class="grid">
                        <label>模型配置</label>
                        <select v-model="modelName">
                            <option value="">未选择</option>
                            <option v-for="item in modelOptions" :key="item.value" :value="item.value">
                                {{ item.label }}
                            </option>
                        </select>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>层数</label>
                            <input :value="modelNumLayers" type="text" readonly />
                        </div>
                        <div class="grid">
                            <label>模型类型</label>
                            <input :value="modelType" type="text" readonly />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>最大序列</label>
                            <input :value="modelMaxPosition" type="text" readonly />
                        </div>
                        <div class="grid"></div>
                    </div>
                    <div class="small">读取 `NeuSight/scripts/asplos/data/DLmodel_configs`。</div>
                </section>

                <section v-if="showPrediction" class="editor-card">
                    <div class="editor-card-header">
                        <h2>预测数据</h2>
                        <span class="chip">prediction</span>
                    </div>
                    <div class="grid">
                        <label>GPU</label>
                        <select v-model="predictionGpu">
                            <option value="">使用 Hosts GPU</option>
                            <option v-for="item in gpuOptions" :key="item" :value="item">
                                {{ item }}
                            </option>
                        </select>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>预测器</label>
                            <select v-model="predictionPredictor">
                                <option v-for="item in predictorOptions" :key="item" :value="item">
                                    {{ item }}
                                </option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>模式</label>
                            <select v-model="predictionMode">
                                <option value="train">train</option>
                                <option value="inf">inf</option>
                            </select>
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>seq</label>
                            <input v-model="predictionSeq" type="text" list="seq-options" placeholder="例如 2" />
                            <datalist id="seq-options">
                                <option v-for="item in seqOptions" :key="item" :value="item"></option>
                            </datalist>
                        </div>
                        <div class="grid">
                            <label>batch</label>
                            <input v-model="predictionBatch" type="text" list="batch-options" placeholder="例如 512" />
                            <datalist id="batch-options">
                                <option v-for="item in batchOptions" :key="item" :value="item"></option>
                            </datalist>
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>DP</label>
                            <input v-model="parallelDp" type="text" placeholder="1" />
                        </div>
                        <div class="grid">
                            <label>TP</label>
                            <input v-model="parallelTp" type="text" placeholder="1" />
                        </div>
                        <div class="grid">
                            <label>PP</label>
                            <input v-model="parallelPp" type="text" placeholder="1" />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>PP microbatch</label>
                            <input v-model="parallelPpMicrobatch" type="text" placeholder="例如 1" />
                        </div>
                        <div class="grid">
                            <label>Pipeline</label>
                            <select v-model="pipelineSchedule">
                                <option value="1f1b">1f1b</option>
                                <option value="fwd_bwd">fwd_bwd</option>
                            </select>
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>GPU 预测</label>
                            <label class="checkbox-line">
                                <input v-model="useBackend" type="checkbox" />
                                使用后端
                            </label>
                        </div>
                        <div class="grid">
                            <label>后端</label>
                            <input type="text" :value="predictApi" readonly />
                        </div>
                    </div>
                    <button type="button" @click="buildFromPrediction" :disabled="!canBuildPrediction">生成 ranks</button>
                    <div class="small status-line">{{ predictionStatus }}</div>
                    <div class="small">options = {{ predictionOptions || "none" }}</div>
                    <div v-if="predictionRequestSummary" class="small status-line">
                        请求: {{ predictionRequestSummary }}
                    </div>
                    <div v-if="predictionResponseSummary" class="small status-line">
                        响应: {{ predictionResponseSummary }}
                    </div>
                    <div class="small">启动后端：`python3 tools/neusight_predict_server.py --port 3099`。</div>
                </section>

                <section v-if="showHook" class="editor-card">
                    <div class="editor-card-header">
                        <h2>真实测量生成器</h2>
                        <span class="chip">workload_gen</span>
                    </div>
                    <div class="grid">
                        <label>GPU</label>
                        <input :value="gpuModel" type="text" readonly />
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>模式</label>
                            <select v-model="hookMode">
                                <option value="train">train</option>
                                <option value="inf">inf</option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>dtype</label>
                            <select v-model="hookDtype">
                                <option value="fp16">fp16</option>
                                <option value="bf16">bf16</option>
                                <option value="fp32">fp32</option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>device</label>
                            <select v-model="hookDevice">
                                <option value="cuda">cuda</option>
                                <option value="cpu">cpu</option>
                            </select>
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>seq</label>
                            <input v-model="hookSeq" type="text" placeholder="例如 1024" />
                        </div>
                        <div class="grid">
                            <label>batch</label>
                            <input v-model="hookBatch" type="text" placeholder="例如 8" />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>DP</label>
                            <input v-model="parallelDp" type="text" placeholder="1" />
                        </div>
                        <div class="grid">
                            <label>TP</label>
                            <input v-model="parallelTp" type="text" placeholder="1" />
                        </div>
                        <div class="grid">
                            <label>PP</label>
                            <input v-model="parallelPp" type="text" placeholder="1" />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>PP microbatch</label>
                            <input v-model="parallelPpMicrobatch" type="text" placeholder="例如 1" />
                        </div>
                        <div class="grid">
                            <label>Pipeline</label>
                            <select v-model="pipelineSchedule">
                                <option value="1f1b">1f1b</option>
                                <option value="fwd_bwd">fwd_bwd</option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>TP comm factor</label>
                            <input v-model="hookTpCommFactor" type="text" placeholder="例如 2" />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>warmup</label>
                            <input v-model="hookWarmup" type="text" placeholder="例如 1" />
                        </div>
                        <div class="grid">
                            <label>measure</label>
                            <input v-model="hookSteps" type="text" placeholder="例如 1" />
                        </div>
                        <div class="grid"></div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>后端</label>
                            <input type="text" :value="hookApi" readonly />
                        </div>
                    </div>
                    <button type="button" @click="buildFromHook" :disabled="!canBuildHook">生成 workload</button>
                    <div class="small status-line">{{ hookStatus }}</div>
                    <div v-if="hookRequestSummary" class="small status-line">
                        请求: {{ hookRequestSummary }}
                    </div>
                    <div v-if="hookResponseSummary" class="small status-line">
                        响应: {{ hookResponseSummary }}
                    </div>
                    <div class="small">启动后端：`cd workload_gen && python3 -m workload_gen.server --port 3100`。</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>拓扑</h2>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>拓扑类型</label>
                            <select v-model="topology.kind">
                                <option value="dumbbell">dumbbell</option>
                                <option value="fat_tree">fat_tree</option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>默认协议</label>
                            <select v-model="defaults.protocol">
                                <option value="tcp">tcp</option>
                                <option value="dctcp">dctcp</option>
                            </select>
                        </div>
                    </div>
                    <div v-if="topology.kind === 'dumbbell'" class="grid">
                        <div class="row">
                            <div class="grid">
                                <label>host_link_gbps</label>
                                <input v-model="topology.host_link_gbps" type="text" />
                            </div>
                            <div class="grid">
                                <label>bottleneck_gbps</label>
                                <input v-model="topology.bottleneck_gbps" type="text" />
                            </div>
                        </div>
                        <div class="grid">
                            <label>link_latency_us</label>
                            <input v-model="topology.link_latency_us" type="text" />
                        </div>
                    </div>
                    <div v-else class="grid">
                        <div class="row">
                            <div class="grid">
                                <label>k</label>
                                <input v-model="topology.k" type="text" />
                            </div>
                            <div class="grid">
                                <label>link_gbps</label>
                                <input v-model="topology.link_gbps" type="text" />
                            </div>
                        </div>
                        <div class="grid">
                            <label>link_latency_us</label>
                            <input v-model="topology.link_latency_us" type="text" />
                        </div>
                    </div>
                    <div class="small status-line">{{ topologyHint }}</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>Hosts</h2>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>host 数量</label>
                            <input :value="hostCount" type="text" readonly />
                        </div>
                        <div class="grid">
                            <label>GPU 型号</label>
                            <input v-model="gpuModel" type="text" placeholder="例如 NVIDIA_A100" />
                        </div>
                    </div>
                    <div class="small">默认使用 topo_index = id，生成 rank0..rankN。</div>
                </section>

            </div>

            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>Ranks</h2>
                        <div class="editor-card-actions">
                            <button class="secondary" type="button" @click="addStep">新增 step</button>
                            <button class="secondary" type="button" @click="reindexSteps">重排 id</button>
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>Rank</label>
                            <select v-model="selectedRankId">
                                <option v-for="rank in ranks" :key="rank.id" :value="String(rank.id)">
                                    rank{{ rank.id }}
                                </option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>Steps</label>
                            <input :value="selectedRankSteps.length" type="text" readonly />
                        </div>
                    </div>
                    <div class="editor-table">
                        <div class="editor-table-row editor-table-head">
                            <div>ID</div>
                            <div>Label</div>
                            <div>Kind</div>
                            <div>Compute (ms)</div>
                            <div>Comm (bytes)</div>
                            <div>Comm ID</div>
                            <div>Op</div>
                            <div>Hosts</div>
                            <div>Peer</div>
                            <div>Dir</div>
                            <div></div>
                        </div>
                        <div
                            v-for="(step, i) in selectedRankSteps"
                            :key="step.key"
                            class="editor-table-row"
                        >
                            <input v-model="step.id" type="text" />
                            <input v-model="step.label" type="text" placeholder="optional" />
                            <select v-model="step.kind">
                                <option value="compute">compute</option>
                                <option value="collective">collective</option>
                                <option value="sendrecv">sendrecv</option>
                            </select>
                            <input v-model="step.compute_ms" type="text" :disabled="step.kind !== 'compute'" />
                            <input v-model="step.comm_bytes" type="text" :disabled="step.kind === 'compute'" />
                            <input v-model="step.comm_id" type="text" :disabled="step.kind === 'compute'" />
                            <input v-model="step.op" type="text" :disabled="step.kind !== 'collective'" />
                            <input
                                v-model="step.hosts"
                                type="text"
                                :placeholder="step.kind === 'collective' ? '0,1,2' : ''"
                                :disabled="step.kind !== 'collective'"
                            />
                            <input v-model="step.peer" type="text" placeholder="rank id" :disabled="step.kind !== 'sendrecv'" />
                            <select v-model="step.direction" :disabled="step.kind !== 'sendrecv'">
                                <option value="">-</option>
                                <option value="send">send</option>
                                <option value="recv">recv</option>
                            </select>
                            <button class="secondary" type="button" @click="removeStep(i)">删除</button>
                        </div>
                    </div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>JSON 预览</h2>
                        <div class="editor-card-actions">
                            <button class="secondary" type="button" @click="copyJson">复制</button>
                        </div>
                    </div>
                    <textarea ref="jsonArea" class="editor-json" :value="jsonText" readonly></textarea>
                    <div class="small status-line">{{ copyStatus }}</div>
                </section>
            </div>
        </div>
    </div>
</template>

<script setup>
import { computed, reactive, ref, watch } from "vue";

const props = defineProps({
    generator: {
        type: String,
        default: "prediction",
    },
});

const showPrediction = computed(() => props.generator === "prediction");
const showHook = computed(() => props.generator === "hook");
const heroTitle = computed(() =>
    showPrediction.value ? "NeuSight Workload 生成器" : "真实 Workload 生成器"
);
const heroSubtitle = computed(() =>
    showPrediction.value
        ? "基于 NeuSight 预测 CSV 生成 `workload.json`。"
        : "基于 PyTorch hooks 真实测量生成 `workload.json`。"
);

function createUuid() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
        return crypto.randomUUID();
    }
    if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
        const bytes = new Uint8Array(16);
        crypto.getRandomValues(bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0"));
        return `${hex[0]}${hex[1]}${hex[2]}${hex[3]}-${hex[4]}${hex[5]}-${hex[6]}${hex[7]}-${hex[8]}${hex[9]}-${hex[10]}${hex[11]}${hex[12]}${hex[13]}${hex[14]}${hex[15]}`;
    }
    return `uuid-${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

const modelConfigFiles = import.meta.glob("../../../NeuSight/scripts/asplos/data/DLmodel_configs/*.json", {
    eager: true,
    as: "raw",
});
const baseModelOptions = Object.entries(modelConfigFiles)
    .map(([path, raw]) => {
        const name = path.split("/").pop()?.replace(/\.json$/, "") || path;
        try {
            const config = JSON.parse(raw);
            return { value: name, label: name, config };
        } catch (err) {
            return null;
        }
    })
    .filter(Boolean)
    .sort((a, b) => a.label.localeCompare(b.label));
const modelConfigMap = new Map(baseModelOptions.map((item) => [item.value, item.config]));

const modelName = ref("bert");
const modelNumLayers = ref("");
const modelType = ref("");
const modelMaxPosition = ref("");
const hookMode = ref("train");
const hookSeq = ref("2");
const hookBatch = ref("512");
const hookDtype = ref("fp16");
const hookDevice = ref("cuda");
const hookWarmup = ref("1");
const hookSteps = ref("1");
const hookTpCommFactor = ref("2");
const hookStatus = ref("请选择模型与 GPU。");
const hookRequestSummary = ref("");
const hookResponseSummary = ref("");
const hookProxyApi = "/api-workload/workload";
const hookDirectApi = computed(() => {
    if (typeof window === "undefined") return "http://127.0.0.1:3100/api/workload";
    const host = window.location.hostname || "127.0.0.1";
    return `http://${host}:3100/api/workload`;
});
const modelOptions = computed(() => {
    const current = modelName.value.trim();
    if (!current || baseModelOptions.some((item) => item.value === current)) {
        return baseModelOptions;
    }
    return [{ value: current, label: `${current} (custom)`, config: null }, ...baseModelOptions];
});

watch(
    modelName,
    (next) => {
        const info = modelConfigMap.get(next);
        if (info) {
            const layers = info.num_hidden_layers != null ? info.num_hidden_layers : info.num_layers;
            modelNumLayers.value = layers != null ? String(layers) : "";
            modelType.value = info.model_type != null ? String(info.model_type) : "";
            const maxPos =
                info.max_position_embeddings != null
                    ? info.max_position_embeddings
                    : info.n_ctx != null
                    ? info.n_ctx
                    : info.n_positions != null
                    ? info.n_positions
                    : "";
            modelMaxPosition.value = maxPos != null ? String(maxPos) : "";
            const maxPosNum = parseNumber(modelMaxPosition.value, NaN);
            if (Number.isFinite(maxPosNum)) {
                const seqNum = parseNumber(hookSeq.value, NaN);
                if (!Number.isFinite(seqNum) || seqNum > maxPosNum) {
                    hookSeq.value = String(maxPosNum);
                }
            }
            return;
        }
        if (!next) {
            modelNumLayers.value = "";
            modelType.value = "";
            modelMaxPosition.value = "";
            return;
        }
        modelType.value = "";
        modelMaxPosition.value = "";
    },
    { immediate: true }
);

const localWorkloadFiles = import.meta.glob("../../workloads/*.json", { eager: true, as: "raw" });
const localWorkloads = Object.entries(localWorkloadFiles)
    .map(([key, raw]) => ({
        key,
        label: key.split("/").slice(-1)[0] || key,
        raw,
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
const localWorkloadKey = ref("");
const localWorkloadStatus = ref(
    localWorkloads.length ? "请选择本地 workload.json。" : "未发现本地 workload.json。"
);
const simWorkloadName = ref("workload.json");
const simOutputName = ref("out.json");
const simUntilMs = ref("");
const simFctStats = ref(true);
const simStatus = ref("");
const simCommand = ref("");
const simLog = ref("");
const simRunning = ref(false);
const copyStatus = ref("");
const metaSource = ref("");

const simSource = computed(() => {
    if (!localWorkloadKey.value) return "编辑器 JSON";
    const entry = localWorkloads.find((item) => item.key === localWorkloadKey.value);
    return entry ? `本地文件: ${entry.label}` : "编辑器 JSON";
});

const topology = reactive({
    kind: "fat_tree",
    host_link_gbps: "100",
    bottleneck_gbps: "10",
    link_latency_us: "2",
    k: "4",
    link_gbps: "100",
});

const defaults = reactive({
    protocol: "tcp",
});

const gpuModel = ref("NVIDIA_RTX_2080_Ti");
const topologyHint = computed(() => {
    const count = hostCountValue.value || 0;
    if (topology.kind === "dumbbell") {
        const capacity = 2;
        if (count > capacity) {
            return `dumbbell 仅支持 ${capacity} hosts（当前 ${count}）。`;
        }
        return `dumbbell 容量 ${capacity} hosts（当前 ${count}）。`;
    }
    const k = parsePositiveInt(topology.k, null);
    if (!k) {
        return "fat_tree 需要偶数 k。";
    }
    if (k % 2 !== 0) {
        return "fat_tree 需要偶数 k。";
    }
    const capacity = Math.trunc((k * k * k) / 4);
    if (count > capacity) {
        return `fat_tree k=${k} 仅支持 ${capacity} hosts（当前 ${count}）。`;
    }
    return `fat_tree k=${k} 容量 ${capacity} hosts（当前 ${count}）。`;
});

const predictionLoaders = import.meta.glob("../../../NeuSight/scripts/asplos/results/prediction/*/*/*.csv", {
    as: "raw",
});
const deviceConfigFiles = import.meta.glob("../../../NeuSight/scripts/asplos/data/device_configs/*.json", {
    eager: true,
    as: "raw",
});
const deviceGpuOptions = Object.keys(deviceConfigFiles)
    .map((path) => path.split("/").pop()?.replace(/\.json$/, "") || path)
    .sort();
const predictionEntries = Object.keys(predictionLoaders)
    .map((path) => {
        const parts = path.split("/");
        const file = parts[parts.length - 1] || "";
        const predictor = parts[parts.length - 2] || "";
        const gpu = parts[parts.length - 3] || "";
        const match = file.match(/^(.*)-(train|inf)-(\d+)-(\d+)(?:-(.+))?\.csv$/);
        if (!match) return null;
        return {
            path,
            file,
            model: match[1],
            mode: match[2],
            seq: Number(match[3]),
            batch: Number(match[4]),
            predictor,
            gpu,
            options: match[5] || "",
        };
    })
    .filter(Boolean);
const gpuOptions = Array.from(
    new Set([...deviceGpuOptions, ...predictionEntries.map((item) => item.gpu)])
).sort();
const predictorOptions = Array.from(
    new Set([...predictionEntries.map((item) => item.predictor), "neusight", "micro", "roofline", "habitat"])
).sort();

const predictionGpu = ref("NVIDIA_A100-PCIE-40GB");
const predictionGpuResolved = computed(() => predictionGpu.value || gpuModel.value.trim());
const predictionPredictor = ref(
    predictorOptions.includes("neusight") ? "neusight" : predictorOptions[0] || ""
);
const predictionMode = ref("train");
const predictionSeq = ref("2");
const predictionBatch = ref("512");
const parallelDp = ref("2");
const parallelTp = ref("1");
const parallelPp = ref("1");
const parallelPpMicrobatch = ref("1");
const pipelineSchedule = ref("1f1b");
const predictionStatus = ref("请选择模型与 GPU。");
const predictionRequestSummary = ref("");
const predictionResponseSummary = ref("");
const useBackend = ref(true);
const predictApi = "/api/predict";
const jsonArea = ref(null);
const dpDegree = computed(() => parsePositiveInt(parallelDp.value));
const tpDegree = computed(() => parsePositiveInt(parallelTp.value));
const ppDegree = computed(() => parsePositiveInt(parallelPp.value));
const ppMicrobatch = computed(() => parsePositiveInt(parallelPpMicrobatch.value, 1));
const hostCountValue = computed(() => {
    const dp = dpDegree.value || 1;
    const tp = tpDegree.value || 1;
    const pp = ppDegree.value || 1;
    return dp * tp * pp;
});
const hostCount = computed(() => String(hostCountValue.value));

const filteredPredictions = computed(() => {
    const model = modelName.value.trim();
    const gpu = predictionGpuResolved.value;
    const predictor = predictionPredictor.value;
    const mode = predictionMode.value;
    const options = predictionOptions.value;
    if (!model || !gpu || !predictor || !mode) return [];
    return predictionEntries.filter(
        (item) =>
            item.model === model &&
            item.gpu === gpu &&
            item.predictor === predictor &&
            item.mode === mode &&
            (options ? item.options === options : !item.options)
    );
});
const seqOptions = computed(() => {
    const set = new Set(filteredPredictions.value.map((item) => item.seq));
    return Array.from(set).sort((a, b) => a - b);
});
const batchOptions = computed(() => {
    const seq = Number(predictionSeq.value);
    const pool = Number.isFinite(seq)
        ? filteredPredictions.value.filter((item) => item.seq === seq)
        : filteredPredictions.value;
    const set = new Set(pool.map((item) => item.batch));
    return Array.from(set).sort((a, b) => a - b);
});
const selectedPrediction = computed(() => {
    const seq = Number(predictionSeq.value);
    const batch = Number(predictionBatch.value);
    if (!Number.isFinite(seq) || !Number.isFinite(batch)) return null;
    return filteredPredictions.value.find((item) => item.seq === seq && item.batch === batch) || null;
});
const canBuildPrediction = computed(() => {
    const model = modelName.value.trim();
    const gpu = predictionGpuResolved.value;
    const predictor = predictionPredictor.value;
    const mode = predictionMode.value;
    const seq = Number(predictionSeq.value);
    const batch = Number(predictionBatch.value);
    if (!dpDegree.value || !tpDegree.value || !ppDegree.value) return false;
    if (ppDegree.value > 1) {
        const layers = parseNumber(modelNumLayers.value, 0);
        if (layers <= 0) return false;
        if (layers % ppDegree.value !== 0) return false;
    }
    return Boolean(
        model &&
            gpu &&
            predictor &&
            mode &&
            Number.isFinite(seq) &&
            Number.isFinite(batch)
    );
});
const canBuildHook = computed(() => {
    const model = modelName.value.trim();
    const gpu = gpuModel.value.trim();
    const seq = parseNumber(hookSeq.value, NaN);
    const batch = parseNumber(hookBatch.value, NaN);
    if (!dpDegree.value || !tpDegree.value || !ppDegree.value) return false;
    if (ppDegree.value > 1) {
        const layers = parseNumber(modelNumLayers.value, 0);
        if (layers <= 0) return false;
        if (layers % ppDegree.value !== 0) return false;
    }
    const maxPosNum = parseNumber(modelMaxPosition.value, NaN);
    if (Number.isFinite(maxPosNum) && seq > maxPosNum) return false;
    return Boolean(model && gpu && Number.isFinite(seq) && seq > 0 && Number.isFinite(batch) && batch > 0);
});

const ranks = ref([
    {
        id: 0,
        steps: [
            {
                key: createUuid(),
                id: "0",
                label: "step0",
                kind: "compute",
                compute_ms: "2.0",
                comm_bytes: "0",
                comm_id: "",
                op: "",
                hosts: "",
                peer: "",
                direction: "",
            },
        ],
    },
]);
const selectedRankId = ref("0");
const selectedRank = computed(
    () => ranks.value.find((rank) => String(rank.id) === selectedRankId.value) || null
);
const selectedRankSteps = computed(() => (selectedRank.value ? selectedRank.value.steps : []));

function parseNumber(raw, fallback = 0) {
    if (raw == null) return fallback;
    const clean = String(raw).replace(/_/g, "").trim();
    if (!clean) return fallback;
    const value = Number(clean);
    return Number.isFinite(value) ? value : fallback;
}

function parsePositiveInt(raw, fallback = null) {
    const value = parseNumber(raw, NaN);
    if (!Number.isFinite(value) || value < 1) return fallback;
    const rounded = Math.floor(value);
    if (rounded !== value) return fallback;
    return rounded;
}

const DEFAULT_SEQ = 2;
const DEFAULT_BATCH = 512;

const predictionOptions = computed(() => {
    if (!dpDegree.value || !tpDegree.value || !ppDegree.value) return "";
    const tokens = [];
    if (dpDegree.value > 1) tokens.push(`dp${dpDegree.value}`);
    if (tpDegree.value > 1) tokens.push(`tp${tpDegree.value}`);
    if (ppDegree.value > 1) {
        const micro = Math.max(1, ppMicrobatch.value || 1);
        tokens.push(`pp${ppDegree.value}_${micro}`);
    }
    return tokens.join(",");
});

function pickDefaultPrediction(entries) {
    if (!entries.length) return null;
    const preferred = entries.find((item) => item.seq === DEFAULT_SEQ && item.batch === DEFAULT_BATCH);
    return preferred || entries[0];
}

function updatePredictionStatus(entries) {
    const model = modelName.value.trim();
    const gpu = predictionGpuResolved.value;
    if (!model || !gpu) {
        predictionStatus.value = "请选择模型与 GPU。";
        return;
    }
    if (!dpDegree.value || !tpDegree.value || !ppDegree.value) {
        predictionStatus.value = "DP/TP/PP 需要是正整数。";
        return;
    }
    if (ppDegree.value > 1) {
        const layers = parseNumber(modelNumLayers.value, 0);
        if (layers <= 0) {
            predictionStatus.value = "PP>1 需要模型层数。";
            return;
        }
        if (layers % ppDegree.value !== 0) {
            predictionStatus.value = "层数需要能被 PP 整除。";
            return;
        }
    }
    if (!predictionSeq.value) predictionSeq.value = String(DEFAULT_SEQ);
    if (!predictionBatch.value) predictionBatch.value = String(DEFAULT_BATCH);

    const seq = Number(predictionSeq.value);
    const batch = Number(predictionBatch.value);
    const hasLocal =
        Number.isFinite(seq) &&
        Number.isFinite(batch) &&
        entries.some((item) => item.seq === seq && item.batch === batch);
    if (hasLocal) {
        predictionStatus.value = useBackend.value
            ? `已找到本地 CSV（${entries.length} 条）。`
            : `使用本地 CSV（${entries.length} 条）。`;
        return;
    }
    if (!entries.length) {
        predictionStatus.value = useBackend.value ? "未找到本地 CSV，将使用 GPU 预测。" : "未找到本地 CSV。";
        return;
    }
    if (!useBackend.value) {
        const pick = pickDefaultPrediction(entries);
        predictionSeq.value = pick ? String(pick.seq) : predictionSeq.value;
        predictionBatch.value = pick ? String(pick.batch) : predictionBatch.value;
        predictionStatus.value = "已选择本地 CSV。";
        return;
    }
    predictionStatus.value = "当前选择无本地 CSV，将使用 GPU 预测。";
}

function formatPredictionSummary(payload) {
    const parts = [
        `model=${payload.model}`,
        `gpu=${payload.gpu}`,
        `predictor=${payload.predictor}`,
        `mode=${payload.mode}`,
        `seq=${payload.seq}`,
        `batch=${payload.batch}`,
    ];
    if (payload.options) parts.push(`options=${payload.options}`);
    return parts.join(", ");
}

function updateHookStatus() {
    const model = modelName.value.trim();
    const gpu = gpuModel.value.trim();
    if (!model || !gpu) {
        hookStatus.value = "请选择模型与 GPU。";
        return;
    }
    if (!dpDegree.value || !tpDegree.value || !ppDegree.value) {
        hookStatus.value = "DP/TP/PP 需要是正整数。";
        return;
    }
    if (ppDegree.value > 1) {
        const layers = parseNumber(modelNumLayers.value, 0);
        if (layers <= 0) {
            hookStatus.value = "PP>1 需要模型层数。";
            return;
        }
        if (layers % ppDegree.value !== 0) {
            hookStatus.value = "层数需要能被 PP 整除。";
            return;
        }
    }
    const seq = parseNumber(hookSeq.value, NaN);
    const batch = parseNumber(hookBatch.value, NaN);
    if (!Number.isFinite(seq) || seq <= 0 || !Number.isFinite(batch) || batch <= 0) {
        hookStatus.value = "seq/batch 需要是正整数。";
        return;
    }
    const maxPosNum = parseNumber(modelMaxPosition.value, NaN);
    if (Number.isFinite(maxPosNum) && seq > maxPosNum) {
        hookStatus.value = `seq 不能超过模型最大序列 ${maxPosNum}。`;
        return;
    }
    hookStatus.value = "可生成 workload.json。";
}

function formatHookSummary(payload) {
    const parts = [
        `model=${payload.model}`,
        `gpu=${payload.gpu}`,
        `mode=${payload.mode}`,
        `seq=${payload.seq}`,
        `batch=${payload.batch}`,
        `dp=${payload.dp}`,
        `tp=${payload.tp}`,
        `pp=${payload.pp}`,
        `pp_microbatch=${payload.pp_microbatch}`,
        `dtype=${payload.dtype}`,
        `device=${payload.device}`,
    ];
    return parts.join(", ");
}

function buildTopologyPayload() {
    if (topology.kind === "fat_tree") {
        return {
            kind: "fat_tree",
            k: parseNumber(topology.k, 0),
            link_gbps: parseNumber(topology.link_gbps, 100),
            link_latency_us: parseNumber(topology.link_latency_us, 2),
        };
    }
    return {
        kind: "dumbbell",
        host_link_gbps: parseNumber(topology.host_link_gbps, 100),
        bottleneck_gbps: parseNumber(topology.bottleneck_gbps, 10),
        link_latency_us: parseNumber(topology.link_latency_us, 2),
    };
}

watch(filteredPredictions, (entries) => {
    updatePredictionStatus(entries);
});

watch(hostCountValue, () => {
    syncRanksWithHostCount();
});

watch([parallelDp, parallelTp, parallelPp, parallelPpMicrobatch, modelNumLayers], () => {
    updatePredictionStatus(filteredPredictions.value);
});

watch(useBackend, () => {
    updatePredictionStatus(filteredPredictions.value);
});
watch(
    [
        modelName,
        gpuModel,
        hookSeq,
        hookBatch,
        hookMode,
        parallelDp,
        parallelTp,
        parallelPp,
        parallelPpMicrobatch,
        modelNumLayers,
    ],
    () => {
        updateHookStatus();
    },
    { immediate: true }
);

watch(predictionSeq, (next) => {
    if (useBackend.value) return;
    const seq = Number(next);
    if (!Number.isFinite(seq)) return;
    const entries = filteredPredictions.value.filter((item) => item.seq === seq);
    if (!entries.length) return;
    const batch = Number(predictionBatch.value);
    if (!Number.isFinite(batch) || !entries.some((item) => item.batch === batch)) {
        predictionBatch.value = String(entries[0].batch);
    }
});


function buildHosts() {
    const count = Math.max(1, hostCountValue.value || 1);
    const gpu = gpuModel.value.trim();
    const hosts = [];
    for (let i = 0; i < count; i += 1) {
        const entry = { id: i, name: `rank${i}`, topo_index: i };
        if (gpu) entry.gpu = { model: gpu };
        hosts.push(entry);
    }
    return hosts;
}

function syncRanksWithHostCount() {
    const count = Math.max(1, hostCountValue.value || 1);
    const byId = new Map(ranks.value.map((rank) => [rank.id, rank]));
    const next = [];
    for (let i = 0; i < count; i += 1) {
        const existing = byId.get(i);
        if (existing) {
            next.push(existing);
        } else {
            next.push({ id: i, steps: [] });
        }
    }
    ranks.value = next;
    if (!next.length) {
        selectedRankId.value = "";
        return;
    }
    if (!next.some((rank) => String(rank.id) === selectedRankId.value)) {
        selectedRankId.value = String(next[0].id);
    }
}

const COMM_OPS = new Set([
    "ALLREDUCE",
    "ALLREDUCE_ASYNC",
    "ALLGATHER",
    "REDUCESCATTER",
    "ALLTOALL",
    "ALLTOALL_EP",
    "ALLGATHER_DP_EP",
    "REDUCESCATTER_DP_EP",
    "SENDRECV",
]);
const BYTES_PER_ELEMENT = 4;

function parseCsv(text) {
    const rows = [];
    let field = "";
    let row = [];
    let inQuotes = false;
    for (let i = 0; i < text.length; i += 1) {
        const ch = text[i];
        const next = text[i + 1];
        if (ch === '"' && inQuotes && next === '"') {
            field += '"';
            i += 1;
            continue;
        }
        if (ch === '"') {
            inQuotes = !inQuotes;
            continue;
        }
        if (ch === "," && !inQuotes) {
            row.push(field);
            field = "";
            continue;
        }
        if (ch === "\n" && !inQuotes) {
            row.push(field);
            field = "";
            if (row.length > 1 || row[0]) rows.push(row);
            row = [];
            continue;
        }
        if (ch === "\r") continue;
        field += ch;
    }
    if (field.length || row.length) {
        row.push(field);
        rows.push(row);
    }
    const header = rows.shift() || [];
    return rows.map((r) => {
        const out = {};
        header.forEach((h, idx) => {
            out[h] = r[idx] ?? "";
        });
        return out;
    });
}

function parseOpsLiteral(raw) {
    if (!raw) return [];
    const text = String(raw).trim();
    if (!text || text === "[]") return [];
    try {
        let normalized = text.replace(/None/g, "null");
        normalized = normalized.replace(/\(/g, "[").replace(/\)/g, "]");
        normalized = normalized.replace(/'/g, '"');
        normalized = normalized.replace(/,\s*]/g, "]");
        return JSON.parse(normalized);
    } catch (err) {
        return [];
    }
}

function formatNumber(value, digits = 6) {
    if (!Number.isFinite(value)) return "0";
    const fixed = value.toFixed(digits);
    return fixed.replace(/\.?0+$/, "");
}

function normalizePredictionRows(raw) {
    const rows = parseCsv(raw);
    return rows.map((row) => ({
        Name: row.Name,
        OpName: row.OpName,
        CommGroup: row.CommGroup || "",
        FwOps: parseOpsLiteral(row.FwOps),
        BwOps: parseOpsLiteral(row.BwOps),
        AccOps: parseOpsLiteral(row.AccOps),
        InputShapes: parseOpsLiteral(row.InputShapes),
        OutputShape: parseOpsLiteral(row.OutputShape),
        fw_latency: parseNumber(row.fw_latency, 0),
        bw_latency: parseNumber(row.bw_latency, 0),
        acc_latency: parseNumber(row.acc_latency, 0),
    }));
}

function splitLayers(rows, modelName, numLayers) {
    if (!numLayers || numLayers <= 1) {
        return { prologue: [], layers: [rows], epilogue: [] };
    }
    const name = String(modelName || "").toLowerCase();
    if (!name || name.includes("switch")) {
        return { prologue: [], layers: [rows], epilogue: [] };
    }
    const findIndex = (target) => rows.findIndex((row) => row.Name === target);
    let start = -1;
    let end = -1;
    if (name.includes("bert")) {
        start = findIndex("bert_encoder_layer_0_attention_self_query");
        end = findIndex("bert_encoder_layer_0_output_layer_norm");
    } else if (name.includes("gpt")) {
        start = findIndex("transformer_h_0_ln_1_grad");
        if (start < 0) start = findIndex("transformer_h_0_ln_1");
        end = findIndex("add_15");
    } else if (name.includes("opt")) {
        start = findIndex("model_decoder_layers_0_self_attn_layer_norm");
        end = findIndex("view_11");
    } else {
        return { prologue: [], layers: [rows], epilogue: [] };
    }
    if (start < 0 || end < 0) {
        return { prologue: [], layers: [rows], epilogue: [] };
    }
    end += 1;
    const prologue = rows.slice(0, start);
    const layer = rows.slice(start, end);
    const epilogue = rows.slice(end);
    const layers = Array.from({ length: numLayers }, () => layer);
    return { prologue, layers, epilogue };
}

function commBytesFromOps(ops, bytesPerElement) {
    let total = 0;
    if (!Array.isArray(ops)) return total;
    for (const op of ops) {
        if (!Array.isArray(op) || op.length < 2) continue;
        const name = op[0];
        if (!COMM_OPS.has(name)) continue;
        const args = op[1];
        if (Array.isArray(args) && args.length) {
            const size = Number(args[0]);
            if (Number.isFinite(size) && size > 0) {
                total += Math.trunc(size) * bytesPerElement;
            }
        }
    }
    return total;
}

function inferCommGroup(row) {
    const name = String(row.Name || "").toLowerCase();
    const opname = String(row.OpName || "").toLowerCase();
    if (name.includes("sendrecv") || opname === "sendrecv") return "pp";
    if (name.endsWith("_grad") && opname === "allreduce") return "dp";
    if (
        name.includes("tensor_model_parallel") ||
        name.includes("reduce_from_tensor_model_parallel_region")
    ) {
        return "tp";
    }
    return "";
}

function collectLayerStats(rows, bytesPerElement) {
    let fwComputeMs = 0;
    let bwComputeMs = 0;
    let tpFwBytes = 0;
    let tpBwBytes = 0;
    let dpBwBytes = 0;
    let ppBytes = 0;
    for (const row of rows) {
        const fwComm = commBytesFromOps(row.FwOps, bytesPerElement);
        const bwComm = commBytesFromOps(row.BwOps, bytesPerElement);
        const commGroupRaw = row.CommGroup ? String(row.CommGroup).toLowerCase() : "";
        const commGroup = commGroupRaw || inferCommGroup(row);

        if (commGroup) {
            if (commGroup === "tp") {
                tpFwBytes += fwComm;
                tpBwBytes += bwComm;
            } else if (commGroup === "dp") {
                dpBwBytes += fwComm + bwComm;
            } else if (commGroup === "pp") {
                ppBytes = Math.max(ppBytes, fwComm, bwComm);
            }
            continue;
        }

        fwComputeMs += row.fw_latency || 0;
        bwComputeMs += (row.bw_latency || 0) + (row.acc_latency || 0);

        if (fwComm || bwComm) {
            const inferred = inferCommGroup(row);
            if (inferred === "tp") {
                tpFwBytes += fwComm;
                tpBwBytes += bwComm;
            } else if (inferred === "dp") {
                dpBwBytes += fwComm + bwComm;
            }
        }
    }

    if (ppBytes <= 0 && rows.length) {
        const shape = rows[rows.length - 1].OutputShape;
        if (Array.isArray(shape) && shape.length) {
            let elems = 1;
            for (const dim of shape) {
                const size = parseNumber(dim, NaN);
                if (!Number.isFinite(size) || size <= 0) {
                    elems = 0;
                    break;
                }
                elems *= Math.trunc(size);
            }
            if (elems > 0) {
                ppBytes = elems * bytesPerElement;
            }
        }
    }

    return {
        fw_compute_ms: fwComputeMs,
        bw_compute_ms: bwComputeMs,
        tp_fw_bytes: tpFwBytes,
        tp_bw_bytes: tpBwBytes,
        dp_bw_bytes: dpBwBytes,
        pp_bytes: ppBytes,
    };
}

function rankFor(dpIdx, ppIdx, tpIdx, dpDegree, ppDegree, tpDegree) {
    return (dpIdx * ppDegree + ppIdx) * tpDegree + tpIdx;
}

function buildRankMap(dpDegree, ppDegree, tpDegree) {
    const ranksOut = [];
    for (let dpIdx = 0; dpIdx < dpDegree; dpIdx += 1) {
        for (let ppIdx = 0; ppIdx < ppDegree; ppIdx += 1) {
            for (let tpIdx = 0; tpIdx < tpDegree; tpIdx += 1) {
                ranksOut.push({
                    id: rankFor(dpIdx, ppIdx, tpIdx, dpDegree, ppDegree, tpDegree),
                    dp: dpIdx,
                    pp: ppIdx,
                    tp: tpIdx,
                });
            }
        }
    }
    return ranksOut;
}

function stepPayloadToState(payload, index) {
    return {
        key: createUuid(),
        id: String(payload.id ?? index),
        label: payload.label || "",
        kind: payload.kind || "compute",
        compute_ms: payload.compute_ms != null ? formatNumber(payload.compute_ms) : "0",
        comm_bytes: payload.comm_bytes != null ? String(payload.comm_bytes) : "0",
        comm_id: payload.comm_id || "",
        op: payload.op || "",
        hosts: Array.isArray(payload.hosts) ? payload.hosts.join(",") : "",
        peer: payload.peer != null ? String(payload.peer) : "",
        direction: payload.direction || "",
    };
}

function buildRankSteps(rankInfo, stageStats, dpDegree, ppDegree, tpDegree, microbatches, pipeline) {
    const dpIdx = rankInfo.dp;
    const ppIdx = rankInfo.pp;
    const tpIdx = rankInfo.tp;
    const tpGroup = [];
    for (let t = 0; t < tpDegree; t += 1) {
        tpGroup.push(rankFor(dpIdx, ppIdx, t, dpDegree, ppDegree, tpDegree));
    }
    const dpGroup = [];
    for (let d = 0; d < dpDegree; d += 1) {
        dpGroup.push(rankFor(d, ppIdx, tpIdx, dpDegree, ppDegree, tpDegree));
    }

    let prevRank = null;
    let nextRank = null;
    if (ppIdx > 0) {
        prevRank = rankFor(dpIdx, ppIdx - 1, tpIdx, dpDegree, ppDegree, tpDegree);
    }
    if (ppIdx + 1 < ppDegree) {
        nextRank = rankFor(dpIdx, ppIdx + 1, tpIdx, dpDegree, ppDegree, tpDegree);
    }

    const stats = stageStats[ppIdx];
    const steps = [];

    const ppCommId = (direction, srcStage, microbatch) =>
        `pp-${direction}-s${srcStage}-mb${microbatch}-dp${dpIdx}-tp${tpIdx}`;
    const tpCommId = (direction, microbatch) =>
        `tp-${direction}-pp${ppIdx}-dp${dpIdx}-mb${microbatch}`;
    const dpCommId = (direction, microbatch) =>
        `dp-${direction}-pp${ppIdx}-tp${tpIdx}-mb${microbatch}`;

    const addCompute = (label, ms) => {
        if (ms <= 0) return;
        steps.push({ kind: "compute", label, compute_ms: ms });
    };
    const addCollective = (label, op, commBytes, hosts, commId) => {
        if (commBytes <= 0) return;
        steps.push({
            kind: "collective",
            label,
            op,
            comm_bytes: Math.trunc(commBytes),
            hosts,
            comm_id: commId,
        });
    };
    const addSendrecv = (label, commBytes, peer, direction, commId) => {
        if (commBytes <= 0 || peer == null) return;
        steps.push({
            kind: "sendrecv",
            label,
            comm_bytes: Math.trunc(commBytes),
            peer,
            direction,
            comm_id: commId,
        });
    };

    const forwardStep = (microbatch) => {
        if (prevRank != null) {
            addSendrecv(
                `fwd_recv_mb${microbatch}`,
                stats.pp_bytes,
                prevRank,
                "recv",
                ppCommId("fwd", ppIdx - 1, microbatch)
            );
        }
        addCompute(`fwd_mb${microbatch}`, stats.fw_compute_ms);
        addCollective(
            `tp_fwd_mb${microbatch}`,
            "allreduce",
            stats.tp_fw_bytes,
            tpGroup,
            tpCommId("fwd", microbatch)
        );
        if (nextRank != null) {
            addSendrecv(
                `fwd_send_mb${microbatch}`,
                stats.pp_bytes,
                nextRank,
                "send",
                ppCommId("fwd", ppIdx, microbatch)
            );
        }
    };

    const backwardStep = (microbatch) => {
        if (nextRank != null) {
            addSendrecv(
                `bwd_recv_mb${microbatch}`,
                stats.pp_bytes,
                nextRank,
                "recv",
                ppCommId("bwd", ppIdx + 1, microbatch)
            );
        }
        addCompute(`bwd_mb${microbatch}`, stats.bw_compute_ms);
        addCollective(
            `tp_bwd_mb${microbatch}`,
            "allreduce",
            stats.tp_bw_bytes,
            tpGroup,
            tpCommId("bwd", microbatch)
        );
        addCollective(
            `dp_bwd_mb${microbatch}`,
            "allreduce",
            stats.dp_bw_bytes,
            dpGroup,
            dpCommId("bwd", microbatch)
        );
        if (prevRank != null) {
            addSendrecv(
                `bwd_send_mb${microbatch}`,
                stats.pp_bytes,
                prevRank,
                "send",
                ppCommId("bwd", ppIdx, microbatch)
            );
        }
    };

    if (pipeline === "fwd_bwd") {
        for (let mb = 0; mb < microbatches; mb += 1) {
            forwardStep(mb);
        }
        for (let mb = 0; mb < microbatches; mb += 1) {
            backwardStep(mb);
        }
    } else {
        const numWarmup = Math.min(microbatches, ppDegree - ppIdx - 1);
        const numRemaining = microbatches - numWarmup;
        let fwdIdx = 0;
        let bwdIdx = 0;
        for (let i = 0; i < numWarmup; i += 1) {
            forwardStep(fwdIdx);
            fwdIdx += 1;
        }
        for (let i = 0; i < numRemaining; i += 1) {
            forwardStep(fwdIdx);
            fwdIdx += 1;
            backwardStep(bwdIdx);
            bwdIdx += 1;
        }
        while (bwdIdx < microbatches) {
            backwardStep(bwdIdx);
            bwdIdx += 1;
        }
    }

    return steps.map((step, index) => ({ ...step, id: index }));
}

function buildRanksFromRows(rows) {
    const dp = dpDegree.value || 1;
    const tp = tpDegree.value || 1;
    const pp = ppDegree.value || 1;
    const microbatches = Math.max(1, ppMicrobatch.value || 1);
    const layers = parseNumber(modelNumLayers.value, 0);
    const model = modelName.value.trim();
    if (pp > 1 && layers <= 0) {
        throw new Error("pp requires num_layers");
    }
    const layerCount = layers > 0 ? Math.floor(layers) : 1;
    const { prologue, layers: layerBlocks, epilogue } = splitLayers(rows, model, layerCount);
    const layerStats = layerBlocks.map((layer) => collectLayerStats(layer, BYTES_PER_ELEMENT));
    if (layerStats.length % pp !== 0) {
        throw new Error("num_layers must be divisible by pp");
    }
    const prologueStats = prologue.length ? collectLayerStats(prologue, BYTES_PER_ELEMENT) : null;
    const epilogueStats = epilogue.length ? collectLayerStats(epilogue, BYTES_PER_ELEMENT) : null;
    const perStage = layerStats.length / pp;
    const stageStats = [];
    for (let stage = 0; stage < pp; stage += 1) {
        const start = stage * perStage;
        const end = start + perStage;
        const chunk = layerStats.slice(start, end);
        const stageStat = {
            fw_compute_ms: chunk.reduce((sum, item) => sum + item.fw_compute_ms, 0),
            bw_compute_ms: chunk.reduce((sum, item) => sum + item.bw_compute_ms, 0),
            tp_fw_bytes: chunk.reduce((sum, item) => sum + item.tp_fw_bytes, 0),
            tp_bw_bytes: chunk.reduce((sum, item) => sum + item.tp_bw_bytes, 0),
            dp_bw_bytes: chunk.reduce((sum, item) => sum + item.dp_bw_bytes, 0),
            pp_bytes: chunk.length ? chunk[chunk.length - 1].pp_bytes : 0,
        };
        if (stage === 0 && prologueStats) {
            stageStat.fw_compute_ms += prologueStats.fw_compute_ms;
            stageStat.bw_compute_ms += prologueStats.bw_compute_ms;
            stageStat.tp_fw_bytes += prologueStats.tp_fw_bytes;
            stageStat.tp_bw_bytes += prologueStats.tp_bw_bytes;
            stageStat.dp_bw_bytes += prologueStats.dp_bw_bytes;
        }
        if (stage === pp - 1 && epilogueStats) {
            stageStat.fw_compute_ms += epilogueStats.fw_compute_ms;
            stageStat.bw_compute_ms += epilogueStats.bw_compute_ms;
            stageStat.tp_fw_bytes += epilogueStats.tp_fw_bytes;
            stageStat.tp_bw_bytes += epilogueStats.tp_bw_bytes;
            stageStat.dp_bw_bytes += epilogueStats.dp_bw_bytes;
        }
        stageStats.push(stageStat);
    }

    return buildRankMap(dp, pp, tp).map((rankInfo) => {
        const stepPayloads = buildRankSteps(
            rankInfo,
            stageStats,
            dp,
            pp,
            tp,
            microbatches,
            pipelineSchedule.value
        );
        return {
            id: rankInfo.id,
            steps: stepPayloads.map(stepPayloadToState),
        };
    });
}

function buildRanksFromCsv(raw) {
    const rows = normalizePredictionRows(raw);
    return buildRanksFromRows(rows);
}

function buildMeta() {
    const meta = {};
    const model = modelName.value.trim();
    const layers = parseNumber(modelNumLayers.value, NaN);
    const device = gpuModel.value.trim();
    const source = metaSource.value.trim();
    const dp = dpDegree.value || 1;
    const tp = tpDegree.value || 1;
    const pp = ppDegree.value || 1;
    const micro = Math.max(1, ppMicrobatch.value || 1);
    if (source) meta.source = source;
    if (model) meta.model = model;
    if (Number.isFinite(layers) && layers > 0) meta.num_layers = layers;
    if (device) meta.device = device;
    meta.parallel = {
        dp,
        tp,
        pp,
        pp_microbatch: micro,
        layout: "dp-pp-tp",
        pipeline: pipelineSchedule.value,
    };
    return Object.keys(meta).length ? meta : null;
}

const jsonText = computed(() => {
    const hosts = buildHosts();
    const ranksOut = ranks.value.map((rank, rankIndex) => {
        const rankId = parseNumber(rank.id, rankIndex);
        const stepsOut = (rank.steps || []).map((step, index) => {
            const id = parseNumber(step.id, index);
            const kind = step.kind || "compute";
            const payload = {
                id,
                kind,
            };
            if (step.label) payload.label = step.label;
            if (kind === "compute") {
                payload.compute_ms = parseNumber(step.compute_ms, 0);
                return payload;
            }
            payload.comm_bytes = parseNumber(step.comm_bytes, 0);
            if (step.comm_id) payload.comm_id = step.comm_id;
            if (kind === "collective") {
                if (step.op) payload.op = step.op;
                const hostsList = String(step.hosts || "")
                    .split(",")
                    .map((h) => parseNumber(h, NaN))
                    .filter((h) => Number.isFinite(h));
                if (hostsList.length) payload.hosts = hostsList;
            } else if (kind === "sendrecv") {
                const peer = parseNumber(step.peer, NaN);
                if (Number.isFinite(peer)) payload.peer = peer;
                if (step.direction) payload.direction = step.direction;
            }
            return payload;
        });
        return { id: rankId, steps: stepsOut };
    });

    const topologyOut =
        topology.kind === "dumbbell"
            ? {
                  kind: "dumbbell",
                  host_link_gbps: parseNumber(topology.host_link_gbps, 100),
                  bottleneck_gbps: parseNumber(topology.bottleneck_gbps, 10),
                  link_latency_us: parseNumber(topology.link_latency_us, 2),
              }
            : {
                  kind: "fat_tree",
                  k: parseNumber(topology.k, 4),
                  link_gbps: parseNumber(topology.link_gbps, 100),
                  link_latency_us: parseNumber(topology.link_latency_us, 2),
              };

    const data = {
        schema_version: 2,
        topology: topologyOut,
        hosts,
        ranks: ranksOut,
    };
    const metaOut = buildMeta();
    if (metaOut) data.meta = metaOut;
    if (defaults.protocol) {
        data.defaults = { protocol: defaults.protocol, bytes_per_element: BYTES_PER_ELEMENT };
    }
    return JSON.stringify(data, null, 2);
});

function addStep() {
    if (!selectedRank.value) return;
    selectedRank.value.steps.push({
        key: createUuid(),
        id: String(selectedRank.value.steps.length),
        label: "",
        kind: "compute",
        compute_ms: "0",
        comm_bytes: "0",
        comm_id: "",
        op: "",
        hosts: "",
        peer: "",
        direction: "",
    });
}

function removeStep(index) {
    if (!selectedRank.value) return;
    selectedRank.value.steps.splice(index, 1);
}

function reindexSteps() {
    if (!selectedRank.value) return;
    selectedRank.value.steps = selectedRank.value.steps.map((step, idx) => ({
        ...step,
        id: String(idx),
    }));
}

function loadSample() {
    topology.kind = "dumbbell";
    topology.host_link_gbps = "100";
    topology.bottleneck_gbps = "10";
    topology.link_latency_us = "2";
    defaults.protocol = "tcp";
    gpuModel.value = "NVIDIA_A100";
    modelName.value = "";
    modelNumLayers.value = "";
    modelType.value = "";
    metaSource.value = "";
    parallelDp.value = "2";
    parallelTp.value = "1";
    parallelPp.value = "1";
    parallelPpMicrobatch.value = "1";
    pipelineSchedule.value = "1f1b";
    const sampleHosts = "0,1";
    const sampleSteps = () => [
        {
            key: createUuid(),
            id: "0",
            label: "step0",
            kind: "compute",
            compute_ms: "2.5",
            comm_bytes: "0",
            comm_id: "",
            op: "",
            hosts: "",
            peer: "",
            direction: "",
        },
        {
            key: createUuid(),
            id: "1",
            label: "step0_comm",
            kind: "collective",
            compute_ms: "0",
            comm_bytes: "1048576",
            comm_id: "allreduce-0",
            op: "allreduce",
            hosts: sampleHosts,
            peer: "",
            direction: "",
        },
        {
            key: createUuid(),
            id: "2",
            label: "step1",
            kind: "compute",
            compute_ms: "1.5",
            comm_bytes: "0",
            comm_id: "",
            op: "",
            hosts: "",
            peer: "",
            direction: "",
        },
        {
            key: createUuid(),
            id: "3",
            label: "step1_comm",
            kind: "collective",
            compute_ms: "0",
            comm_bytes: "2097152",
            comm_id: "allreduce-1",
            op: "allreduce",
            hosts: sampleHosts,
            peer: "",
            direction: "",
        },
    ];
    ranks.value = [
        { id: 0, steps: sampleSteps() },
        { id: 1, steps: sampleSteps() },
    ];
    selectedRankId.value = "0";
    localWorkloadStatus.value = "已载入样例。";
}

function inferRankStepKind(step) {
    if (step?.kind) return step.kind;
    if (step?.peer != null || step?.direction) return "sendrecv";
    if (step?.op || Array.isArray(step?.hosts)) return "collective";
    return "compute";
}

function normalizeRankStep(step, index) {
    const kind = inferRankStepKind(step);
    return {
        key: createUuid(),
        id: String(step?.id ?? index),
        label: step?.label || "",
        kind,
        compute_ms: step?.compute_ms != null ? String(step.compute_ms) : "0",
        comm_bytes: step?.comm_bytes != null ? String(step.comm_bytes) : "0",
        comm_id: step?.comm_id || "",
        op: step?.op || "",
        hosts: Array.isArray(step?.hosts) ? step.hosts.join(",") : "",
        peer: step?.peer != null ? String(step.peer) : "",
        direction: step?.direction || "",
    };
}

function normalizeRanksFromData(rawRanks) {
    return rawRanks
        .map((rank, idx) => ({
            id: parseNumber(rank?.id, idx),
            steps: Array.isArray(rank?.steps) ? rank.steps.map(normalizeRankStep) : [],
        }))
        .sort((a, b) => a.id - b.id);
}

function convertV1StepsToRanks(rawSteps, hostTotal) {
    const count = Math.max(1, hostTotal || 1);
    const allHosts = Array.from({ length: count }, (_, idx) => idx);
    const steps = Array.isArray(rawSteps) ? rawSteps : [];
    const ranksOut = [];
    for (let rankId = 0; rankId < count; rankId += 1) {
        const rankSteps = [];
        for (let i = 0; i < steps.length; i += 1) {
            const step = steps[i] || {};
            const label = step.label || `step${step.id ?? i}`;
            const computeMs = parseNumber(step.compute_ms, 0);
            if (computeMs > 0) {
                rankSteps.push({
                    key: createUuid(),
                    id: "",
                    label,
                    kind: "compute",
                    compute_ms: formatNumber(computeMs),
                    comm_bytes: "0",
                    comm_id: "",
                    op: "",
                    hosts: "",
                    peer: "",
                    direction: "",
                });
            }
            const commBytes = parseNumber(step.comm_bytes, 0);
            if (commBytes > 0) {
                const hostsList = Array.isArray(step.hosts) && step.hosts.length ? step.hosts : allHosts;
                if (hostsList.includes(rankId)) {
                    let opName = "allreduce";
                    if (Array.isArray(step.comm_ops) && step.comm_ops.length && step.comm_ops[0]?.op) {
                        opName = String(step.comm_ops[0].op);
                    }
                    rankSteps.push({
                        key: createUuid(),
                        id: "",
                        label: `${label}_comm`,
                        kind: "collective",
                        compute_ms: "0",
                        comm_bytes: String(commBytes),
                        comm_id: `v1-${step.id ?? i}`,
                        op: opName,
                        hosts: hostsList.join(","),
                        peer: "",
                        direction: "",
                    });
                }
            }
        }
        ranksOut.push({
            id: rankId,
            steps: rankSteps.map((entry, idx) => ({ ...entry, id: String(idx) })),
        });
    }
    return ranksOut;
}

function applyWorkloadData(data) {
    try {
        metaSource.value = "";
        modelName.value = "";
        modelNumLayers.value = "";
        modelType.value = "";
        ranks.value = [];
        if (data.topology?.kind) topology.kind = data.topology.kind;
        if (data.topology?.host_link_gbps != null) topology.host_link_gbps = String(data.topology.host_link_gbps);
        if (data.topology?.bottleneck_gbps != null) topology.bottleneck_gbps = String(data.topology.bottleneck_gbps);
        if (data.topology?.link_latency_us != null) topology.link_latency_us = String(data.topology.link_latency_us);
        if (data.topology?.k != null) topology.k = String(data.topology.k);
        if (data.topology?.link_gbps != null) topology.link_gbps = String(data.topology.link_gbps);
        defaults.protocol = data.defaults?.protocol || "tcp";
        if (data.meta?.source != null) metaSource.value = String(data.meta.source || "");
        if (data.meta?.model != null) modelName.value = String(data.meta.model || "");
        if (data.meta?.num_layers != null) modelNumLayers.value = String(data.meta.num_layers || "");
        if (Array.isArray(data.hosts)) {
            const gpu = data.hosts[0]?.gpu?.model || "";
            if (gpu) gpuModel.value = gpu;
        }
        if (!gpuModel.value && data.meta?.device) {
            gpuModel.value = String(data.meta.device);
        }
        const parallel = data.meta?.parallel || {};
        if (parallel.dp != null || parallel.tp != null || parallel.pp != null) {
            const dpValue = parsePositiveInt(parallel.dp, 1) || 1;
            const tpValue = parsePositiveInt(parallel.tp, 1) || 1;
            const ppValue = parsePositiveInt(parallel.pp, 1) || 1;
            const microValue = parsePositiveInt(parallel.pp_microbatch, 1) || 1;
            parallelDp.value = String(dpValue);
            parallelTp.value = String(tpValue);
            parallelPp.value = String(ppValue);
            parallelPpMicrobatch.value = String(microValue);
            if (parallel.pipeline) pipelineSchedule.value = String(parallel.pipeline);
        } else if (Array.isArray(data.hosts)) {
            parallelDp.value = String(data.hosts.length || 1);
            parallelTp.value = "1";
            parallelPp.value = "1";
            parallelPpMicrobatch.value = "1";
        }
        if (Array.isArray(data.ranks) && data.ranks.length) {
            ranks.value = normalizeRanksFromData(data.ranks);
        } else if (Array.isArray(data.steps)) {
            ranks.value = convertV1StepsToRanks(data.steps, data.hosts?.length || 1);
        }
        syncRanksWithHostCount();
        return true;
    } catch (err) {
        return false;
    }
}

function loadLocalWorkload() {
    const entry = localWorkloads.find((item) => item.key === localWorkloadKey.value);
    if (!entry) {
        localWorkloadStatus.value = "未找到选择的 workload.json。";
        return;
    }
    try {
        const ok = applyWorkloadData(JSON.parse(entry.raw));
        localWorkloadStatus.value = ok ? `已载入 ${entry.label}。` : `解析失败：${entry.label}。`;
    } catch (err) {
        localWorkloadStatus.value = `解析失败：${entry.label}。`;
    }
}

async function runWorkloadSim() {
    if (simRunning.value) return;
    const localEntry = localWorkloads.find((item) => item.key === localWorkloadKey.value);
    let workload = null;
    if (!localEntry) {
        try {
            workload = JSON.parse(jsonText.value);
        } catch (err) {
            simStatus.value = "当前 JSON 解析失败，无法运行。";
            return;
        }
    }
    const workloadName = simWorkloadName.value.trim() || "workload.json";
    const outputName = simOutputName.value.trim() || "out.json";
    const until = parseNumber(simUntilMs.value, NaN);
    const payload = {
        output_name: outputName,
        fct_stats: simFctStats.value,
    };
    if (localEntry) {
        payload.workload_path = `viz/workloads/${localEntry.label}`;
    } else {
        payload.workload = workload;
        payload.workload_name = workloadName;
    }
    if (Number.isFinite(until)) {
        payload.until_ms = Math.max(0, Math.floor(until));
    }
    const workloadPath = localEntry
        ? `viz/workloads/${localEntry.label}`
        : `viz/workloads/${workloadName}`;
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

async function buildFromHook() {
    if (!canBuildHook.value) {
        hookStatus.value = "请选择有效的模型/GPU/seq/batch。";
        return;
    }
    const model = modelName.value.trim();
    const gpu = gpuModel.value.trim();
    const seq = parseNumber(hookSeq.value, NaN);
    const batch = parseNumber(hookBatch.value, NaN);
    const dp = dpDegree.value || 1;
    const tp = tpDegree.value || 1;
    const pp = ppDegree.value || 1;
    const ppMicro = ppMicrobatch.value || 1;
    const warmupSteps = Math.max(0, Math.floor(parseNumber(hookWarmup.value, 0)));
    const measureSteps = Math.max(1, Math.floor(parseNumber(hookSteps.value, 1)));
    const tpCommFactor = parseNumber(hookTpCommFactor.value, 2);
    const payload = {
        model,
        gpu,
        mode: hookMode.value,
        seq,
        batch,
        dp,
        tp,
        pp,
        pp_microbatch: ppMicro,
        pipeline: pipelineSchedule.value,
        dtype: hookDtype.value,
        device: hookDevice.value,
        warmup_steps: warmupSteps,
        measure_steps: measureSteps,
        tp_comm_factor: tpCommFactor,
        device_scale_mode: "none",
        model_backend: "transformers",
        topology: buildTopologyPayload(),
        defaults: {
            protocol: defaults.protocol,
            routing: "per_flow",
        },
    };
    hookRequestSummary.value = formatHookSummary(payload);
    hookResponseSummary.value = "";
    hookStatus.value = "测量中，请稍候...";
    try {
        const postJson = async (url) => {
            const resp = await fetch(url, {
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
            return { resp, data };
        };
        let endpoint = hookProxyApi;
        let resp = null;
        let data = null;
        try {
            ({ resp, data } = await postJson(endpoint));
            if ([404, 502, 503].includes(resp.status)) {
                throw new Error(`hook proxy unavailable (status ${resp.status})`);
            }
        } catch (proxyErr) {
            endpoint = hookDirectApi.value;
            ({ resp, data } = await postJson(endpoint));
        }
        let data = null;
        try {
            data = await resp.json();
        } catch (parseErr) {
            data = null;
        }
        const responseParts = [`status=${resp.status}`, `ok=${Boolean(data?.ok)}`, `endpoint=${endpoint}`];
        if (Number.isFinite(data?.elapsed_ms)) responseParts.push(`elapsed_ms=${data.elapsed_ms}`);
        if (data?.path) responseParts.push(`path=${data.path}`);
        if (data?.error) responseParts.push(`error=${data.error}`);
        hookResponseSummary.value = responseParts.join(", ");
        if (!resp.ok || !data?.ok) {
            throw new Error(data?.error || `backend status ${resp.status}`);
        }
        if (!data?.workload) {
            throw new Error("missing workload");
        }
        const ok = applyWorkloadData(data.workload);
        hookStatus.value = ok ? `已生成 ${ranks.value.length} 个 rank。` : "生成失败：workload 解析错误。";
    } catch (err) {
        const message = err?.message || "unknown error";
        hookStatus.value = `生成失败：${message}`;
    }
}

async function buildFromPrediction() {
    if (!canBuildPrediction.value) {
        predictionStatus.value = "请选择有效的模型/GPU/预测器/seq/batch。";
        return;
    }
    const model = modelName.value.trim();
    const gpu = predictionGpuResolved.value;
    const predictor = predictionPredictor.value;
    const mode = predictionMode.value;
    const seq = parseNumber(predictionSeq.value, NaN);
    const batch = parseNumber(predictionBatch.value, NaN);
    const options = predictionOptions.value;
    const entry = selectedPrediction.value;
    const payload = { model, gpu, predictor, mode, seq, batch };
    if (options) payload.options = options;
    predictionRequestSummary.value = formatPredictionSummary(payload);
    predictionResponseSummary.value = "";

    const useLocalCsv = async () => {
        if (!entry) {
            predictionStatus.value = "未找到本地 CSV。";
            return false;
        }
        const loader = predictionLoaders[entry.path];
        if (!loader) {
            predictionStatus.value = "未找到对应 CSV。";
            return false;
        }
        const raw = await loader();
        ranks.value = buildRanksFromCsv(raw);
        selectedRankId.value = ranks.value.length ? String(ranks.value[0].id) : "";
        metaSource.value = `prediction/${entry.gpu}/${entry.predictor}/${entry.file}`;
        predictionStatus.value = `已生成 ${ranks.value.length} 个 rank。`;
        predictionResponseSummary.value = `source=local_csv, path=${entry.path}`;
        return true;
    };

    if (useBackend.value) {
        predictionStatus.value = "GPU 预测中，请稍候...";
        try {
            const resp = await fetch(predictApi, {
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
            const responseParts = [`status=${resp.status}`, `ok=${Boolean(data?.ok)}`];
            if (Number.isFinite(data?.elapsed_ms)) responseParts.push(`elapsed_ms=${data.elapsed_ms}`);
            if (data?.path) responseParts.push(`path=${data.path}`);
            if (data?.csv) responseParts.push(`csv_bytes=${data.csv.length}`);
            if (data?.error) responseParts.push(`error=${data.error}`);
            predictionResponseSummary.value = responseParts.join(", ");
            if (!resp.ok) {
                throw new Error(data?.error || `backend status ${resp.status}`);
            }
            if (!data?.ok || !data.csv) {
                throw new Error(data?.error || "backend error");
            }
            try {
                ranks.value = buildRanksFromCsv(data.csv);
            } catch (buildErr) {
                const message = buildErr?.message || "CSV 解析错误";
                predictionStatus.value = `生成失败：${message}`;
                return;
            }
            selectedRankId.value = ranks.value.length ? String(ranks.value[0].id) : "";
            metaSource.value = data.path || `prediction/${gpu}/${predictor}/${model}-${mode}-${seq}-${batch}.csv`;
            predictionStatus.value = `已生成 ${ranks.value.length} 个 rank。`;
            return;
        } catch (err) {
            if (entry) {
                predictionStatus.value = "GPU 预测失败，回退到本地 CSV。";
                try {
                    await useLocalCsv();
                    return;
                } catch (fallbackErr) {
                    predictionStatus.value = "回退失败：本地 CSV 解析错误。";
                    return;
                }
            }
            const message = err?.message || "unknown error";
            predictionStatus.value = `GPU 预测失败：${message}`;
            return;
        }
    }
    try {
        await useLocalCsv();
    } catch (err) {
        const message = err?.message || "CSV 解析错误";
        predictionStatus.value = `生成失败：${message}`;
    }
}

async function copyJson() {
    try {
        await navigator.clipboard.writeText(jsonText.value);
        copyStatus.value = "已复制 JSON。";
    } catch (err) {
        const el = jsonArea.value;
        if (el && typeof el.select === "function") {
            el.focus();
            el.select();
            try {
                const ok = document.execCommand && document.execCommand("copy");
                copyStatus.value = ok ? "已复制 JSON。" : "复制失败，请手动选中复制。";
                return;
            } catch (fallbackErr) {
                copyStatus.value = "复制失败，请手动选中复制。";
                return;
            }
        }
        copyStatus.value = "复制失败，请手动选中复制。";
    }
}

function downloadJson() {
    const blob = new Blob([jsonText.value], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "workload.json";
    a.click();
    URL.revokeObjectURL(url);
}
</script>
