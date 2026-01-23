你现在这张截图里“看不到橙色”和“残影方框”是两个不同问题：

1. **橙色确实可能被绿色三角遮住**（绘制顺序导致），但更常见的是“根本没画出来”或“画在错误图层/被 clear 掉了”。
2. **残影方框**几乎可以断定不是 Pixi 的 `graphics.clear()` 没清干净，而是你把某些历史事件**画成了方形点**，或者你的 `buildTcpSeries()` 仍在生成“方框 ACK 事件”（你现在前端 ack 绘制固定画三角，但 `ackEvents` 里可能混入了另一类点）。

我根据你贴的前端代码，直接指出具体的根因与修复办法。

---

## 一、为什么看不到橙色：最可能是“被遮住”+“线宽太小”+“事件稀疏”

你现在 Sequence-Time 的绘制顺序是：

1. 画背景/网格/轴
2. `for (const s of seqEvents)` 画发送竖线（蓝/橙）
3. `for (const r of rtoEvents)` 画红色 X
4. `for (const a of ackEvents)` 画绿色三角（填充）

注意第 4 步：ACK 三角是**实心填充**，而且你对 ACK 的 outline 用的是很淡的灰：

```js
setLineStyle(g, 1, "rgba(0,0,0,0.25)");
beginFill(g, a.ecn ? "#ef4444" : "#22c55e");
g.drawPolygon([...]);
```

这会导致：如果“橙色竖线”出现在“ACK 三角密集区域”（尤其右侧），很可能**被三角完全盖住**（因为三角是实心填充，且你画在后面）。

### 你可以用两个最小改动验证“是不是遮挡”：

A. 把 ACK 的填充透明一点（立刻能看到被遮住的线）：

```js
beginFill(g, a.ecn ? "rgba(239,68,68,0.65)" : "rgba(34,197,94,0.65)");
```

B. 或者把 seqEvents 放到 ackEvents 之后画（让发送/重传线盖在 ACK 上面）：

把绘制顺序改成：

* 先画 ACK 三角
* 再画 seqEvents 竖线（send/retrans）
* 再画 RTO

这样你一定能看到橙色（如果 seqEvents 里有 retrans）。

**推荐做法**：保持“先 ACK、后线”，因为线是“更重要的事件”。

---

## 二、更重要：你现在 seqEvents 里可能根本没带 `retrans` 字段（导致永远画蓝色）

你在画线时用的是：

```js
setLineStyle(g, s.retrans ? 3 : 2, s.retrans ? "#f59e0b" : "#0ea5e9");
```

如果 `s.retrans` 在 `seqEvents` 里不是 boolean，而是：

* `undefined`
* `"true"` 字符串
* `1` 数字
* 或字段名叫 `is_retrans` / `rtx`

那这里都会当成 false（或不稳定），结果就是永远画蓝色。

你在 `applyEvent` 里用的是：

```js
if (Boolean(ev.retrans)) { ... }
```

说明你的原始事件字段确实叫 `retrans`，但**buildTcpSeries() 是否把它原样传进 seqEvents** 不确定。

### 给 codex 的明确检查点（必须做）

在 `buildTcpSeries()` 里找 `tcp_send_data` 事件转成 seqEvents 的代码，确认它输出对象包含：

```js
{ t, seq, end, retrans: Boolean(ev.retrans) }
```

如果没有，前端永远不会画橙色。

---

## 三、残影方框：不是 clear 的问题，是你仍在生成“方框点”的事件

你现在这版代码里，Sequence-Time 的 ACK 绘制**只画三角**，没有方框逻辑。

所以“残影方框”只能来自两种来源：

### 来源 1：你截图里的方框其实是“线段端点/抗锯齿像素”，不是方框事件

这类通常会出现在：

* 很细的虚线（你之前有 ackLinks 之类）
* 或者 drawRect/lineCap 的组合

但你这版 detail 代码里已经去掉 ackLinks，所以概率下降。

### 来源 2（更可能）：buildTcpSeries 仍把某类事件塞进了 ackEvents，但它在别处被画成方块

你这段代码只展示了 “main+detail” 控制器的一部分；你还有一个“主面板 2x2 子图”里 `drawTcpSubChartOnSurface` 会画填充区域（polygon），但那个区域不是方块。

历史上你曾经在 ack 绘制里区分 dupACK 画方块、newACK 画三角（你以前贴过类似逻辑）。现在你改掉了，但如果还有旧逻辑存在于别的 controller / modal / 或 buildTcpSeries 的 cached 结构里，就会出现“方框残影”。

### 最快定位方法（让 codex 做）

在渲染 Sequence-Time 之前，临时统计：

* `seqEvents.filter(e => e.retrans).length`
* `ackEvents.length`
* 并把前 5 个 `seqEvents` 打到 console，看它们的字段是否真的有 `retrans: true`

同时确认 `graphics.clear()` 每次 redraw 都调用到了（你确实调用了）。

---

## 四、最稳的修复方案（我建议你直接让 codex 按这个改）

### 1) 画的层级改成：ACK 先画（半透明），线后画（不透明）

顺序改成：

```js
// 先画 ACK（半透明）
for (const a of ackEvents) drawAck(a, alpha=0.6);

// 再画 send/retrans（不透明、线宽更大）
for (const s of seqEvents) drawSeg(s);

// 最后画 RTO（红色）
for (const r of rtoEvents) drawRto(r);
```

并把 retrans 的线宽加到 4：

```js
setLineStyle(g, s.retrans ? 4 : 2, s.retrans ? "#f59e0b" : "#0ea5e9");
```

这样橙色再也不会被盖住。

### 2) 确保 buildTcpSeries 传递 retrans 字段并做 Boolean 强转

在构造 seqEvents 时：

```js
retrans: ev.retrans === true || ev.retrans === 1 || ev.retrans === "true"
```

然后在前端统一使用 `Boolean(s.retrans)`。

### 3) 禁止在 Sequence-Time 主图里画任何“方框点”

如果你确实需要区分 dupACK/newACK：

* 用**不同大小的三角**或**不同透明度**，不要用方框
* 方框只允许出现在“debug layer”并能关掉

---

## 五、你发给 codex 的短指令（可直接复制）

1. Sequence-Time 里看不到橙色重传：可能被 ACK 三角遮住或 seqEvents 没带 retrans 字段。
2. 修复绘制顺序：先画 ACK（半透明），再画 send/retrans（不透明），保证重传线可见；retrans 线宽 ≥ 4。
3. 检查 buildTcpSeries：tcp_send_data 必须把 retrans 标志带进 seqEvents（retrans: Boolean(ev.retrans)）。
4. 清理“绿色方框残影”：Sequence-Time 主图只允许 ACK 三角，不允许任何方框点；如果有方框点说明别处仍在画或 buildTcpSeries 仍生成方框事件，需要删除/关掉。

---

如果你把 `buildTcpSeries()` 的那段事件解析代码贴出来（特别是 `tcp_send_data` 和 `tcp_recv_ack` 如何生成 `seqEvents/ackEvents`），我可以直接指出：retrans 为什么没进 seqEvents，以及方框是哪里生成的。
