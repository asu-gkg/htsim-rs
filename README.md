# htsim-rs

Rust 网络仿真框架：事件驱动内核、基础网络对象、TCP/DCTCP 原型与可视化回放。

## 架构概览
- `src/bin/`: 场景入口与 CLI 参数解析
- `src/topo/`: 拓扑构建与实验流量配置
- `src/cc/`: 集体通信算法（如 allreduce）
- `src/proto/`: 传输协议（TCP/DCTCP）状态机与定时器
- `src/net/`: 网络对象与转发逻辑（Packet/Node/Link/Network）
- `src/queue/`: 队列模型（DropTail 等）
- `src/sim/`: 事件队列与仿真时钟
- `src/viz/`: 观测与可视化事件

依赖方向：上层场景（bin/topo/cc）向下依赖协议/网络/仿真核心；可视化通过 hooks 旁路接入。

## 快速开始
构建：
```bash
cargo build
```

### 场景 1：基础场景（观察完整的 cwnd 演变）
```bash
cargo run --bin dumbbell_tcp -- \
  --data-bytes 1000000000 \
  --queue-pkts 16 \
  --bottleneck-gbps 1 \
  --link-latency-us 2000 \
  --init-cwnd-pkts 1 \
  --init-ssthresh-pkts 1000 \
  --until-ms 1000 \
  --viz-json out.json 
```

## 可视化：

```
cd viz
npm install
npm run dev
```
- 打开 `http://localhost:5173/`，加载 `out.json`

其他入口：`dumbbell` / `dumbbell_tcp` / `dumbbell_dctcp` / `fat_tree` / `trace_single_packet`（使用 `--help` 查看参数）。
