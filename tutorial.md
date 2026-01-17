# htsim-rs 教程（从“能跑”到“会改/会调试”）

这份文档的目标是：让你能快速理解 `htsim-rs` 目前这套**事件驱动仿真框架**的结构，知道一个包是怎么“被注入→排队→传播→到达→再转发”的，以及你应该在哪些地方下断点、打印日志、修改参数。

> 当前阶段是“框架原型”：实现了最小的事件队列、链路的串行化发送（`busy_until`），以及 **预设路由 + 动态路由（ECMP/FIB）可混用** 的转发骨架与 `dumbbell` 示例。后续你会在这个骨架上逐步加协议、队列模型、统计等。

---

## 1. 先建立心智模型：三件事

你可以把代码分成三层：

- **仿真内核层（`sim`）**：只关心“什么时候执行什么事件”
  - `Simulator`: 持有当前时间 `now` 和事件优先队列
  - `Event`: 事件接口（`execute`）
  - `World`: 世界/业务状态（这里用 `NetWorld` 承载网络）

- **网络模型层（`net`）**：只关心“包在网络里怎么走”
  - `Network`: 节点、链路、转发表（这里是 `(from,to)->link`）
  - `Node`: 节点行为（`Host`/`Switch` 当前都是“若未到达 dst 就转发”；下一跳可来自 packet 预设前缀/路径或动态路由表 ECMP）
  - `Link`: 链路模型（传播时延 `latency`、带宽 `bandwidth_bps`、忙碌时间 `busy_until`）

- **场景/流量层（`demo` + `main`）**：搭拓扑、注入流量、启动仿真
  - `build_dumbbell`: 搭 dumbbell 拓扑
  - `InjectFlow`: 周期性注入 packet 的事件
  - `main.rs`: CLI 解析参数并调用 `run_dumbbell`

---

## 2. 最关键：一个 Packet 的“调用链”

以 dumbbell 为例，路径是：`h0 -> s0 -> s1 -> h1`。

### 2.1 注入阶段（`demo.rs`）

1. `run_dumbbell()` 会调度一个 `InjectFlow` 事件在 \(t=0\) 执行。
2. `InjectFlow::execute()` 每次执行会：
   - `make_packet(...)` 创建一个 `Packet`
   - 调用 `w.net.forward_from(src, pkt, sim)` 把包“从 src 发出”
   - 如果还有剩余包，则再 schedule 下一个 `InjectFlow`（间隔 `gap`）

### 2.2 发送/排队/传播（`net.rs`）

`Network::forward_from(from, pkt, sim)` 做了三件事：

1. **找到下一跳与链路**：
   - 若 packet 仍有预设下一跳：用 `pkt.preset_next()`
   - 否则：用动态路由表（按最短跳数）给出 (from,dst) 的 ECMP 候选，并按 `flow_id` 做 hashing 选择
   - 然后用 `edges[(from,to)]` 找到 `Link`
2. **算发送开始时间**：
   - `now = sim.now()`
   - `start = max(now, link.busy_until)`（链路忙就排队）
3. **算 depart/arrive 并调度到达事件**：
   - `tx_time = pkt.size_bytes * 8 / bandwidth_bps`
   - `depart = start + tx_time`
   - `arrive = depart + latency`
   - `sim.schedule(arrive, DeliverPacket { to, pkt: pkt.advance() })`

### 2.3 到达并交给节点处理（`DeliverPacket` + `Node`）

1. `Simulator::run_until()` 会不断弹出“最早时间”的事件执行。
2. 当 `DeliverPacket` 被执行：
   - 它从 `world` downcast 到 `NetWorld`
   - 调用 `w.net.deliver(to, pkt, sim)`
3. `Network::deliver()` 把目标节点取出来并调用 `node.on_packet(pkt, sim, self)`：
   - 如果 `node.id != pkt.dst`：继续 `net.forward_from(...)`（再 schedule 下一跳到达事件）
   - 否则：调用 `net.on_delivered(pkt)` 更新统计

---

## 3. 如何运行（Run）

### 3.1 基本运行

在仓库根目录：

```bash
cd htsim-rs
cargo run -- dumbbell
```

### 3.2 查看参数帮助

```bash
cd htsim-rs
cargo run -- --help
cargo run -- dumbbell --help
```

### 3.3 常用参数示例

```bash
# 跑更久一点
cargo run -- dumbbell --until-ms 200

# 调整瓶颈链路带宽 / 发送间隔
cargo run -- dumbbell --bottleneck-gbps 5 --gap-us 5 --pkts 20000 --until-ms 200
```

### 3.4 为什么 injected 的包数和 delivered 不一致？

你可能会看到“注入 1000 个包，但 delivered 只有 500”。原因通常是：

- `until_ms` 太小：仿真在某个时间点停止，队列里还有大量“未来才会到达/转发”的事件没执行。

排查方法：

- 先把 `--until-ms` 调大（比如 10 倍），看 delivered 是否追上。

---

## 4. 如何调试（Debug）

Rust 的调试方式一般分三种：**打印/断言**、**回溯**、**调试器断点**。

### 4.1 最快：打印关键变量（推荐你第一时间用）

你可以在这些位置加 `eprintln!` 或 `dbg!`：

- `InjectFlow::execute()`：打印当前时间、还剩多少包、当前包 id
- `Network::forward_from()`：打印 `now/start/depart/arrive`、链路 `(from,to)`、`busy_until`
- `DeliverPacket::execute()`：打印到达时间、到达节点、`pkt.hops_taken`
- `Network::deliver()` / `Node::on_packet()`：打印节点名、是否还有下一跳

建议打印模板（举例）：

```rust
eprintln!(
    "[t={:?}] fwd pkt={} flow={} from={:?} to={:?} hops_taken={}",
    sim.now(), pkt.id, pkt.flow_id, from, to, pkt.hops_taken
);
```

### 4.2 出错了怎么看调用栈（Backtrace）

如果遇到 panic（比如 route 不匹配导致 “no link from ... to ...”），用：

```bash
cd htsim-rs
RUST_BACKTRACE=1 cargo run -- dumbbell
```

想要更详细：

```bash
RUST_BACKTRACE=full cargo run -- dumbbell
```

### 4.3 用 gdb/lldb 下断点（适合追复杂 bug）

先编译 debug 版本（默认就是）：

```bash
cd htsim-rs
cargo build
```

然后：

```bash
# gdb（推荐用 rust-gdb，如果系统提供）
rust-gdb target/debug/htsim-rs --args target/debug/htsim-rs dumbbell --pkts 1000 --until-ms 50

# 或 lldb
rust-lldb target/debug/htsim-rs -- dumbbell --pkts 1000 --until-ms 50
```

断点建议（从最关键的开始）：

- `htsim_rs::sim::Simulator::schedule`
- `htsim_rs::sim::Simulator::run_until`
- `htsim_rs::net::Network::forward_from`
- `htsim_rs::net::DeliverPacket::execute`
- `htsim_rs::net::Network::deliver`
- `htsim_rs::net::Host::on_packet` / `htsim_rs::net::Switch::on_packet`

### 4.4 用断言快速定位模型错误

常用断言点：

- 如果使用预设/混合路由（`Routing::Preset`/`Routing::Mixed`），则 `path/prefix` 必须非空，且注入时 `prefix[0]` 应该等于 `src`
- `pkt.hops_taken` 单调递增
- 每次 forward 前，`edges[(from,to)]` 必须存在（否则说明 route 或 connect 有问题）

---

## 5. 如何修改（Modify）

### 5.1 改拓扑（最常改）

改 `demo.rs` 的 `build_dumbbell()`：

- 增加/减少节点：`add_host` / `add_switch`
- 增加/修改链路：`connect(from, to, latency, bandwidth_bps)`
- 修改路径：`let route = vec![...]`

如果你要做更复杂的拓扑，建议新建一个函数，比如：

- `build_fattree(world, k, opts) -> ...`
- `build_line(world, n, opts) -> ...`

### 5.2 改链路模型（时延/带宽/排队）

目前链路排队用的是一个极简模型：`busy_until`（同一条链路串行发送）。

你最可能改的是：

- `Link::tx_time()`：更精确的单位/舍入方式
- `Network::forward_from()`：把 `busy_until` 替换成更真实的队列（比如多队列/优先级/ECN）

### 5.3 改流量注入（到达率、并发流、多源多宿）

现在只有一个 `InjectFlow`：

- 想加多流：再 schedule 多个 `InjectFlow`，每个用不同 `flow_id`、不同 route（预设路由），或改用 `make_packet_dynamic(src,dst)`（动态路由/ECMP）
- 想做突发/分段：把 `InjectFlow` 事件拆成“开始/停止/阶段切换”的多个事件

### 5.4 加协议（TCP/NDP/Swift/HPCC…）的落点建议

当前 `Node` 只是“按 route 转发”。当你要加协议，常见做法是：

- **把 Host 变成 endpoint**：`Host::on_packet` 根据 `pkt.type` 或 `flow_id` 进入协议状态机
- **新增事件类型**：例如 `SendData`, `RecvAck`, `RtoTimeout` 等，全部通过 `sim.schedule()` 驱动
- **把统计放进 World**：例如在 `NetWorld` 里增加 `flows: HashMap<flow_id, FlowState>`

---

## 6. 常见坑与定位方法

- **panic: no link from A to B**
  - **原因**：`route` 里写了 `A->B` 但没 `connect(A,B,...)`
  - **定位**：在 `Network::forward_from()` 打印 `(from,to)`，检查 `build_*` 里是否连了双向

- **delivered 很少/为 0**
  - **原因**：`until_ms` 太小；或 `gap_us` 太小导致队列排得很长；或路由最后一跳没连上
  - **定位**：先把 `--until-ms` 调大；再在 `DeliverPacket::execute` 打印到达次数

- **想“单步看事件顺序”**
  - **建议**：在 `Simulator::run_until()` 里每次 pop 事件后打印 `now`

---

## 7. 下一步我可以继续帮你做什么

你告诉我你要优先对齐 C++/Python 版里的哪一块（比如 `Packet/Route`、队列、某个协议 TCP/NDP/Swift），我可以按“最小可验证”方式把对应模块落进去，并配一个可跑的 demo/统计输出。

