# AI 训练模拟（workload.json）+ 可视化重构计划

## 目标
- 从 `Neusight` 产出的 `workload.json` 驱动事件级仿真，生成 `output.json` 供现有可视化前端播放。
- 提供一个可视化界面用于编辑/生成 `workload.json`。
- 前端可视化架构拆分，fat-tree 优化不影响 dumbbell。

## 范围与约束
- 后端：新增一个 `src/bin/` 的解析与仿真入口，尽量复用现有网络/协议栈。
- 前端：保留 dumbbell 现有表现，fat-tree 单独演进。
- 先保证数据流闭环（workload.json -> output.json -> viz）。

## Phase 1: 规格与数据流打通
1) 定义 `workload.json` 规范（最小可行）
   - 顶层：`topology`、`hosts`、`steps`（或 `timeline`）、全局参数。
   - 每个 step：`type`（compute/comm）、`host(s)`、`bytes`、`duration_ms`、可选 `flow_id`/`tags`。
   - 输出：`output.json` 采用现有 viz 事件结构（`Meta` + `tx_start`/`enqueue`/`drop` 等）。
2) 解析器与配置模型
   - 新建 `src/proto/workload.rs` 或 `src/sim/workload.rs`，用 `serde` 定义结构体。
   - 校验：拓扑类型/host 数/参数范围。
3) 新增仿真入口
   - 新二进制：`src/bin/workload_sim.rs`（示例名）。
   - 读取 `workload.json` -> 构建拓扑 -> 生成事件 -> 输出 `output.json`。

## Phase 2: 事件级仿真实现
1) 拓扑映射
   - 支持 `dumbbell` 与 `fat-tree`（复用 `src/topo/`）。
2) 计算与通信建模
   - compute：在 host 上安排“本地耗时事件”（无链路传输，仅推进时间）。
   - comm：按 workload 描述创建 flow（TCP/DCTCP/裸包），使用现有 `NetApi`。
3) 事件调度策略
   - 串行/并行：支持 step 内并行（例如多个 host 同时执行）。
   - 统一生成 flow_id，保证可视化可跟踪。

## Phase 3: 可视化编辑器（workload.json 生成）
1) 前端新增页面/模式 （top bar进行区分，不要与现在的网络可视化界面放到一个page）
   - 选择拓扑类型与规模。
   - 以表格/时间线编辑 compute/comm step。
2) 导出/导入
   - 导出为 `workload.json`，支持导入已有文件编辑。
   - 提供最小模板与样例（自动填充）。

## Phase 4: 前端可视化重构（fat-tree 与 dumbbell 解耦）
1) 架构拆分
   - `viz/src/layouts/`：`dumbbellLayout.js`、`fatTreeLayout.js`、`circleLayout.js`
   - `viz/src/renderers/`：`dumbbellRenderer.js`、`fatTreeRenderer.js`
   - 公共模块：`viz/src/renderers/common/`（links/nodes/labels/palette）
2) 路由与配置隔离
   - layout 选择逻辑集中到一处，renderer 内部只处理自身特有规则。
   - fat-tree 专用配置与样式不影响 dumbbell。
3) fat-tree 优化迭代
   - 先重排布局（减少重叠/交叉），再优化连线样式与标签可见性。

## Phase 5: 验证与样例
1) 生成样例 workload
   - 小规模 dumbbell 与 fat-tree 各 1 个。
2) 回放检查
   - `workload_sim` 输出能被 `viz/` 正确播放。
   - dumbbell 视觉不回退；fat-tree 可读性提升。

## 交付物
- `workload.json` 规范与样例文件
- 新二进制：`workload_sim`
- 前端 workload 编辑器入口
- 可视化重构后的模块结构

## 风险与依赖
- workload 与 viz 输出格式对齐（需要确定事件 schema 的最小集合）。
- fat-tree 布局改动需隔离，避免牵连 dumbbell。
