<template>
    <div class="editor-shell">
        <header class="editor-hero">
            <div>
                <h1>Workload Editor</h1>
                <p class="editor-subtitle">用图形化方式生成 `workload.json`，支持导入 NeuSight 预测 CSV。</p>
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
                        <h2>导入</h2>
                        <span class="chip">workload.json</span>
                    </div>
                    <input type="file" accept=".json,application/json" @change="onImportJson" />
                    <div class="small">导入后会覆盖当前配置。</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>NeuSight CSV</h2>
                        <span class="chip">prediction</span>
                    </div>
                    <div class="grid">
                        <label>预测 CSV</label>
                        <input type="file" accept=".csv,text/csv" @change="onPredCsv" />
                        <label>摘要 JSON（可选，用于 num_layer）</label>
                        <input type="file" accept=".json,application/json" @change="onSummaryJson" />
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>num_layer</label>
                            <input v-model="csvForm.numLayers" type="text" placeholder="自动或手动填写" />
                        </div>
                        <div class="grid">
                            <label>bytes/elem</label>
                            <input v-model="csvForm.bytesPerElement" type="text" />
                        </div>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>hosts</label>
                            <input v-model="csvForm.hostCount" type="text" />
                        </div>
                        <div class="grid">
                            <label>GPU 型号</label>
                            <input v-model="csvForm.gpuModel" type="text" placeholder="例如 NVIDIA_H100" />
                        </div>
                    </div>
                    <button type="button" @click="buildFromCsv" :disabled="!csvForm.predText">从 CSV 生成 steps</button>
                    <div class="small">{{ csvStatus }}</div>
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
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>Hosts</h2>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>host 数量</label>
                            <input v-model="hostCount" type="text" />
                        </div>
                        <div class="grid">
                            <label>GPU 型号</label>
                            <input v-model="gpuModel" type="text" placeholder="例如 NVIDIA_A100" />
                        </div>
                    </div>
                    <div class="small">默认使用 topo_index = id，生成 rank0..rankN。</div>
                </section>

                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>默认参数</h2>
                    </div>
                    <div class="row">
                        <div class="grid">
                            <label>routing</label>
                            <select v-model="defaults.routing">
                                <option value="per_flow">per_flow</option>
                                <option value="per_packet">per_packet</option>
                            </select>
                        </div>
                        <div class="grid">
                            <label>bytes/elem</label>
                            <input v-model="defaults.bytes_per_element" type="text" />
                        </div>
                    </div>
                </section>
            </div>

            <div class="editor-column">
                <section class="editor-card">
                    <div class="editor-card-header">
                        <h2>Steps</h2>
                        <div class="editor-card-actions">
                            <button class="secondary" type="button" @click="addStep">新增</button>
                            <button class="secondary" type="button" @click="reindexSteps">重排 id</button>
                        </div>
                    </div>
                    <div class="editor-table">
                        <div class="editor-table-row editor-table-head">
                            <div>ID</div>
                            <div>Label</div>
                            <div>Compute (ms)</div>
                            <div>Comm (bytes)</div>
                            <div>Hosts</div>
                            <div>Protocol</div>
                            <div></div>
                        </div>
                        <div
                            v-for="(step, i) in steps"
                            :key="step.key"
                            class="editor-table-row"
                        >
                            <input v-model="step.id" type="text" />
                            <input v-model="step.label" type="text" placeholder="optional" />
                            <input v-model="step.compute_ms" type="text" />
                            <input v-model="step.comm_bytes" type="text" />
                            <input v-model="step.hosts" type="text" placeholder="0,1,2" />
                            <select v-model="step.protocol">
                                <option value="">默认</option>
                                <option value="tcp">tcp</option>
                                <option value="dctcp">dctcp</option>
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
                    <textarea class="editor-json" :value="jsonText" readonly></textarea>
                </section>
            </div>
        </div>
    </div>
</template>

<script setup>
import { computed, reactive, ref } from "vue";

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

const topology = reactive({
    kind: "dumbbell",
    host_link_gbps: "100",
    bottleneck_gbps: "10",
    link_latency_us: "2",
    k: "4",
    link_gbps: "100",
});

const defaults = reactive({
    protocol: "tcp",
    routing: "per_flow",
    bytes_per_element: "4",
});

const hostCount = ref("2");
const gpuModel = ref("");

const steps = ref([
    {
        key: crypto.randomUUID(),
        id: "0",
        label: "step0",
        compute_ms: "2.0",
        comm_bytes: "1048576",
        hosts: "",
        protocol: "",
    },
]);

const csvForm = reactive({
    predText: "",
    summaryText: "",
    numLayers: "",
    bytesPerElement: "4",
    hostCount: "2",
    gpuModel: "",
    fileName: "",
});

const csvStatus = ref("等待 CSV 导入。");

function parseNumber(raw, fallback = 0) {
    if (raw == null) return fallback;
    const clean = String(raw).replace(/_/g, "").trim();
    if (!clean) return fallback;
    const value = Number(clean);
    return Number.isFinite(value) ? value : fallback;
}

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

function extractCommElems(ops) {
    let total = 0;
    for (const op of ops) {
        if (!Array.isArray(op) || op.length < 2) continue;
        const name = op[0];
        if (!COMM_OPS.has(name)) continue;
        const args = op[1];
        if (Array.isArray(args) && args.length) {
            const size = Number(args[0]);
            if (Number.isFinite(size) && size > 0) total += size;
        }
    }
    return total;
}

function findModelName(fileName) {
    const match = String(fileName).match(/^([a-zA-Z0-9_]+)-/);
    return match ? match[1] : String(fileName).split(".")[0] || "model";
}

function replicateLayers(rows, modelName, numLayers) {
    if (!numLayers || numLayers <= 1) return rows;
    const name = modelName.toLowerCase();
    if (name.includes("switch")) return rows;
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
        return rows;
    }
    if (start < 0 || end < 0) return rows;
    end += 1;
    const prologue = rows.slice(0, start);
    const layer = rows.slice(start, end);
    const epilogue = rows.slice(end);
    const repeated = [];
    for (let i = 0; i < numLayers; i += 1) repeated.push(...layer);
    return [...prologue, ...repeated, ...epilogue];
}

function rowsToSteps(rows, bytesPerElement) {
    const out = [];
    let computeMs = 0;
    for (const row of rows) {
        const fwOps = parseOpsLiteral(row.FwOps);
        const bwOps = parseOpsLiteral(row.BwOps);
        const commElems = extractCommElems(fwOps) + extractCommElems(bwOps);
        if (commElems > 0) {
            const commBytes = commElems * bytesPerElement;
            out.push({
                key: crypto.randomUUID(),
                id: String(out.length),
                label: row.Name || "",
                compute_ms: computeMs.toFixed(6),
                comm_bytes: String(commBytes),
                hosts: "",
                protocol: "",
            });
            computeMs = 0;
        } else {
            const fw = parseNumber(row.fw_latency);
            const bw = parseNumber(row.bw_latency);
            const acc = parseNumber(row.acc_latency);
            computeMs += fw + bw + acc;
        }
    }
    if (computeMs > 0) {
        out.push({
            key: crypto.randomUUID(),
            id: String(out.length),
            label: "compute_tail",
            compute_ms: computeMs.toFixed(6),
            comm_bytes: "0",
            hosts: "",
            protocol: "",
        });
    }
    return out;
}

function buildHosts() {
    const count = Math.max(1, parseNumber(hostCount.value, 1));
    const gpu = gpuModel.value.trim();
    const hosts = [];
    for (let i = 0; i < count; i += 1) {
        const entry = { id: i, name: `rank${i}`, topo_index: i };
        if (gpu) entry.gpu = { model: gpu };
        hosts.push(entry);
    }
    return hosts;
}

const jsonText = computed(() => {
    const hosts = buildHosts();
    const stepsOut = steps.value.map((step, index) => {
        const id = parseNumber(step.id, index);
        const payload = {
            id,
            label: step.label || undefined,
            compute_ms: parseNumber(step.compute_ms, 0),
            comm_bytes: parseNumber(step.comm_bytes, 0),
        };
        if (step.protocol) payload.protocol = step.protocol;
        const hostsList = String(step.hosts || "")
            .split(",")
            .map((h) => parseNumber(h, NaN))
            .filter((h) => Number.isFinite(h));
        if (hostsList.length) payload.hosts = hostsList;
        return payload;
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
        schema_version: 1,
        topology: topologyOut,
        defaults: {
            protocol: defaults.protocol,
            routing: defaults.routing,
            bytes_per_element: parseNumber(defaults.bytes_per_element, 4),
        },
        hosts,
        steps: stepsOut,
    };
    return JSON.stringify(data, null, 2);
});

function addStep() {
    steps.value.push({
        key: crypto.randomUUID(),
        id: String(steps.value.length),
        label: "",
        compute_ms: "0",
        comm_bytes: "0",
        hosts: "",
        protocol: "",
    });
}

function removeStep(index) {
    steps.value.splice(index, 1);
}

function reindexSteps() {
    steps.value = steps.value.map((step, idx) => ({ ...step, id: String(idx) }));
}

function loadSample() {
    topology.kind = "dumbbell";
    topology.host_link_gbps = "100";
    topology.bottleneck_gbps = "10";
    topology.link_latency_us = "2";
    defaults.protocol = "tcp";
    defaults.routing = "per_flow";
    defaults.bytes_per_element = "4";
    hostCount.value = "2";
    gpuModel.value = "NVIDIA_A100";
    steps.value = [
        {
            key: crypto.randomUUID(),
            id: "0",
            label: "step0",
            compute_ms: "2.5",
            comm_bytes: "1048576",
            hosts: "",
            protocol: "",
        },
        {
            key: crypto.randomUUID(),
            id: "1",
            label: "step1",
            compute_ms: "1.5",
            comm_bytes: "2097152",
            hosts: "",
            protocol: "",
        },
    ];
}

async function onImportJson(ev) {
    const file = ev.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    try {
        const data = JSON.parse(text);
        if (data.topology?.kind) topology.kind = data.topology.kind;
        if (data.topology?.host_link_gbps != null) topology.host_link_gbps = String(data.topology.host_link_gbps);
        if (data.topology?.bottleneck_gbps != null) topology.bottleneck_gbps = String(data.topology.bottleneck_gbps);
        if (data.topology?.link_latency_us != null) topology.link_latency_us = String(data.topology.link_latency_us);
        if (data.topology?.k != null) topology.k = String(data.topology.k);
        if (data.topology?.link_gbps != null) topology.link_gbps = String(data.topology.link_gbps);
        if (data.defaults?.protocol) defaults.protocol = data.defaults.protocol;
        if (data.defaults?.routing) defaults.routing = data.defaults.routing;
        if (data.defaults?.bytes_per_element != null) defaults.bytes_per_element = String(data.defaults.bytes_per_element);
        if (Array.isArray(data.hosts)) {
            hostCount.value = String(data.hosts.length || 1);
            const gpu = data.hosts[0]?.gpu?.model || "";
            gpuModel.value = gpu;
        }
        if (Array.isArray(data.steps)) {
            steps.value = data.steps.map((step, idx) => ({
                key: crypto.randomUUID(),
                id: String(step.id ?? idx),
                label: step.label || "",
                compute_ms: String(step.compute_ms ?? 0),
                comm_bytes: String(step.comm_bytes ?? 0),
                hosts: Array.isArray(step.hosts) ? step.hosts.join(",") : "",
                protocol: step.protocol || "",
            }));
        }
        csvStatus.value = "导入 workload.json 完成。";
    } catch (err) {
        csvStatus.value = "导入失败：JSON 格式错误。";
    }
    ev.target.value = "";
}

async function onPredCsv(ev) {
    const file = ev.target.files?.[0];
    if (!file) return;
    csvForm.predText = await file.text();
    csvForm.fileName = file.name || "";
    csvStatus.value = `已载入 CSV：${csvForm.fileName}`;
    ev.target.value = "";
}

async function onSummaryJson(ev) {
    const file = ev.target.files?.[0];
    if (!file) return;
    csvForm.summaryText = await file.text();
    try {
        const data = JSON.parse(csvForm.summaryText);
        if (data.num_layer != null) csvForm.numLayers = String(data.num_layer);
    } catch (err) {
        csvStatus.value = "摘要 JSON 解析失败。";
    }
    ev.target.value = "";
}

function buildFromCsv() {
    if (!csvForm.predText) return;
    const rows = parseCsv(csvForm.predText);
    const modelName = findModelName(csvForm.fileName);
    const numLayers = parseNumber(csvForm.numLayers, 0);
    const expanded = replicateLayers(rows, modelName, numLayers);
    const bytesPerElement = parseNumber(csvForm.bytesPerElement, 4);
    steps.value = rowsToSteps(expanded, bytesPerElement);
    defaults.bytes_per_element = String(bytesPerElement);
    hostCount.value = String(parseNumber(csvForm.hostCount, 2));
    gpuModel.value = csvForm.gpuModel;
    csvStatus.value = `生成 ${steps.value.length} 条 step。`;
}

async function copyJson() {
    try {
        await navigator.clipboard.writeText(jsonText.value);
        csvStatus.value = "已复制 JSON。";
    } catch (err) {
        csvStatus.value = "复制失败，请手动选中复制。";
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
