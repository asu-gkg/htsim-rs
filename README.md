# htsim-rs

Rust 网络仿真框架（事件驱动内核 + 基础网络组件 + 示例）。更详细说明见 `tutorial.md`。

## 目录结构
- `src/sim/`: 事件队列仿真内核（Simulator/Event/World）
- `src/net/`: 基础网络对象与转发逻辑（Packet/Node/Link/Network）
- `src/proto/`: 协议原型（TCP）
- `src/queue/`: 队列模型（DropTail 等）
- `src/topo/`: 可复用拓扑构建（dumbbell/fat-tree 等）
- `src/bin/`: 可执行示例
- `tools/viz/`: 可视化回放页面

## 运行
Dumbbell 拓扑：
```bash
cargo run --bin dumbbell -- --pkts 10000 --until-ms 50
cargo run --bin dumbbell_tcp -- --handshake --app-limited-pps 2000  --viz-json out.json
cargo run --bin dumbbell -- --help
```

单包追踪：
```bash
cargo run --bin trace_single_packet
cargo run --bin trace_single_packet -- --help
```

TCP + 可视化：
```bash
cargo run --bin dumbbell_tcp -- --viz-json out.json
```

DCTCP + ECN：
```bash
cargo run --bin dumbbell_dctcp -- --ecn-k-pkts 20
```

可视化（含 DCTCP 丢包场景）：
```bash
# 中等队列 + 低阈值 ECN + 适度初始窗口，能看到 ECN 与丢包共存
cargo run --bin dumbbell_dctcp -- --viz-json out.json --queue-pkts 12 --ecn-k-pkts 4 --init-cwnd-pkts 20 --bottleneck-gbps 5
```

长时间运行 + cwnd/alpha 采样：
```bash
# 大数据量 + 较长仿真时间，输出 cwnd.csv 便于画图

cargo run --bin dumbbell_dctcp -- --quiet \
--viz-json out.json  \
--data-bytes 500000000 \
--until-ms 2000 \
--link-latency-us 20 \
--bottleneck-gbps 1 \
--queue-pkts 12 --ecn-k-pkts 4

```

Fat-tree demo:
```bash
cargo run --bin fat_tree
```

## 日志与解析
```bash
RUST_LOG=debug cargo run --bin trace_single_packet
RUST_LOG=trace cargo run --bin trace_single_packet 2>&1 | python3 parse_logs.py


cargo run --bin fat_tree_allreduce_dctcp -- --quiet  --viz-json out.json --queue-pkts 32 --ecn-k-pkts 8

cargo run --bin fat_tree_allreduce_dctcp -- --quiet  --viz-json out.json --k 4 --ranks 16 --msg-bytes 40000 --chunk-bytes 10000  --queue-pkts 32 --ecn-k-pkts 8
```


```

python3 tools/plot_allreduce_tcp.py --routing per-flow

```

## 下一步（可选）
- 协议层：完善 TCP/NDP/Swift 等
- 队列模型：引入 ECN/PFC/多队列
- 统计输出：导出 CSV/JSON
