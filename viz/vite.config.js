import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");

function sanitizeFileName(raw, fallback) {
    const base = String(raw || fallback || "").trim() || "out.json";
    const cleaned = base.replace(/[^A-Za-z0-9._-]/g, "_");
    return cleaned.endsWith(".json") ? cleaned : `${cleaned}.json`;
}

function isWithin(parent, target) {
    const parentPath = path.resolve(parent) + path.sep;
    const targetPath = path.resolve(target) + path.sep;
    return targetPath.startsWith(parentPath);
}

function readBody(req) {
    return new Promise((resolve, reject) => {
        let data = "";
        req.on("data", (chunk) => {
            data += chunk;
        });
        req.on("end", () => resolve(data));
        req.on("error", reject);
    });
}

function simApiPlugin() {
    const workloadDir = path.join(repoRoot, "viz", "workloads");
    const outputDir = path.join(repoRoot, "viz", "outputs");
    return {
        name: "workload-sim-api",
        configureServer(server) {
            server.middlewares.use("/api-sim/run", async (req, res) => {
                if (req.method !== "POST") {
                    res.setHeader("Content-Type", "application/json");
                    res.statusCode = 405;
                    res.end(JSON.stringify({ ok: false, error: "method not allowed" }));
                    return;
                }
                try {
                    const body = await readBody(req);
                    const payload = JSON.parse(body || "{}");
                    await fs.mkdir(workloadDir, { recursive: true });
                    await fs.mkdir(outputDir, { recursive: true });

                    let workloadPath = null;
                    if (payload.workload) {
                        const workloadName = sanitizeFileName(payload.workload_name, "workload.json");
                        workloadPath = path.join(workloadDir, workloadName);
                        await fs.writeFile(workloadPath, JSON.stringify(payload.workload, null, 2));
                    } else if (payload.workload_path) {
                        const resolved = path.resolve(repoRoot, payload.workload_path);
                        if (!isWithin(repoRoot, resolved)) {
                            res.setHeader("Content-Type", "application/json");
                            res.statusCode = 400;
                            res.end(JSON.stringify({ ok: false, error: "workload path out of repo" }));
                            return;
                        }
                        workloadPath = resolved;
                    } else if (payload.workload_name) {
                        const workloadName = sanitizeFileName(payload.workload_name, "workload.json");
                        workloadPath = path.join(workloadDir, workloadName);
                    }

                    if (!workloadPath) {
                        res.setHeader("Content-Type", "application/json");
                        res.statusCode = 400;
                        res.end(JSON.stringify({ ok: false, error: "missing workload" }));
                        return;
                    }
                    try {
                        await fs.stat(workloadPath);
                    } catch (err) {
                        res.setHeader("Content-Type", "application/json");
                        res.statusCode = 400;
                        res.end(JSON.stringify({ ok: false, error: "workload not found" }));
                        return;
                    }

                    const outputName = sanitizeFileName(payload.output_name, "out.json");
                    const outputPath = path.join(outputDir, outputName);
                    const args = [
                        "run",
                        "--bin",
                        "workload_sim",
                        "--",
                        "--workload",
                        workloadPath,
                        "--viz-json",
                        outputPath,
                    ];
                    if (payload.fct_stats !== false) {
                        args.push("--fct-stats");
                    }
                    if (payload.until_ms != null) {
                        args.push("--until-ms", String(payload.until_ms));
                    }

                    const child = spawn("cargo", args, {
                        cwd: repoRoot,
                        env: process.env,
                    });
                    let stdout = "";
                    let stderr = "";
                    child.stdout.on("data", (chunk) => {
                        stdout += chunk.toString();
                    });
                    child.stderr.on("data", (chunk) => {
                        stderr += chunk.toString();
                    });
                    child.on("close", (code) => {
                        res.setHeader("Content-Type", "application/json");
                        res.statusCode = code === 0 ? 200 : 500;
                        res.end(
                            JSON.stringify({
                                ok: code === 0,
                                code,
                                workload_path: workloadPath,
                                output_path: outputPath,
                                stdout,
                                stderr,
                            })
                        );
                    });
                } catch (err) {
                    res.setHeader("Content-Type", "application/json");
                    res.statusCode = 500;
                    res.end(JSON.stringify({ ok: false, error: err?.message || "server error" }));
                }
            });

            server.middlewares.use("/api-sim/run-multi", async (req, res) => {
                if (req.method !== "POST") {
                    res.setHeader("Content-Type", "application/json");
                    res.statusCode = 405;
                    res.end(JSON.stringify({ ok: false, error: "method not allowed" }));
                    return;
                }
                try {
                    const body = await readBody(req);
                    const payload = JSON.parse(body || "{}");
                    await fs.mkdir(workloadDir, { recursive: true });
                    await fs.mkdir(outputDir, { recursive: true });

                    const rawPaths = Array.isArray(payload.workload_paths) ? payload.workload_paths : [];
                    if (!rawPaths.length) {
                        res.setHeader("Content-Type", "application/json");
                        res.statusCode = 400;
                        res.end(JSON.stringify({ ok: false, error: "missing workload_paths" }));
                        return;
                    }

                    const workloadPaths = [];
                    for (const raw of rawPaths) {
                        const resolved = path.resolve(repoRoot, String(raw || ""));
                        if (!isWithin(repoRoot, resolved)) {
                            res.setHeader("Content-Type", "application/json");
                            res.statusCode = 400;
                            res.end(JSON.stringify({ ok: false, error: "workload path out of repo" }));
                            return;
                        }
                        try {
                            await fs.stat(resolved);
                        } catch (err) {
                            res.setHeader("Content-Type", "application/json");
                            res.statusCode = 400;
                            res.end(JSON.stringify({ ok: false, error: `workload not found: ${raw}` }));
                            return;
                        }
                        workloadPaths.push(resolved);
                    }

                    const outputName = sanitizeFileName(payload.output_name, "out.json");
                    const outputPath = path.join(outputDir, outputName);
                    const args = ["run", "--bin", "workloads_sim", "--"];
                    for (const workloadPath of workloadPaths) {
                        args.push("--workload", workloadPath);
                    }
                    args.push("--viz-json", outputPath);
                    if (payload.fct_stats !== false) {
                        args.push("--fct-stats");
                    }
                    if (payload.until_ms != null) {
                        args.push("--until-ms", String(payload.until_ms));
                    }

                    const child = spawn("cargo", args, {
                        cwd: repoRoot,
                        env: process.env,
                    });
                    let stdout = "";
                    let stderr = "";
                    child.stdout.on("data", (chunk) => {
                        stdout += chunk.toString();
                    });
                    child.stderr.on("data", (chunk) => {
                        stderr += chunk.toString();
                    });
                    child.on("close", (code) => {
                        res.setHeader("Content-Type", "application/json");
                        res.statusCode = code === 0 ? 200 : 500;
                        res.end(
                            JSON.stringify({
                                ok: code === 0,
                                code,
                                workload_paths: workloadPaths,
                                output_path: outputPath,
                                stdout,
                                stderr,
                            })
                        );
                    });
                } catch (err) {
                    res.setHeader("Content-Type", "application/json");
                    res.statusCode = 500;
                    res.end(JSON.stringify({ ok: false, error: err?.message || "server error" }));
                }
            });
        },
    };
}

export default defineConfig({
    plugins: [vue(), simApiPlugin()],
    base: "./",
    server: {
        host: "0.0.0.0",
        watch: {
            ignored: ["**/viz/workloads/**", "**/viz/outputs/**"],
        },
        fs: {
            allow: [repoRoot],
        },
        proxy: {
            "/api": "http://127.0.0.1:3099",
            "/api-workload": {
                target: "http://127.0.0.1:3100",
                rewrite: (path) => path.replace(/^\/api-workload/, "/api"),
            },
        },
    },
});
