# htsim-rs（框架）

这是一个用 Rust 重写/复刻本仓库网络模拟器的**最小可运行框架**：事件驱动内核 + 基础网络组件 + 示例。

更详细的上手说明见 `tutorial.md`。

## 目录结构

- `src/sim/`: 事件队列仿真内核（`Simulator` / `SimTime` / `Event`）
- `src/net/`: 基础网络对象（`Packet` / `Node` / `Link` / `Network`）
- `src/demo.rs`: 共享的拓扑构建函数和类型
- `src/bin/`: 独立的可执行文件
  - `dumbbell.rs`: Dumbbell 拓扑仿真
  - `trace_single_packet.rs`: 单包追踪模式

## 运行

项目包含多个独立的可执行文件，每个都有自己的 `main` 函数：

### Dumbbell 拓扑仿真

```bash
cd htsim-rs
cargo run --bin dumbbell -- --pkts 10000 --until-ms 50
```

查看所有参数：
```bash
cargo run --bin dumbbell -- --help
```

### 单包追踪模式

```bash
cd htsim-rs
cargo run --bin trace_single_packet
```

查看所有参数：
```bash
cargo run --bin trace_single_packet -- --help
```

使用日志解析器美化输出：
```bash
RUST_LOG=trace cargo run --bin trace_single_packet 2>&1 | python3 parse_logs.py
```

你会看到类似输出：

```
done @ SimTime(...), delivered_pkts=..., delivered_bytes=...
```

## 下一步建议（你后续要加的东西）

- **协议层**：把 TCP/NDP/Swift/HPCC 等做成独立模块（例如 `proto::tcp`），在 `Node::on_packet` 或专门的 endpoint 中处理 ACK/重传/拥塞控制等。
- **路由/拓扑**：把 `Packet.route` 从"显式路径"升级为"按目的地查表/ECMP/喷洒"等。
- **队列/交换机**：把 `Link` 的 `busy_until` 原型升级为可插拔队列（ECN、PFC、优先级、多队列等）。
- **统计与日志**：在 `World` 中集中统计（FCT、吞吐、排队时延、丢包等），并导出为 CSV/JSON。

## Tracing 日志使用指南

项目已集成 `tracing` 日志库，可以自动记录函数调用、文件位置、行号等信息。

### 基本使用

```bash
# 默认 INFO 级别
cargo run --bin trace_single_packet

# DEBUG 级别（推荐调试时使用）
RUST_LOG=debug cargo run --bin trace_single_packet

# TRACE 级别（最详细）
RUST_LOG=trace cargo run --bin trace_single_packet
```

### 只查看特定模块的日志

```bash
# 只看网络模块
RUST_LOG=htsim_rs::net=debug cargo run --bin trace_single_packet

# 只看仿真器模块
RUST_LOG=htsim_rs::sim=debug cargo run --bin trace_single_packet
```

## 日志解析器

使用 `parse_logs.py` 美化日志输出：

```bash
# 基本使用
RUST_LOG=trace cargo run --bin trace_single_packet 2>&1 | python3 parse_logs.py

# 保存日志到文件再解析
RUST_LOG=debug cargo run --bin trace_single_packet 2>&1 > logs.txt
cat logs.txt | python3 parse_logs.py
```

详细说明见代码注释和 `tutorial.md`。
