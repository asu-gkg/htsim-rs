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

## 7. 下一步
告诉我你要优先对齐的模块（协议/队列/统计/拓扑），我可以按最小可验证方式补齐并给出示例。
