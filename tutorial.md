# htsim-rs 教程（精简版）

这份文档只保留最核心的运行与改动入口，帮助你快速定位“数据包从哪里进、在哪里转发、什么时候结束”。

## 1. 模块分层
- `sim`: 事件驱动仿真内核（`Simulator`/`Event`/`World`）
- `net`: 网络模型与转发（`Network`/`Node`/`Link`/`Packet`）
- `proto`: 协议原型（目前有简化 TCP）
- `topo`: 可复用拓扑构建（`dumbbell`/`fat_tree` 等）
- `bin`: 可执行入口（`dumbbell`/`dumbbell_tcp`/`fat_tree`/`trace_single_packet`）

## 2. 一个数据包的流转（以 dumbbell 为例）
1. `InjectFlow` 或 `TcpStart` 在 `t=0` 被调度。
2. `Network::forward_from` 选择下一跳（预设或 ECMP），计算排队与传播时间，并调度 `DeliverPacket`。
3. `DeliverPacket::execute` 调用 `Network::deliver`，再交给 `Node::on_packet` 处理。
4. 未到达目的地则继续转发；到达后由 `Network::on_delivered` 统计。

## 3. 运行
```bash
# dumbbell 拓扑
cargo run --bin dumbbell -- --pkts 10000 --until-ms 50

# 单包追踪
cargo run --bin trace_single_packet

# TCP dumbbell + 可视化事件
cargo run --bin dumbbell_tcp -- --viz-json out.json



# DCTCP dumbbell
cargo run --bin dumbbell_dctcp -- --ecn-k-pkts 20  --viz-json out.json

# fat-tree demo
cargo run --bin fat_tree

# 查看参数
cargo run --bin dumbbell -- --help
```

## 4. 调试与日志
- 日志级别：`RUST_LOG=debug` 或 `RUST_LOG=trace`
- 回溯：`RUST_BACKTRACE=1 cargo run --bin dumbbell`
- 建议打印位置：`InjectFlow::execute`、`Network::forward_from`、`DeliverPacket::execute`、`Node::on_packet`
- 断点（gdb/lldb）：
```bash
cargo build
rust-gdb target/debug/dumbbell --args target/debug/dumbbell --pkts 1000 --until-ms 50
```

## 5. 如何修改
- 改拓扑/路由：`src/topo/dumbbell.rs` 或 `src/topo/fat_tree.rs`
- 改链路与队列：`src/net/` + `src/queue/`
- 改流量注入：`src/bin/dumbbell.rs` 或 `src/bin/fat_tree.rs`
- 加协议与统计：`src/proto/` 与 `src/net/stats.rs`

## 6. 常见问题
- `no link from A to B`：检查拓扑是否双向 `connect`，以及路由是否覆盖最后一跳。
- delivered 很少：增大 `--until-ms` 或放大 `--gap-us`。

## 8. 下一步
告诉我你要优先对齐的模块（协议/队列/统计/拓扑），我可以按最小可验证方式补齐并给出示例。














## 7. TCP 拥塞控制学习参数

如果想学习 TCP 拥塞控制，推荐以下几个参数场景：

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

**参数说明**：
- `--init-cwnd-pkts 1`：从 1 个包开始，清晰看到**慢启动的指数增长**
- `--init-ssthresh-pkts 1000`：较大的初始阈值，慢启动阶段更长
- `--queue-pkts 16`：队列容量 16 包，超过后触发丢包
- `--link-latency-us 2000`：2ms 延迟，RTT 约 8ms
- `--bottleneck-gbps 1`：1Gbps 瓶颈，配合延迟形成适中的 BDP
- `--until-ms 1000`：运行 1 秒，观察多个 AIMD 周期

### 场景 3：三次握手 + 完整流程

```bash
cargo run --bin dumbbell_tcp -- \
  --data-bytes 500000 \
  --queue-pkts 1 \
  --bottleneck-gbps 1 \
  --link-latency-us 5000 \
  --init-cwnd-pkts 1 \
  --handshake \
  --viz-json out.json
```

**特点**：`--handshake` 启用三次握手，能看到连接建立过程

### 可视化观察重点

在 `viz/index.html` 中加载生成的 `out.json`，重点关注：

- **cwnd 图**：慢启动（指数增长）→ 拥塞避免（线性增长）→ 丢包（减半）
- **ssthresh 图**：每次丢包时下降到 cwnd/2
- **inflight 图**：实际在途数据，受 cwnd 和应用数据限制
- **三者对比**：cwnd 是上限，inflight 是实际值