# Rust 版本 vs C++ 版本功能对比

本文档对比 `htsim-rs` (Rust) 和 `csg-htsim` (C++) 两个版本的功能差异。

## 📊 总体对比

| 类别 | C++ 版本 | Rust 版本 | 状态 |
|------|---------|-----------|------|
| **协议** | 11+ | 2 | ⚠️ 缺失较多 |
| **队列模型** | 15+ | 1 | ⚠️ 缺失较多 |
| **拓扑** | 9+ | 2 | ⚠️ 缺失较多 |
| **路由策略** | 10+ | 基础 | ⚠️ 缺失较多 |
| **统计/日志** | 完整 | 基础 | ⚠️ 缺失较多 |

---

## 🔌 协议层 (Protocols)

### ✅ 已实现
- ✅ **TCP** - 基础 TCP 实现
- ✅ **DCTCP** - DCTCP with ECN 支持

### ❌ 缺失的协议

#### 1. **MPTCP (Multipath TCP)**
- 文件：`mtcp.h/cpp`
- 功能：多路径 TCP，支持多条子流
- 路由策略：UNCOUPLED, COUPLED_INC, FULLY_COUPLED, COUPLED_TCP, COUPLED_EPSILON
- 重要性：⭐⭐⭐⭐ (用于数据中心多路径传输研究)

#### 2. **NDP (Near-Optimal Datacenter Transport)**
- 文件：`ndp.h/cpp`, `ndppacket.h/cpp`
- 功能：基于 pull 的传输协议，专为数据中心设计
- 特性：RTS, PULL, NACK 机制
- 重要性：⭐⭐⭐⭐⭐ (核心数据中心协议)

#### 3. **NDPTunnel**
- 文件：`ndptunnel.h/cpp`, `ndptunnelpacket.h`
- 功能：NDP 隧道封装
- 重要性：⭐⭐⭐

#### 4. **Swift**
- 文件：`swift.h/cpp`, `swiftpacket.h/cpp`, `swift_scheduler.h`
- 功能：Swift 传输协议
- 特性：调度器支持
- 重要性：⭐⭐⭐⭐

#### 5. **HPCC (High Precision Congestion Control)**
- 文件：`hpcc.h/cpp`, `hpccpacket.h/cpp`
- 功能：高精度拥塞控制
- 重要性：⭐⭐⭐⭐

#### 6. **RoCE (RDMA over Converged Ethernet)**
- 文件：`roce.h/cpp`, `rocepacket.h`
- 功能：RoCE 协议模拟
- 特性：支持 PFC (Priority Flow Control)
- 重要性：⭐⭐⭐⭐

#### 7. **EQDS (Efficient Queue-based Datacenter Switch)**
- 文件：`eqds.h/cpp`, `eqdspacket.h/cpp`, `eqds_logger.cpp`
- 功能：基于队列的数据中心交换机协议
- 重要性：⭐⭐⭐⭐

#### 8. **Strack**
- 文件：`strack.h/cpp`, `strackpacket.cpp`
- 功能：Strack 传输协议
- 重要性：⭐⭐⭐

#### 9. **CBR (Constant Bit Rate)**
- 文件：`cbr.h/cpp`, `cbrpacket.h/cpp`
- 功能：恒定比特率流量生成
- 重要性：⭐⭐ (用于测试和背景流量)

---

## 📦 队列模型 (Queue Disciplines)

### ✅ 已实现
- ✅ **DropTail** - 尾丢弃队列

### ❌ 缺失的队列类型

#### 1. **ECNQueue**
- 文件：`ecnqueue.h/cpp`
- 功能：支持 ECN (Explicit Congestion Notification) 标记
- 重要性：⭐⭐⭐⭐⭐ (DCTCP 依赖)

#### 2. **RandomQueue**
- 文件：`randomqueue.h/cpp`
- 功能：随机丢弃队列
- 重要性：⭐⭐⭐

#### 3. **CompositeQueue**
- 文件：`compositequeue.h/cpp`
- 功能：复合队列（多队列组合）
- 重要性：⭐⭐⭐⭐

#### 4. **PriorityQueue / CtrlPrioQueue**
- 文件：`prioqueue.h/cpp`
- 功能：优先级队列
- 重要性：⭐⭐⭐⭐

#### 5. **ECNPrioQueue**
- 文件：`ecnprioqueue.h/cpp`
- 功能：支持 ECN 的优先级队列
- 重要性：⭐⭐⭐⭐

#### 6. **CompositePrioQueue**
- 文件：`compositeprioqueue.h/cpp`
- 功能：复合优先级队列
- 重要性：⭐⭐⭐⭐

#### 7. **LosslessQueue / LosslessInputQueue / LosslessOutputQueue**
- 文件：`queue_lossless.h/cpp`, `queue_lossless_input.h/cpp`, `queue_lossless_output.h/cpp`
- 功能：无损队列（用于 PFC）
- 重要性：⭐⭐⭐⭐⭐ (RoCE/PFC 必需)

#### 8. **FairPullQueue / PrioPullQueue**
- 文件：`fairpullqueue.h/cpp`, `priopullqueue.h/cpp`
- 功能：Pull-based 队列（用于 NDP）
- 重要性：⭐⭐⭐⭐ (NDP 必需)

#### 9. **CutPayloadQueue**
- 文件：`cpqueue.h/cpp`
- 功能：截断载荷队列
- 重要性：⭐⭐

#### 10. **ExoQueue**
- 文件：`exoqueue.h/cpp`
- 功能：外部队列
- 重要性：⭐⭐

#### 11. **QcnQueue**
- 文件：`qcn.h/cpp`
- 功能：QCN (Quantized Congestion Notification) 队列
- 重要性：⭐⭐⭐

#### 12. **AeolusQueue**
- 文件：`aeolusqueue.h/cpp`
- 功能：Aeolus 队列模型
- 重要性：⭐⭐⭐

#### 13. **FairCompositeQueue**
- 文件：`faircompositequeue.h/cpp`
- 功能：公平复合队列
- 重要性：⭐⭐⭐

---

## 🗺️ 拓扑 (Topologies)

### ✅ 已实现
- ✅ **Dumbbell** - 哑铃拓扑
- ✅ **FatTree** - Fat-tree 拓扑

### ❌ 缺失的拓扑

#### 1. **VL2**
- 文件：`datacenter/vl2_topology.h/cpp`
- 功能：VL2 数据中心拓扑
- 重要性：⭐⭐⭐⭐

#### 2. **BCube**
- 文件：`datacenter/bcube_topology.h/cpp`
- 功能：BCube 数据中心拓扑
- 重要性：⭐⭐⭐⭐

#### 3. **DragonFly**
- 文件：`datacenter/dragon_fly_topology.h/cpp`
- 功能：DragonFly 拓扑
- 重要性：⭐⭐⭐⭐

#### 4. **OversubscribedFatTree**
- 文件：`datacenter/oversubscribed_fat_tree_topology.h/cpp`
- 功能：过订阅 Fat-tree
- 重要性：⭐⭐⭐

#### 5. **MultihomedFatTree**
- 文件：`datacenter/multihomed_fat_tree_topology.h/cpp`
- 功能：多宿 Fat-tree
- 重要性：⭐⭐⭐

#### 6. **Star**
- 文件：`datacenter/star_topology.h/cpp`
- 功能：星型拓扑
- 重要性：⭐⭐

#### 7. **CamCube**
- 文件：`datacenter/camcubetopology.h/cpp`
- 功能：CamCube 拓扑
- 重要性：⭐⭐⭐

#### 8. **GenericTopology**
- 文件：`datacenter/generic_topology.h/cpp`
- 功能：通用拓扑（从文件加载）
- 重要性：⭐⭐⭐⭐ (灵活性)

---

## 🛣️ 路由策略 (Routing Strategies)

### ✅ 已实现
- ✅ 基础路由（单路径）

### ❌ 缺失的路由策略

#### 1. **ECMP (Equal-Cost Multi-Path)**
- 策略：`ECMP_FIB`, `SCATTER_ECMP`
- 功能：等价多路径路由
- 重要性：⭐⭐⭐⭐⭐

#### 2. **ECMP with ECN**
- 策略：`ECMP_FIB_ECN`
- 功能：基于 ECN 的 ECMP
- 重要性：⭐⭐⭐⭐

#### 3. **Reactive ECN**
- 策略：`REACTIVE_ECN`
- 功能：响应式 ECN 路由切换
- 重要性：⭐⭐⭐⭐

#### 4. **Adaptive Routing**
- 策略：`ECMP_HOST_AR`, `ADAPTIVE_ROUTING`
- 功能：自适应路由
- 重要性：⭐⭐⭐⭐

#### 5. **Scatter Random**
- 策略：`SCATTER_RANDOM`
- 功能：随机分散路由
- 重要性：⭐⭐⭐

#### 6. **Scatter Permute**
- 策略：`SCATTER_PERMUTE`
- 功能：排列分散路由
- 重要性：⭐⭐⭐⭐

#### 7. **Pull-Based**
- 策略：`PULL_BASED`
- 功能：基于 pull 的路由（NDP）
- 重要性：⭐⭐⭐⭐

#### 8. **Round-Robin ECMP**
- 策略：`RR_ECMP`
- 功能：轮询 ECMP
- 重要性：⭐⭐⭐

#### 9. **First-Fit Routing**
- 文件：`datacenter/firstfit.h/cpp`
- 功能：首次适配路由
- 重要性：⭐⭐⭐

---

## 📊 统计与日志 (Logging & Statistics)

### ✅ 已实现
- ✅ 基础可视化 JSON 输出
- ✅ 基础统计

### ❌ 缺失的功能

#### 1. **Logger 系统**
- 文件：`loggers.h/cpp`, `loggertypes.h`, `logfile.cpp`
- 功能：
  - `QueueLogger` - 队列统计
  - `TcpLogger` - TCP 统计
  - `NdpLogger` - NDP 统计
  - `SwiftLogger` - Swift 统计
  - `HpccLogger` - HPCC 统计
  - `RoceLogger` - RoCE 统计
  - `EqdsLogger` - EQDS 统计
- 重要性：⭐⭐⭐⭐⭐

#### 2. **parse_output 工具**
- 文件：`parse_output.cpp`
- 功能：解析日志输出，生成 CSV/统计
- 重要性：⭐⭐⭐⭐

#### 3. **Connection Matrix**
- 文件：`datacenter/connection_matrix.h/cpp`
- 功能：连接矩阵（流量模式定义）
- 重要性：⭐⭐⭐⭐

#### 4. **Short Flows**
- 文件：`datacenter/shortflows.h/cpp`
- 功能：短流生成和管理
- 重要性：⭐⭐⭐⭐

---

## 🔧 其他功能

### ❌ 缺失的功能

#### 1. **Switch 模型**
- 文件：`switch.h/cpp`
- 功能：交换机转发逻辑
- 特性：FatTreeSwitch, 多种转发策略
- 重要性：⭐⭐⭐⭐⭐

#### 2. **Meter**
- 文件：`meter.h/cpp`
- 功能：流量计量
- 重要性：⭐⭐⭐

#### 3. **Route Table**
- 文件：`routetable.cpp`
- 功能：路由表管理
- 重要性：⭐⭐⭐⭐

#### 4. **Trigger**
- 文件：`trigger.h/cpp`
- 功能：事件触发器
- 重要性：⭐⭐⭐

#### 5. **Callback Pipe**
- 文件：`callback_pipe.h/cpp`
- 功能：回调管道
- 重要性：⭐⭐⭐

#### 6. **Transfer 抽象**
- 文件：`tcp_transfer.h`, `ndp_transfer.h`, `swift_transfer.h`, `dctcp_transfer.h`
- 功能：传输层抽象（便于实验）
- 重要性：⭐⭐⭐⭐

#### 7. **Periodic TCP**
- 文件：`tcp_periodic.h/cpp`
- 功能：周期性 TCP 流量
- 重要性：⭐⭐

#### 8. **Sent Packets Tracking**
- 文件：`sent_packets.h/cpp`
- 功能：已发送包追踪
- 重要性：⭐⭐⭐⭐

#### 9. **PFC (Priority Flow Control)**
- 文件：`eth_pause_packet.h`
- 功能：以太网暂停帧（PFC）
- 重要性：⭐⭐⭐⭐ (RoCE 必需)

---

## 📈 优先级建议

### 🔴 高优先级（核心功能）
1. **ECNQueue** - DCTCP 完整支持需要
2. **ECMP 路由** - 多路径必需
3. **Logger 系统** - 实验分析必需
4. **Switch 模型** - 数据中心仿真必需

### 🟡 中优先级（重要功能）
5. **NDP 协议** - 重要数据中心协议
6. **LosslessQueue** - RoCE/PFC 支持
7. **更多拓扑** (VL2, BCube, DragonFly)
8. **MPTCP** - 多路径传输研究

### 🟢 低优先级（扩展功能）
9. **Swift, HPCC, RoCE, EQDS** - 特定协议研究
10. **更多队列类型** - 特定场景需要
11. **Connection Matrix** - 复杂流量模式
12. **parse_output 工具** - 数据分析便利性

---

## 📝 总结

Rust 版本目前实现了：
- ✅ 核心仿真框架（事件驱动）
- ✅ 基础网络组件（Packet, Node, Link, Network）
- ✅ TCP 和 DCTCP 协议
- ✅ 基础拓扑（Dumbbell, FatTree）
- ✅ 基础可视化

**主要缺失**：
- ⚠️ 大部分协议（NDP, Swift, HPCC, RoCE, EQDS, MPTCP 等）
- ⚠️ 高级队列模型（ECN, Priority, Lossless, Pull-based 等）
- ⚠️ 高级路由策略（ECMP, Adaptive Routing 等）
- ⚠️ 完整的统计/日志系统
- ⚠️ 更多数据中心拓扑

**建议开发顺序**：
1. 先完善 ECNQueue 和 ECMP，使现有协议更完整
2. 然后添加 Logger 系统，便于实验分析
3. 再逐步添加其他协议和拓扑
