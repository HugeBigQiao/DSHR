# dshr 设计文档（单一真源）

> 本文件是 dshr 的**整体设计单一真源**：当前架构、边界、决策与操作约束。
> 「为什么变成这样、踩过什么坑、下一步想做什么」记录在 [`AI-LOG.md`](AI-LOG.md)。
>
> 官方参考仓库：`D:\dsh\deepseek-harness`（**源码是唯一权威**，本文是施工蓝图 + 决策记录；描述不准时以官方源码为准）。
> 官方 TS 客户端 `@deepseek-ai/dsh-sdk-client` 与 Python SDK 是 design twin（同 runtime 同协议）。
>
> 本轮（2026-09-28）：协议同步至官方 `0.1.7-rc.2`（59 种已知事件）；state 层定为
> **raw / engine / fold** 三层；`fold.rs`(1193 行) 与 `store.rs`(844 行) 完成拆分。

## 目录

1. [定位](#1-定位)
2. [架构总览](#2-架构总览)
3. [state 分层（dshr-state）](#3-state-分层dshr-state)
4. [wire 协议面](#4-wire-协议面)
5. [仓库布局](#5-仓库布局)
6. [协议 port 关键决策](#6-协议-port-关键决策)
7. [调用链](#7-调用链)
8. [数据罗盘 / 统计域 / 数据管道](#8-数据罗盘--统计域--数据管道)
9. [决策记录](#9-决策记录)
10. [关键事实与坑](#10-关键事实与坑)
11. [代码规范](#11-代码规范)
12. [待办与里程碑](#12-待办与里程碑)
13. [风险](#13-风险)

---

## 1. 定位

dshr = **Rust 三层 + 一个桌面端**：

| 层 | crate | 职责 |
|---|---|---|
| ① 协议 | `dsh-sdk-protocol` | wire 类型 + 帧层（纯逻辑，仅 serde） |
| ② 客户端 | `dsh-sdk-client` | 驱动**一个** runtime 子进程（`dsh --profile sdk`），stdio JSON-RPC |
| ③ 状态 | `dshr-state` | 桌面端 state：配置 / 记录 / runtime 获取 / **数据处理与落库** |
| ④ 界面 | `dshr-ui` | 桌面端薄壳（Iced 0.14，无边框 Zed 式布局） |

**核心判断**：官方给模型注册了 `cordis_inspect_*` / `cordis_define` / `cordis_run` 等自扩展工具
（`packages/extensions/tool-cordis`），模型能在会话内自写自装插件。所以桌面端只需
「固定基础方案」的薄壳 UI（凭据/模型/策略落文件，UI 极简）；薄壳场景下原生（Iced）劣势消失、
优势凸显（快/小/无 WebView）。

**宿主依赖**：需要 node（跑 dsh CLI）。官方 exe 打包在 Windows 是 non-goal。

---

## 2. 架构总览

```
┌──────────────────── dshr-ui（Iced，纯页面）────────────────────┐
│ 顶栏（Zed：页面标签 + — □ ✕ + 拖动区）                          │
│  ┌─侧边栏─┐  ┌─对话区─┐  ┌─详情─┐  底部图标栏（任务页）         │
└──────────────────────────┬─────────────────────────────────────┘
                           ▼ Cmd / ▲ Event（bridge 薄层：只搬运）
┌──────────────────── dshr-state ────────────────────────────────┐
│  engine   核心数据处理：多 runtime / 多会话路由、脏检测、落库节流  │
│    │  ▲ wire 级事件（带 runtime/session 标）                    │
│  raw      与 SDK 沟通：进程生死 + wire 协议 + 订阅 + WireLog      │
│  fold     纯投影：会话事件流 → 内存快照（被 engine 调用）          │
│  store    sqlite 加工库                                         │
└──────────────────────────┬─────────────────────────────────────┘
                           ▼ SessionDriver trait（3 请求 / 4 通知 + stderr + 退出）
                   dsh-sdk-client：client(总装) + transport(管道) + process(生死)
                           ▼ spawn + stdio JSON-RPC
                   dsh --profile sdk（官方 runtime，node 子进程）
```

**UI 设计原则**：页面参考官方 `packages/client/*`（AppFrame 三栏、SettingsRoot 遮罩面板、
SidebarRoot、消息气泡/工具卡片、StatsLine、composer dock）；窗口/顶栏布局参考 Zed
（标签与窗口控制同排、无边框 + 拖动区、底部图标栏）。

---

## 3. state 分层（dshr-state）

### 3.1 三层职责与命名

| 层 | 名字 | 职责 | 什么会改它 |
|---|---|---|---|
| 与 SDK 沟通 | **raw** | 进程生死、wire 协议、订阅、WireLog；把「一个有状态的子进程」收敛成可替换接口 | 官方发版 |
| 核心数据处理 | **engine** | 多 runtime/会话的路由、脏检测、快照缓存、落库节流 | 产品需求 |
| UI 数据查询 | **fold** | 纯投影：事件 → UI 可消费的数据（**保持无副作用**） | UI 形态 |

依赖方向**单向**：`ui → engine → raw → dsh-sdk-client`。
`fold` 与 `snapshot` 是**被 engine 调用的纯数据层**（不在这条链上）：
`engine → fold → snapshot::SessionSnapshot`。

### 3.2 为什么不合并 engine 与 raw

1. **变化频率不同**：raw 跟官方发版走；engine 跟产品走。合并后每次官方发版都要动数据管道代码。
2. **可测性会塌**：raw 碰进程（`ChildStdin` / `oneshot` / `Arc<Mutex<Child>>`），
   合并后纯数据管道无法用假 driver 单测（不起进程）。
3. **与多 runtime 冲突**：engine 需要 runtime 注册表 + 跨会话聚合；raw 需要每 runtime 独立的
   进程生命周期。放一个结构里就是又一个 God object。

### 3.3 raw 不是「薄层」，是端口

raw 内是**真逻辑**，不是转发：帧分类与 id 配对、请求超时、`TransportClosed`（带 exit code +
stderr 尾部）、dispose 阶梯（EOF → [SIGTERM] → SIGKILL，Windows 跳过 SIGTERM）、
WireLog 双向全量记录、官方 4 个错误类映射。

**它的价值 = 把有状态、有副作用、难测的世界收敛成可替换接口**，让上层只处理数据。
判断一个层是否该存在，看它**约束了什么**，不看行数。

### 3.4 fold 变薄是对的

fold 的职责就是投影；薄才能纯，纯才能用「事件 JSON → 快照相等」断言。
它是唯一能被单测完全覆盖的层——协议漂移/字段改名/新事件全靠它兜住而不炸 UI。

### 3.5 SessionDriver trait（粒度：runtime 级）

**状态：已落地（M2）**。实现在 `dshr-state/src/raw/driver.rs`（接口）与
`raw/client_driver.rs`（`HarnessClient` 适配）。

**粒度依据**：`Transport` 是**每 runtime 一条**（持有单个 `wire_log: Option<Arc<WireLog>>`、
一个广播发送端、一个 stdin）。所以 trait 实例 = 一个 runtime，`session_id` 作为方法参数传入。

完整暴露面 = **三进四出 + 两类进程信号**（后两类**不走协议通知**，必须显式暴露）：

```rust
/// 一个 runtime 的驱动面。实现方持有进程 + 管道；调用方只处理数据。
#[async_trait]
pub trait SessionDriver: Send {
    /// 进程级握手（provider/model 路由）。
    async fn initialize(&mut self, params: &InitializeParams) -> Result<InitializeResult, Error>;

    /// 发一条用户消息（sessionId 是参数 → 同一 runtime 可多会话）。
    async fn prompt(&mut self, params: &SessionPromptParams) -> Result<SessionPromptResult, Error>;

    /// 取通知流（4 种通知的原始帧；解析在 protocol::notifications::parse）。
    fn events(&mut self) -> broadcast::Receiver<Notification>;

    /// 进程 stderr 流（第二类信号；store 有 runtime_logs 表等它落库）。
    fn stderr(&mut self) -> mpsc::UnboundedReceiver<String>;

    /// 进程退出信号（第三类信号：exit code + stderr 尾部 + 是否异常）。
    fn exit_signal(&mut self) -> ExitSignal;

    /// 收尾：协议 shutdown → dispose 阶梯。消费型（官方 close 语义）。
    async fn shutdown(self: Box<Self>) -> Result<(), Error>;
}
```

**为什么这两类必须在 trait 里**：引擎要做「runtime 死了要报错 + 落库审计」，
只靠四通知拿不到——现在只能从 `Error::TransportClosed` **间接**看出退出，
而 stderr 流整个被丢弃（`take_stderr()` 无人调用）。

### 3.6 落盘完整性原则

**尽可能多地暴露并落盘数据**。已定的取舍：

- **逐 chunk 不落盘**（空间代价不可接受），改为**按会话记录完整 chunk 序列**：
  一条 `assistant/message.stream` 保留该次消息的全部 `AssistantStreamRecord`，
  而不是每个 chunk 一行数据库记录。
- **`stderr` 必须落盘**（`store/runtime_logs` 表已建，等 engine 消费）。
- **进程退出必须落盘**（exit code + stderr 尾部）为审计事实。

### 3.7 已知的结构错配（多 runtime/会话的待做项）

`EngineCmd` 是**运行时级**（无 session 标识），`EngineEvent::Snapshot` 是**会话级**（带 session_id）
——两者无法对上，`last_sent: Option<SessionSnapshot>` 的单会话去重就是这个假设的产物。

**待做**：`runtime_id → session_id → 会话态` 两级结构；`Bridge::feed` 现在**过滤阶段直接丢弃**
非当前会话的通知，多会话化时要改成路由。

---

## 4. wire 协议面

### 4.1 方法面（7 种消息，双向）

**请求侧（client → server，3 个）：**

| method | params | result |
|---|---|---|
| `initialize` | `InitializeParams`（cwd / provider / model / reasoningEffort? / maxTokens?） | `InitializeResult`（`serverInfo{name,version}`） |
| `session/prompt` | `SessionPromptParams`（**sessionId** / contentBlocks[]） | `SessionPromptResult`（messageId 入队回执） |
| `shutdown` | 无（wire 上 `{}`） | 空对象 `{}` |

**通知侧（server → client，4 个）：**

| method | payload |
|---|---|
| `session.event` | `{ sessionId, event: SessionEvent }` |
| `session.status` | `{ sessionId, status: 'idle' \| 'running' }` |
| `subagent.started` | `{ parentSessionId, childSessionId }` |
| `subagent.finished` | `{ provider, agentId, parentSessionId, childSessionId, status, stopReason, lastAssistantMessage? }` |

**方向判定规则**：请求带 `id`（配对响应）；通知无 `id` 有 `method`。`rpc::classify` 按此区分。

**协议面本身不含「runtime」概念**——多 runtime 是产品能力，由 engine 持注册表实现。
一个 runtime 内可并存多会话（官方 `session/prompt` 的 `sessionId` 注释：unknown id lazily
creates the agent+session pair）。

### 4.2 会话事件全集（59 种）

事件全集来自官方 `packages/core/session/src/known-event-types.ts`（**上游生成物，勿手改**）。
dshr 侧 59 种全部结构化，另有 `Unknown` 兜底（lossless）。

**维护纪律**：

- 新增/变更事件类型时，**三处同步**：判别枚举变体 + `session_event/fallback.rs` 的手写 `Deserialize`
  分发 + `session_event/meta.rs` 的 `as_str/time/seq/turn_step` 四方法。漏任一处要么解析不到、
  要么穷尽匹配编译失败。
- **机器化对账（两处，别互相替代）**：
  ① `node scripts/compare-session-events.mjs` —— **对官方真源**（需要 deepseek-harness 克隆），
  退出码 1 = 有差异；当前结果 **59 = 59，零差异**。
  ② `cargo test -p dsh-sdk-protocol --test event_catalog` —— **对锁定的官方快照**
 （`dsh-sdk-protocol/tests/fixtures/known-event-types.txt`），只 clone 了 dshr 也能跑。
  **不要手工比对**（规模已到必须脚本化）。
- 同步前先用上游标签做权威 diff（见 AI-LOG §5）。

### 4.3 内容块（7 种）

官方 `packages/llm/llm/src/types.ts` 的 `ContentBlockMap`：
`text` / `reasoning` / `image` / `file` / `tool-call` / `tool-addition` / `tool-removal`。

dshr 额外保留 `ToolResult`（官方 0.1.7-rc.2 已移除该块，保留变体**仅作旧日志读取兼容**）
与 `Unknown`（插件扩展面 lossless 兜底）。

### 4.4 消息来源（`MessageSource`）

一条消息「是谁生产的」由 `Message.source`（`{kind:…}` 对象）声明。
官方是 **merge-extensible**：每个生产者在自己的包里 `declare module '@deepseek-ai/dsh-llm'`
注册自己的 kind（**没有**统一的 catch-all `plugin` kind）。

**kind 的权威判据（0.1.7-rc.2 起）**：官方**没有**汇总式的基座 `MessageSourceMap` 了
（0.1.2 时代那份在 `packages/llm/llm/src/message.ts`，现在拆到各包的
`declare module '@deepseek-ai/dsh-llm'`）。核对顺序：
① **真实帧**（`data/wire-logs` 里的 `*.message.source`）→
② **合并声明**（`dsh/node_modules/@deepseek-ai/dsh-llm/lib/typert.host.js` 里嵌了完整 `.d.ts`）→
③ 迁移表（`packages/session/session-format-v3-to-v4/src/sources.ts`）。

**当前状态（2026-09-29）**：已建模 **20 种** kind。
user / model / tool / system-prompt / runtime-context / plugin / goal / webhook / skill-catalog /
skill-invocation / agent-instructions / session-reference / agent-message / subagent-settled /
team-message / **plan-mode / model-selection / user-approval / ptc-mode / compact-checkpoint**（后 5 个为本次补齐）。

**未建模的 kind 会让含它的整条消息降级 `Unknown`**：丢的是结构化视图（见下），
**wire log 的原始 JSONL 无损**。界面目前看不出差别（fold 只折 `role=user 且 source.kind=user`
的行，`system/message`、`developer/message` 也不折），代价落在「s3 按来源分类渲染」与统计域上。

**降级不再静默**（本次新增）：
`SessionEvent::Unknown` 现在带 `degraded: bool` 区分两种情况——「**已知类型但 data 解析失败**」
（协议漂移，`degraded = true`）与「类型本身就未知」（插件自注册，merge-extensible 的**预期**行为）。
engine 对前者写一条 app 轨迹 `{"cat":"app","kind":"event.degraded","data":{sessionId,eventType,seq}}`，
用 `scripts/scan-message-sources.mjs` 可汇总。**这是本节的护栏 ①**。

**护栏与其分工**：

| 手段 | 守什么 | 边界 |
|---|---|---|
| `tests/frame_shape.rs::real_message_sources_are_modelled` | fixture 样本里已知的 kind 不被改坏 | 看不见样本外的新 kind |
| engine 的 `event.degraded` 轨迹 | 运行时真实发生的漂移 | 事后可见（不是拦截） |
| `scripts/scan-message-sources.mjs` | **发现**新 kind 与字段漂移（扫已安装 runtime + 合并声明 + 日志） | 需在装了 runtime 的机器上跑 |

**不在本 profile 依赖闭包里的**（`time-context` / `tmux-context` / `coordinator` /
`subagent-report` / `tool-cordis` / `cordis-host-runner` / `schedule` / `hooks-*` /
`tool-registry` …）**不会出现**，不必建模；`scan-message-sources.mjs` 会按「是否在依赖闭包内」
自动区分「真实缺口」与「理论存在」。

**已建模但与官方声明不一致的风险**：无（脚本的逐字段比对当前全绿）。

---

## 5. 仓库布局

```
dshr/
├── Cargo.toml                # workspace（protocol + client + state + ui）
├── README.md                 # 项目门面（三层简介 + 快速开始 + data/ 说明）
├── DESIGN.md                 # 本文件：整体设计单一真源
├── AI-LOG.md                 # 交流记录 / 踩坑 / 设想 / 决策简史（过程）
├── dsh/                      # dsh 本体（运行时下载，发布不带，gitignore）
├── config.json               # 本地配置（provider/model/dsh-version，gitignore）
├── data/                     # 状态数据（gitignore）：dsh-home / wire-logs / .pnpm-store / secrets.json
│
├── dsh-sdk-protocol/         # ① 协议层（纯逻辑，仅 serde）
│   └── src/
│       ├── lib.rs            # pub mod 汇总
│       ├── rpc.rs            # 帧层 ← 官方 transport.ts
│       ├── requests.rs       # 请求侧 wire 类型根 ← HarnessSdkRequestMap
│       ├── requests/         #   initialize / session / shutdown
│       ├── content_block.rs  # 内容块根 ← ContentBlockMap
│       ├── content_block/    #   contentblock（类型）/ fallback（未知块兜底）
│       ├── session_event.rs  # SessionEvent 信封 + 判别枚举 + turn_step() ← SessionEventMap
│       ├── session_event/    #   事件 data 按事件族拆（59 种 + Unknown 兜底）
│       ├── llm.rs            # TokenUsage / FinishReason / StreamChunk / LlmFailure ← llm/types.ts
│       ├── notifications.rs  # 通知侧 wire 类型 + Kind 分发 ← NotificationMap
│       └── subagent.rs       # SubagentStopReason ← subagent/types.ts
│
├── dsh-sdk-client/           # ② 客户端：管理单个 runtime 进程
│   ├── src/
│   │   ├── lib.rs            # crate 声明
│   │   ├── error.rs          # 统一错误（4 类对应官方错误类，From 链吸收 ParseError）
│   │   ├── client.rs         # 总装师：HarnessClient 类型化方法 API
│   │   ├── transport.rs      # 管道对话：读循环 + id 配对 + 事件广播 + WireLog
│   │   ├── process.rs        # 进程生死：spawn / stderr / exit 监控 / dispose 阶梯
│   │   ├── subscription.rs   # 事件订阅 + 会话树 scoping（≈ subscribeSessionTree）
│   │   └── api.rs            # run() receipt-to-idle（≈ DeepSeekHarness.run）
│   └── tests/                # 集成测试（fake runtime 进程）
│
├── dshr-state/               # ③ 状态层（详见 dshr-state/README.md）
│   ├── src/
│   │   ├── lib.rs            # 分层说明 + 模块清单
│   │   ├── raw.rs  raw/      #   与 SDK 沟通（Runtime + mode + driver/client_driver）
│   │   ├── engine.rs engine/ #   核心数据处理（M3 落地；当前为 raw 的再出口）
│   │   ├── fold.rs fold/     #   纯投影（Folder + event / render）
│   │   ├── snapshot.rs       #   fold 的输出类型（UI 模型；含 last_request）
│   │   ├── store.rs store/   #   sqlite 加工库（门面 + error/schema/convert/write）
│   │   ├── export.rs         #   CSV 导出（库表 + 跨会话历史回放；监控页导出先行）
│   │   ├── record.rs         #   WireLog 装载
│   │   ├── runtime.rs        #   runtime 获取（锁版本 pnpm install --ignore-scripts）
│   │   ├── config.rs         #   配置加载（config.json）
│   │   ├── secrets.rs        #   API key（data/secrets.json，Unix 0600）
│   │   └── workspace.rs      #   工作区文件读写（仅限相对路径）
│   │   └── main.rs           #   可执行入口（全链路运行 + 记录汇总）
│   └── README.md             # 本 crate 的组成结构与依赖关系
│
└── dshr-ui/                  # ④ 桌面端薄壳 UI（Iced 0.14）
    ├── src/
    │   ├── main.rs           # iced::application，无边框窗口（decorations: false）
    │   ├── app.rs            # 根状态机 + 消息分发 + 窗口控制
    │   ├── bridge.rs         # 总线：Cmd/Event 搬运 + iced 订阅装配
    │   ├── nav.rs            # 顶栏：页面标签 + 自绘 — □ ✕ + 拖动区
    │   ├── theme.rs          # 官方设计 token → Palette（深/浅）+ 控件样式
    │   ├── model.rs          # UI 视图模型（由 snapshot 映射）
    │   ├── dpi.rs            # DPI 一致性兜底（见决策 §9.16/§9.17）
    │   ├── widgets.rs widgets/  # 自定义控件（Popover 覆盖式菜单）
    │   ├── task.rs task/     #   任务页（sidebar / chat / details）
    │   ├── files.rs files/   #   文件页（工作区树 + 代码编辑器）
    │   ├── monitor.rs        # 监控页（占位）
    │   ├── setting.rs        # 配置页（左类别导航 + 右内容）
    │   └── statusbar.rs      # 底部图标栏
    └── Cargo.toml            # iced features：默认 wgpu-renderer + code-editor
```

**工作区级脚本**在 `D:\dsh\scripts\`（不在 dshr 内）：步骤索引见 `scripts/pipeline.json`。

---

## 6. 协议 port 关键决策

1. **判别联合用 `#[serde(tag = "type")]`**：事件 wire 类型带斜杠（`turn/start`），每个变体显式
   `#[serde(rename = "...")]`；data 结构体驼峰处 `camelCase`；嵌套联合（如 `FinishReason`、`TurnEndReason`）
   用 `tag = "kind"`。
2. **merge-extensible 必须宽容**（两层兜底）：
   - **事件级**：信封 → 按 type 分发，未知进 `Unknown`（lossless 保留）。
   - **已知但漂移**：`fallback.rs` 的 `known()` 助手——已知类型 data 解析失败也降级 `Unknown`，
     不整体报错。这是 `reason: 'series'` 事件的通用解法，有回归测试。
   - **字符串联合枚举**加 `#[serde(other)]` 兜底（`TurnEndReason::Other`、`MessageRole::Other`）。
   官方自己要求读端宽容未知（`known-event-types.ts` 注释）。
3. **transport 划分**：帧逻辑（构造/判断/解析/信封）全在 `protocol/rpc.rs`（零依赖纯函数）；
   管道 I/O + 配对在 `client/transport.rs`。
4. **错误分层**：`protocol::rpc::ParseError`（帧层）+ `client::Error`（thiserror，`From` 链吸收）——
   不建单独 error crate。
5. **事件通道结构化**：通知以 `Notification { method, params: Value }` 出通道，消费方按 method 解析
   （`notifications::parse` → `Kind`）。
6. **行数约束**：单文件平均 ≤350 行；超了拆文件（`fold`、`store` 已按此拆分）。
7. **测试惯例**：协议改动**必须带回归测试**。⚠️ **全部测试已于 2026-09-29 清空**，正在按新布局重建——见 §12.4「测试策略」。

---

## 7. 调用链

### 7.1 SDK 层（client + transport）

```
consumer
├─ HarnessClient::spawn(config)                      [client/client.rs]
│   ├─ RuntimeProcess::spawn(config)                 [client/process.rs]
│   │   └─ Command::new("node").args([dsh_bin, "--profile", "sdk"])…spawn()
│   └─ Transport::start(stdin, stdout, status, wire_log)   [client/transport.rs]
│       └─ tokio::spawn(读循环)：lines.next_line() → rpc::classify
│            ├─ Response{id}  → pending.remove(id) → tx.send(Ok(line))
│            ├─ Notification → wire_log.record_recv → events_tx.send(...)
│            └─ EOF → 失败所有 pending（Error::TransportClosed{exit_code, stderr_tail}）
├─ client.initialize(&InitializeParams) → transport.request("initialize") → rpc::parse
├─ client.prompt(&SessionPromptParams) → 同上，返回 messageId 入队回执
├─ 事件消费：client.take_events() → Notification → notifications::parse → Kind（4 种之一）
└─ client.shutdown() → 协议 shutdown → process.dispose(EOF→[SIGTERM]→SIGKILL)
```

**一句话**：`client` 三行委托（序列化 → `transport.request` → `rpc.parse`），`transport` 管"写+配对"
（读循环后台常驻），`process` 管生死，`rpc` 管帧形状。

### 7.2 state 层（目标形态）

```
UI ──EngineCmd──▶ engine ──SessionDriver──▶ raw ──▶ dsh --profile sdk
UI ◀─EngineEvent── engine ◀──wire 级事件──── raw
                     │
                     ├─▶ fold::Folder ─▶ snapshot::SessionSnapshot（快照缓存，UI 按需读）
                     └─▶ store::Store（落库：会话/turn/工具/文件/审计）
```

读取模型（M6 定的 **B 方案**：拉 + 变更通知）：engine 持快照缓存，事件只发轻量变更通知，
UI 按需读快照（避免多会话下整份 clone 的开销）。

**落地状态（2026-09-29）**：

- ✅ **「拉」这一半已完成**：`EngineCmd::ReadSnapshot { session }` → `EngineEvent::SessionLoaded
  { session, runtime: Option<RuntimeId>, snapshot }`；`EngineCmd::ListSessions` →
  `EngineEvent::Sessions { rows }`（库里的聚合目录，§8.3）。
  查找顺序 = **运行中优先 → 库里复原**（`Store::load_snapshot`），所以「打开历史会话」
  与「切到当前会话」用的是同一条路径、同一个事件形状。
  三个边界：**只读**（不脏检测、不落库——落库由变更路径负责）、**历史态不缓存**
  （读几百行是毫秒级，缓存反而引入失效问题）、**找不到就静默**（不造空快照）。
- ⬜ **「只发轻通知」这一半与 UI 同批做**（M5）：执行者（engine 停发整份快照、改发
  `SnapshotChanged`）与消费者（UI 收到通知后发 `ReadSnapshot`）**必须同时改**——
  只改一侧的结果是界面静止（推送没了而没人去拉），而静默正是本项目最贵的失败形态。
  切换时 `SessionLoaded` 的应用逻辑已经就位（`dshr-ui/src/app.rs` 与推送快照走同一段），
  只需再加上「收到轻通知 → 发 ReadSnapshot」这条回路。

### 7.3 UI 层

```
App::view   ── nav(顶栏) + task/sidebar(树) + task/chat(对话) + statusbar
App::update ── Message 分发：
  ├─ Window(cmd) → iced::window::{minimize,maximize,close,drag}(window_id)
  ├─ Task(⋯ 菜单) → Popover（自定义 advanced widget）
  ├─ Task(Send)   → cmd_tx.send(BridgeCmd::Prompt{..}) → engine → raw → runtime
  └─ Task(Edit)   → composer.perform(action)（Edit::Enter 除外——转 Send）
```

---

## 8. 数据罗盘 / 统计域 / 数据管道

### 8.1 数据罗盘：`data/`

| 路径 | 归属 | 内容 |
|---|---|---|
| `data/dshr.db` | dshr 加工库（rusqlite） | 会话/轮/工具/文件变更事实表（§8.2） |
| `data/config.json` | dshr 配置 | provider / model / dsh-version |
| `data/secrets.json` | dshr 敏感 | api-key（0600，不入 git） |
| `data/dsh-home/` | dsh runtime（**不碰**） | profiles / sessions / storages / 匿名 id |
| `data/wire-logs/` | dshr 记录 | 全程 JSONL（**lossless 源**） |
| `data/exports/` | dshr 导出 | CSV 导出产物（`DSHR_EXPORT=1` 真跑；可整目录删除） |
| `data/.pnpm-store/` | pnpm 缓存 | 安装 store |

原则：**db 只含 dshr 自己的加工数据**；dsh 的会话/storages 留在 `dsh-home/`。

### 8.2 dshr.db 表集

| 表 | 内容 | 写入方 |
|---|---|---|
| `runtimes` | id/name/state/created/command/args/cwd/env | ✅ engine（启动/退出）；command/args/cwd/env 留位 |
| `sessions` | id/runtime_id/cwd/parent/created/status/title/last_seq + **`meta_json`**（复原用聚合） | ✅ fold 快照（+ `ensure_session` 壳行） |
| `requests` | session/runtime/turn/method/time/duration_ms/success/**error_message** | ✅ engine（每次 prompt，含失败） |
| `messages` | **逐条对话事实**：turn/step/kind/source/text/reasoning/**error**/token 六桶/流摘要/工具全文（含 arguments 与原样 meta） | ✅ fold 快照 |
| `turns` | turn_id/session/turn/started/ended/duration/reason + token 六桶列 | ✅ fold 快照 |
| `tool_calls` | call_id/name/arguments/result/is_error/duration_ms/**error**/meta_json | ✅ fold 快照 |
| `file_ops` | session/turn/time/path/op/lines_added/lines_removed | ✅ 自 `meta.diffs` 折叠（turn 已能填） |
| `runtime_logs` | runtime stderr 行（审计） | ✅ engine 消费 stderr |

**落库粒度原则（2026-09-29 用户要求后定）**：**除逐 chunk 之外全部落库**——
消息（含程序化注入与未提交的尝试）、每条工具调用及其**失败原因**、每轮的结算与 token、
宿主发出的每次请求（耗时/成败/原因）。原文类字段**不截断**（截断只发生在渲染层），
逐 chunk 仍只留流摘要（`stream_*` 七列），与 §8.3 的统计域一致。

**仍然不建 events 全量表**：wire-logs JSONL 已是 lossless 原始源，事件级重放走它；
但要分清「事件」与「事实」——**事件不逐条落库，消息/轮/工具/请求这些事实必须落库**，
因为它们是查询与复原的对象（不是日志的副本）。

**写入语义 = 会话整体重放**：`persist_snapshot` 一个事务内 UPSERT `sessions` +
DELETE+INSERT `messages`/`turns`/`tool_calls`/`file_ops`，同一快照重复 persist 行数不变（幂等）。

**schema 版本与迁移**：`PRAGMA user_version` 记录版本（当前 2），加列走
`schema::MIGRATIONS_V2` 的「先探测后 ALTER」（幂等）。**用户的历史会话在库里**，
所以升级不能靠删库（§8.1 的「data/ 可整体删除」是兜底不是流程）。

**复原（关掉再打开）**：`Store::load_snapshot(session_id)` 用 `messages` + `turns` +
`sessions.meta_json` 重建完整快照；`load_session_ids()` 给出目录（按最后更新倒序）。
契约测试 `tests/store_persistence.rs::restore_roundtrip` 断言**逐字段相等**——
不等就说明某个字段在落库或读回时丢了语义（实测例子：把 adapter **未报**的 token 桶写成 0，
读回就从「未知」变成「0」；所以六桶列可空）。

### 8.3 统计域（含 stream 摘要，**不保留逐 chunk**）

`assistant/message.stream` / `assistant/attempt.stream` 记录紧凑流；dshr 展开后只保留统计摘要
（chunks / 首 token 延迟 / 时长 / text 与 reasoning 字符数），**不保留逐 chunk 内容**
（避免快照重复克隆；逐 chunk 落盘经评估空间代价不可接受）。

其余按层级全统计（落库 = §8.2 事实表；跨层聚合 = read 层函数，不入库）：

| 层 | 统计项 |
|---|---|
| 请求 | method / time / duration_ms / success / provider+model / reason / LlmFailure |
| 轮 | turn/start–end、tokens 六桶、reason、step 数 |
| 工具 | 每工具名：次数 / 成功失败 / 总耗时 / 平均耗时；call↔result 配对 |
| 文件 | 每 path：op 计数、+n / −m 合计、按会话/轮时间线 |
| 会话 | 起止 / 轮数 / 总 token 六桶 / 工具次数 / 错误数 / 标题 |
| 系统 | runtime stderr、进程退出码、spawn/退出时间 |

### 8.4 数据管道

```
wire 事件（SDK 通知 / WireLog 回放）
   │  同一巡两个去向，同一折叠语义
   ▼
fold（纯函数，可测）      ──► 内存快照：消息流 / turn 统计 / 会话树
   ▼
落库（事实表写入）        ──► 历史查询：监控页 / 会话目录 / 跨会话聚合
```

- fold 与落库**同源同巡**；**离线模式**：WireLog 回放走同一 fold（UI 开发/回归用，
 不 spawn runtime、不烧 token）。
- **导出**（`dshr-state/src/export.rs`，2026-09-29）：库表 → CSV（显式列序 + 稳定排序）、
  wire-log → **按会话分组回放**出逐条历史。它是监控页历史导出的先行实现，
  入口暂时是门控测试（`DSHR_EXPORT=1`），见 AI-LOG §4.8。
  实测差异值得记住：wire-log 回放出 42 个会话，而库里只有 3 行 session——**历史的全集在 wire-log**，
  库是「引擎跑过并落盘过的」加工结果（§4.5 的历史索引议题就建在这个事实之上）。

---

## 9. 决策记录

> 只记**当前有效**的决策与其理由；被推翻的见 AI-LOG 的决策简史。

### 9.1 协议与 runtime

1. **runtime = `dsh --profile sdk`，锁版本**：npm `@deepseek-ai/dsh` 的 `latest` 长期不含 sdk profile
   （现为 0.1.7-rc.2，`next` 是 0.2.0-rc.1），**必须显式锁版本**。当前锁 `0.1.7-rc.2`。
2. **DSH_HOME 独立**：spawn 时给 runtime 单独 `DSH_HOME`（`<管理目录>/data/dsh-home`），
   不碰用户 `~/.dsh`；工作区经 `DSH_CWD` env + `InitializeParams.cwd` 锁死。
3. **结构化范围 = 全集**：官方 `known-event-types.ts` 全集（当前 59 种）全部结构化，
   另有 `Unknown` lossless 兜底。同步按上游标签 diff 驱动（见 AI-LOG §5）。
4. **序列化兼容**：官方新增字段一律 `Option + skip_serializing_if`（wire 可选，缺省 = 旧行为）。
5. **版本校验读已安装包的 `package.json`**：**不可**用 `InitializeResult.serverInfo.version`
   （官方硬编码 `'0.0.1'`，见 AI-LOG §2.1）。
6. **0.2.0-rc.1 暂不采纳**：经标签 diff 核实其协议层零改动，换版本无功能收益、反引入行为变量。

### 9.2 发布与获取

7. **发布策略 = 独立 crate + 生态目录**（awesome-dsh-plugin / dshget / market catalog）；
   官方树内收编等协议 1.0 稳定后（参照 python/ 进树先例）。**发布等 SDK 全做完 + 测试完再说**。
8. **runtime 获取**：dsh 本体放 `dshr/dsh/`（发布不带，运行时检测/下载，删除可重下）。
   包管理器 **pnpm**：共享全局 store 去重 + `--ignore-scripts`（实测 node-pty/koffi 的 tarball
   自带预编译产物，跳过构建完全可用——免 node-gyp 工具链）+ `--config.minimumReleaseAge=0`
   （pnpm 供应链年龄策略默认拒绝刚发布的 alpha 包）。node 检测 ≥22.19。

### 9.3 state 分层

9. **三层：raw / engine / fold**（2026-09-28 定）。理由与边界见 §3。
10. **`SessionDriver` trait 粒度 = runtime 级**，暴露三进四出 + stderr + 退出信号（§3.5）。
11. **快照读取用 B 方案**（拉 + 变更通知）：engine 持快照缓存，事件只发轻通知（§7.2）。
    拉取侧已落地（`ReadSnapshot`/`SessionLoaded`/`ListSessions`/`Sessions`）；
    轻通知的**切换**与消费侧同批做（M5，理由见 §7.2）。
12. **落盘完整性原则**（§3.6）：逐 chunk 不落盘、改为按会话记录完整 chunk 序列；
    stderr 与进程退出必须落盘。
13. **`session.rs` 删除**（推迟到 M4）：它用的 `HarnessClient::run()` 语义当前 engine 接口没有，
    直接删会丢功能；需先补 `run()` 语义与可配置 WireLog 目录。
14. **`mod.rs` 弃用**：统一 Rust 2018+ 风格 `x.rs` + `x/`。

### 9.4 UI

15. **页面设计全参考官方 deepseek-harness**（用户指令）：token 对齐官方
    `packages/client/ui-theme/src/styles/design-platform.css` 的 `--dsw-alias-*`
    （bg_base 21,21,23 / layer1-3 / label_primary 249,250,251 / accent deepseek-400 103,158,254 /
    border rgba(255,255,255,0.06) / bubble 44,44,46）。布局参考 Zed（顶栏标签 + 窗口控制同排、
    侧边栏 runtime + 会话树、底部图标栏）。
16. **渲染后端 = wgpu 默认，tiny-skia 作兜底**：tiny-skia 纯 CPU 光栅化在弱机/高缩放下
    **滚轮直接卡死**（实测），故 wgpu 为默认并保留 `fallback::Renderer<wgpu, tiny_skia>`
    （设备创建失败自动退回，不白屏）。实测常驻：wgpu + `DSHR_DPI=auto` 97 MB；
    wgpu + `WGPU_BACKEND=gl` 52 MB；tiny-skia 19 MB（但会卡）。
    `features` 由 `dshr-ui` 显式声明（根 `Cargo.toml` 关掉 iced 默认 features）。
    **附带约束（易踩）**：`iced-code-editor` 是路径依赖，cargo feature 是**并集**——
    该 crate 的 `Cargo.toml` 已打补丁显式关掉 iced 默认 features（原文件备份 `Cargo.toml.bak`）；
    **重新下载该仓库会让补丁失效**，判定命令 `cargo tree -p dshr-ui -i wgpu` 应输出 `nothing to print`。
17. **Windows DPI 不一致的兜底**：本机 `GetDpiForSystem()=96` 而显示器 192 DPI，此时 **winit
    自相矛盾**（窗口按 1:1 创建、却向 iced 报缩放 2.0），可见区域恒为画布的 `1/winit缩放`，
    任何 `scale_factor` 都无法自洽。修法：`main()` 里建窗口**之前**调
    `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE)`（`dpi.rs::make_consistent()`），
    仅当检测到显示器被缩放时启用；`DSHR_DPI=aware|unaware|auto` 可强制覆盖。
    代价：Windows 会按显示器 DPI 做位图拉伸（发糊）——**糊/切二选一**。
    附带修正：窗口尺寸必须写在 `window::Settings.size` 里（`.window_size()` 会被随后的
    `.window(..)` 覆盖）；位置用 `Position::Specific` 自行居中（`Position::Centered` 在 DPI
    不一致时算错）；顶栏三个窗口图标用文字字形（U+2212 / U+25A1 / U+2715），
    因为 `canvas` 自绘在 tiny-skia 后端下画布拿到 0 尺寸、完全不出图。
18. **Enter 发送**：iced 0.14 `text_editor` 把 Enter 发布为 `Action::Edit(Edit::Enter)`，
    插入换行是 App 收到 action 后 `content.perform()` 才执行的——`on_action` 里拦截它转 `Send`。
    **0.14 限制**：`Edit::Enter` 不携带 shift 信息，**Shift+Enter 也会发送**；多行文本用中间换行。
19. **覆盖式菜单自研（Popover）**：iced 无内置，`widgets/popover.rs` 用 `features=["advanced"]`
    自定义 `Widget::overlay`。三个硬教训：**viewport 必须传绝对坐标 `layout.bounds()`**
    （传 `Rectangle::with_size` 会让整个菜单被裁剪——"画了但看不见"）；锚点 =
    `layout.position() + translation`；菜单定位在宿主右下、偏移 +8（0 偏移会让第一项压在 ⋯
    正下方，点击 ⋯ 误触新建），右缘超出视口时左移钳制。
20. **配置页 Zed 化**：左分区导航（选中 accent 竖条）+ 右分组表单（节标题 + caption 说明行 +
    Zed 式输入框）；分区 = 通用/模型/运行时/API；保存仍显式按钮（config.json 非自动保存）。

---

## 10. 关键事实与坑

- **npm latest 陷阱**：`@deepseek-ai/dsh` 的 `latest` 长期不是最新、且不含 sdk profile。
  **锁版本是唯一安全路径**。
- **`reason: 'series'` 是生产事件**：官方 `packages/core/agent-loop/src/agent.ts` 的
  `Agent.buildRequest()` 在消息序列边界发出（goal 轮等场景必现）；严格枚举会整体解析失败
  （已由 `known()` 兜底 + 测试覆盖）。
- **会话 id 必须唯一**：复用固定 id 会撞上官方磁盘持久化日志，`turn/end` 报
  `session already has a persisted log on disk … (id collision)` error 回合。
  正式会话 id 一律唯一化（epoch 前缀）。
- **engine 在「一个 runtime 都没有」时必须阻塞在命令通道上**：`FuturesUnordered::next()`
  在**空集合**上立刻返回 `None`，若不特判就会让 `Engine::next()` 立即返回空批 →
  总线循环（`dshr-ui/src/bridge.rs`）空转烧掉一个核（界面上毫无异常）。
  实测覆盖：`dshr-state/tests/engine_flow.rs` 的 `no_runtime_waits_for_command`。
  触发条件常见：程序刚启动（还没 StartRuntime）、用户停掉最后一个 runtime。
- **子表外键要求父行先存在，而落库失败的信号非常弱**：`requests.session_id` 引用 `sessions(id)`，
  但**第一次 prompt 时该会话还没落过任何快照**（快照要等事件回来）→ 直接插请求行会
  `FOREIGN KEY constraint failed`；而 engine 的落库错误是「打一行 stderr 就继续」，
  于是在测试里表现为「表是空的」——与「表根本没有写入方」表面一模一样（本项目在 `runtime_logs`
  上踩过同类）。对策：`Store::ensure_session` 幂等补壳行（见 §8.2）。
- **审批/询问流在 SDK 通道是死的**：`ask_user_question` 无 provider 转发；通知面固定 4 种，
  审批要 runtime 侧 TS 插件转发 `ctx.approval`。
- **`web_fetch` 默认禁用**（SSRF 未防护），`web_search` 可用（60s 超时）。
- **会话日志 `.jsonl.zstd` = 多独立 Zstandard frames**（Node 只解第一帧，按 RFC 8878 切）；
  首行是 `SessionHeader` 非事件。SDK 若做历史直读要处理。
- **`SESSION_FORMAT_VERSION` 已为 4**（官方 0.1.7-rc.2 起），日志格式有结构性变更。
- **官方 exe 打包 Windows 是 non-goal**：Windows 上 runtime 必须有 node。
- **`examples/jsonrpc-agent` 已删**：角色由 `dsh --profile sdk`（dsh-base + dsh-sdk-app）取代。
- **官方 SDK 生态**：ACP（`dsh --profile acp`，Zed 用）与 Claude Code/Codex hooks 是另外两条接入线。
- **iced 0.14 坑**：无 `theme::Button/Container` 枚举（用 `button::primary/secondary` + 闭包样式）；
  `Padding` 无 `[f32;4]` From；Tree 非 Clone；`Element::draw/update` 需 viewport 参数；
  无内置 popover；无边框窗口用 `Window::Settings{decorations:false}` +
  `iced::window::{close,maximize,minimize,drag}`，主窗口 id 用 `window::open_events()` 订阅捕获
  （无 MAIN 常量，首个 `Id::unique()` 即主窗口）。
  **改动前查 `iced_widget-0.14.2` 源码**（registry 路径），以官方 widget 实现为准。

---

## 11. 代码规范

1. **官方引用必须钉到具体文件 + 类/方法/函数**（行号可加分），例如
   `packages/core/session/src/types.ts 的 SessionEventMap['turn/end']`、
   `packages/llm/llm/src/types.ts 的 ImageBlock.offloaded`。
2. **行数约束**：单文件平均 ≤350 行；超了拆文件。
3. **注释三级制**（用户要求）：
   - **文件级**：这个文件干什么、主要用途、为什么需要、**上接**谁、**下接**谁。
   - **函数 / trait 级**：为什么需要它、输入输出是什么、主要功能是什么。
   - 保留既有「官方对应」钉法。
4. **测试为硬约束**：协议改动无回归测试不合并。
5. **文件风格**：Rust 2018+（`x.rs` + `x/`，**不用 `mod.rs`**）。
6. **提交前跑**：`cargo test --workspace`、`cargo fmt --all -- --check`。
   注意 PowerShell 会把 cargo 的 stderr 误判为失败（见 AI-LOG §2.3）。

---

## 12. 待办与里程碑

### 12.1 里程碑

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 dsh-sdk-protocol | 全部类型 + fallback + 帧层 | **完成**（同步 0.1.7-rc.2，59 事件全集） |
| M1 dsh-sdk-client | HarnessClient + spawn + dispose + smoke | **完成** |
| M2 API 对齐 | run / 订阅 / 会话树 / 图片 | **完成**（剩发布） |
| M2.5 dshr-state 重建 | 配置/记录/runtime/全链路（真实 runtime 跑通） | **完成** |
| M3.5 UI 骨架 | 三页 + 侧边栏树 + 对话 + 覆盖菜单 + 窗口控制 | **完成** |
| M3.6 UI 接真实数据 | bridge 接 state（真 runtime + 真记录 + 落库） | **完成** |
| M3.7 配置页 Zed 化 | 分区导航 + 分组表单 | **完成** |
| **S1 state 分层** | raw/engine/fold 三层（M1 改名、M1.5 store 拆分、M2 SessionDriver、M3 新 engine 已完成） | **进行中**（M4 部分完成；**M6 数据面完成**；M5 UI 多 runtime、M6 的轻通知切换 待做） |
| **S3 契约测试重建** | 按「各 crate 一个 `tests/`」重建（§12.4） | **进行中**（3/5 项完成：state × 4 文件 + protocol × 2 文件，45 条绿） |
| **S4 落盘数据做细 + 可达** | 逐条对话落库（除逐 chunk）、失败原因、CSV 导出、**关掉再打开可复原** | **数据侧完成**（`messages` 表 + `export.rs` + `load_snapshot` + `requests` 写入方；UI 侧待 M5） |
| S2 数据管道完善 | stderr/退出落盘、多会话路由、B 方案读取 | 未开始 |
| M4 发布 | crate 打包 + README + 生态目录 | 未开始（用户暂缓） |
| M3.8 监控页 | §8.3 read 聚合 + 页面 | 未开始 |

### 12.2 state 分层实施步骤（每步可编译、可测）

| 步 | 内容 | 验证 |
|---|---|---|
| **M1 ✅** | 旧 `engine` → `raw`（`Engine` → `Runtime`）；`mod.rs` 全消除；`cargo fmt` 独立落地 | 62 测试通过、fmt 合规、事件 59=59 |
| **M1.5 ✅** | 拆 `store.rs`（844 行）→ 门面 + `store/{error,schema,convert,write,tests}` | 62 通过 / 0 warning |
| **M2 ✅** | 引入 `SessionDriver` trait + `HarnessClient` 实现；raw 改经 trait 调用 | 纯重构、无行为变化；新增「驱动注入」测试证明可替换（63 通过） |
| **M3 ✅** | 新 `engine.rs`：runtime 注册表 + 每会话态；搬入脏检测/落库；**stderr 与退出落盘接通**（`runtime_logs` / `runtimes` 表首次有写入方）；`record::Recorder` 重新接入（app 轨迹 + 线级记录同源） | 5 条 engine 集成测试（假 driver，不起进程）：主链路/会话重置/多 runtime 路由隔离/退出上报/stderr 落库（67 通过） |
| M4 | raw 只留进程与协议 → 改发 wire 级事件批（带标） | 部分已完成：`raw.rs` 已是纯进程句柄（442 行）、`session.rs` 已删除、WireLog 路径已归 engine 管理 |
| M5 | UI bridge 换到新 `EngineCmd`/`EngineEvent`（加 runtime/session 标） | UI 手验 |
| M6 | 快照读取改 B 方案（拉 + 变更通知） | **数据面 ✅**：`ReadSnapshot`/`SessionLoaded`/`ListSessions`/`Sessions` + 3 条 engine 单测（运行中拉取 / 库里复原 / 目录）。**轻通知切换待 M5**（见 §7.2） |

### 12.3 其它待办

| 项 | 状态 |
|---|---|
| README 双语 + 发布准备 | 未做（发布等 SDK 全做完 + 测试重建完） |
| portable node 自动安装 | 未做（当前 node 缺失时报清晰错误） |

### 12.4 测试策略（2026-09-29 调整）

**现状（2026-09-29 更新）：重建进行中，已落地 45 条契约测试**（`cargo test --workspace` 全绿）：

| crate | 文件 | 条数 | 覆盖 |
|---|---|---|---|
| `dsh-sdk-protocol` | `tests/event_catalog.rs` | 5 | 事件全集对账（对锁定快照）+ 降级识别（`degraded_event`）+ merge-extensible 兜底 |
| `dsh-sdk-protocol` | `tests/frame_shape.rs` | 6 | **真实录制帧**的形状对账 + 20 种消息来源建模（含漂移容忍）+ 内容块 roundtrip + 请求面/信封 |
| `dshr-state` | `tests/engine_flow.rs` | 11 | engine 主链路、多 runtime 路由隔离、stderr/退村落盘、空注册表不空转、降级写 app 轨迹、请求事实与失败可见、**按需拉取快照 / 库里复原 / 会话目录** |
| `dshr-state` | `tests/fold_projection.rs` | 10 | 投影语义（消息序/工具配对/token/轮结算/错误口径）+ 在线与回放同源同巡 + 模型请求可见 + **行级 turn/step/source/error 与注入/尝试成行** |
| `dshr-state` | `tests/store_persistence.rs` | 6 | 落库幂等、替换语义、空 session_id 拒绝、多会话隔离、**复原往返（逐字段相等）**、缺失会话为 None |
| `dshr-state` | `tests/export_csv.rs` | 5 | CSV 转义、八张表导出、跨会话回放分组、全量导出落盘 + 复原抽查（`DSHR_EXPORT=1` 真跑） |
| `dshr-state` | `tests/engine_session.rs` | 2 | 真实会话逐步透明账本（`DSHR_LIVE=1`）+ 冷启动负例 |

`dsh-sdk-client`（帧层/配对/超时，需 node fixture）与 `dshr-ui`（纯映射函数）**尚未重建**。
下面保留「为什么当初清空」的原始记录与重建方式。

#### 为什么清空

旧布局是**按文件散落**的：一部分内联在 `src/*.rs`（`#[cfg(test)] mod tests`），
一部分是 `src/<模块>/tests.rs`（`fold` / `store` / `engine` / `raw` 四处）。
三个问题：

1. **测试与实现抢同一文件的行数预算**。本项目对单文件行数有约束（≤350 行为宜），
   内联测试把已经接近上限的文件顶破；重构时被迫在「拆实现」与「拆测试」之间二选一。
2. **改实现会顺手改到测试**，两者在同一文件的 diff 里纠缠，review 分不清
   「行为真变了」还是「断言跟着改了」。
3. **契约测试放错地方**。「wire 帧能否解析」「落库是否幂等」这类**对外契约**，
   用集成测试（`tests/`）表达才正确——它们只依赖 `pub` 接口，从而在重构内部结构时
   保持不动。

#### 新约定

| 测试类型 | 放哪 | 依赖什么 |
|---|---|---|
| **契约 / 集成**（对外行为、跨模块链路） | `<crate>/tests/<关注点>.rs` | 只用 `pub` 接口 |
| **纯内部单元**（私有函数边界） | 允许内联 `#[cfg(test)] mod tests`，**仅当**该文件离行数上限还有余量 | 可用私有项 |

**不再**新建 `src/<模块>/tests.rs`。每个 crate 的 `tests/` 目录里有 `_conventions.md`
（下划线前缀不会被 cargo 当作测试目标），写明该测什么与该 crate 的注意事项。

#### 已预留的测试接缝（**无门控的正式 API**）

`dshr-state` 保留 11 处测试接缝。2026-09-29 按用户要求**取消 `#[cfg(test)]` 门控**：
「以后测试都是直接跑最终和 dsh 沟通的实际测试了，直接通过 test 块来模拟触发和接收就可以了」。

**设计含义**：它们不再是「测试专用代码」，而是**注入式测试基础设施**——正式 API 的一部分。
代价是生产二进制里也包含它们（几行、无副作用）；收益是测试与生产走**同一条代码路径**，
不存在「`cfg` 掉了才发现生产路径没编译」这类问题。
取消门控时顺带删掉了 `injected` 字段——它是**只写不读**的死字段，正是取消门控才暴露出来的。

| 接缝 | 作用 |
|---|---|
| `SessionDriver` trait | 把进程层抽象掉：假 driver 只回 `Ok(...)`，测试自持通知流发送端造事件 |
| `Runtime::{with_driver_for_test, inject_stderr_for_test, mark_exited_for_test, force_fake}` | 注入 driver/stderr、模拟退出、强制 Fake |
| `Engine::{register_injected_runtime[_with_stderr], with_db, store_ref, recorder_ref, with_recorder_for_test, mark_runtime_exited_for_test}` | 注册「已就绪」runtime、注入内存库/记录器、断言落库与 app 轨迹 |

#### 两类测试，别混

| 类型 | 起进程 | 烧 token | 何时跑 |
|---|---|---|---|
| **契约/集成**（注入假 driver） | 否 | 否 | 每次 `cargo test` |
| **真实会话**（与 dsh 沟通） | 是（官方 dsh） | **是** | 显式 `DSHR_LIVE=1`，低谷期 |

真实会话测试用环境变量门控 + 无条件 `return` 跳过（**不用** `#[ignore]`）：
没配密钥时自动跳过，配了就真跑，比 `#[ignore]` 更显式。

`dshr-state/tests/engine_session.rs` 是真实会话的**逐步透明账本**：它按
「Start → Started → Prompt → 等 idle → Stop」逐步打印当步的
「事件摘要 + sessions 汇总 + runtime_logs 行数 + wire-log 字节数 + 消息明细」。
为什么值得存在：本项目已经吃过一次教训——`runtimes` 与 `runtime_logs` 两张表建了却
长期**没有写入方**，从代码表面完全看不出来，只有查询才会暴露。
这个账本把「表里到底有没有东西」变成可执行的断言。

##### 运行真实会话测试的前提（实测记录，2026-09-29）

真实会话测试会触发 `runtime::ensure` → `pnpm install --force`，装**588 个包**
（含 `@deepseek-ai/dsh-*` 全家桶与 `sharp` / `sherpa-onnx` / `koffi` 的**全平台**二进制；
`@deepseek-ai/dsh` 本体只有 69 KB，但 81 个直接依赖会解析成这么多，且**没有一个是
optional**，无法裁剪）。

**实测结论：官方 registry 在弱网下装不完，必须走镜像。** 两次尝试都在 356/587 处
因 `registry.npmjs.org` 的 socket 超时（`error (23)`）中断（659s / 300s+）；
换 `registry.npmmirror.com` 后 **`Done in 56.6s`**（`reused 561` + 新下 26）。

因此 `runtime.rs` 做了三项加固（见该文件头「下载稳定性」）：

| 加固 | 做法 | 为什么有效 |
|---|---|---|
| **重试 + 指数退避** | 同 registry 最多 3 次，退避 5/10/20s | pnpm store 是 content-addressable 的：**重试即续传**，不是从头再来（实测 `reused 561`） |
| **`.npmrc` 提高取数韧性** | `fetch-retries=10`、`fetch-timeout=600000`、`network-concurrency=4`、`prefer-offline=true` | npm 默认 `fetch-retries=2`，对弱网太弱；降并发能减少被掐断 |
| **registry 回退链** | 官方 → `registry.npmmirror.com`，可用 `DSHR_NPM_REGISTRY` 环境变量或 `config.json` 的 `npm-registry` **覆盖为只走指定源**（内网代理场景） | 镜像已镜像 `@deepseek-ai` scope（实测 dist-tags 与官方一致） |

**不做打包归档**：用户明确要求「不在仓库里打包一个，还是通过 pnpm 下载构建」——
所以不引入"预热归档 + 校验 + 解压"那条路。

#### 等待模型期间没有反馈（数据侧 ✅ 已做 / UI 侧待做）

M3 实测暴露的产品问题：engine 只在**状态有变化**时发事件。发完 prompt 后直到 runtime
返回首条事件之前，`engine.next()` 安静等待——真实 LLM 首字节延迟可达几十秒，
UI 上表现为「点了发送之后一片静止」，用户无法区分「在等模型」与「卡死了」。

**根因（不是缺定时器）**：`request/header` 与 `request/context` 此前都折成 `{}`，
折完快照没变化 → 脏检测判定「无变化」→ **UI 根本收不到通知**。

**数据侧修法（2026-09-29 已落地）**：两个事件折进 `SessionSnapshot.last_request`
（起点时刻 / seq / 追加原因 / 工具数 + provider / model / 上下文窗口）。
语义上「等待」= 已发起请求（`started_at` 有值）且尚无对应产出 —— UI 用
`last_request.seq` 与最后一条 assistant 行的 seq 比较即可判定，`started_at` 用来显示
「已等待 N 秒」。测试见 `tests/fold_projection.rs::model_request_is_visible_in_snapshot`。

**UI 侧待做**：状态栏显示「正在等待 <model> · Ns」（属 M5 那一轮）。
**注意区分两类等待**：等本地事件（毫秒级）与等真实 API（可达分钟级），
测试里的单步超时必须分开设（本项目在这里误判过两次）。

#### 三条踩过的坑（重建前先读）

1. **`assistant/message` 帧有必填字段**：`data` 需 `turn` / `step` / `message`
   （含 `id`/`role`/`content`/`source`），且 `source.kind = "model"` **必须带
   `provider` 与 `model`**。漏字段**不报错**——`known()` 反序列化失败会静默降级成
   `Unknown`，测试只表现为「事件没反应、等超时」。**必须保留一条帧形状自检测试**。
2. **广播通道的生命周期**：假 driver 持有自己的发送端克隆，测试 drop 手里的发送端
   **关不掉**流。模拟「进程退出」要用 `mark_exited_for_test`，不要靠 drop。
3. **一条测试只验一件事**：测试助手的推进循环有副作用（可能让 engine `teardown`
   并移除 runtime），与后续断言写在同一条测试里会互相吃掉状态。

#### 重建的优先级（建议）

1. ✅ `dshr-state`：engine 主链路（prompt→通知→折叠→落库）、多 runtime 路由隔离、
   stderr/退村落盘 —— 这些是**当前最复杂、最不可见**的逻辑（`tests/engine_flow.rs`）；
2. ✅ `dsh-sdk-protocol`：事件全集对账 + 形状解析（协议漂移是头号风险）
  （`tests/event_catalog.rs` + `tests/frame_shape.rs`）；
3. ✅ `dshr-state`：fold 投影语义、store 幂等（`tests/fold_projection.rs` + `tests/store_persistence.rs`）；
4. ⬜ `dsh-sdk-client`：帧层/配对/超时（node fixture，不烧 token）；
5. ⬜ `dshr-ui`：纯映射函数（**不要**在测试里起窗口）。

**重建的意外收获**（这三条都是「不写测试就看不见」的问题，详见 AI-LOG §2.6/§2.1）：

- `Engine::next()` 在注册表为空时立即返回空批 → 总线空转烧一个核（已修）；
- `MessageSource` 漏移植 `system-prompt` / `runtime-context` 两个官方 0.1.7-rc.2 就有的 kind →
  **每条** `system/message` 与注入类 `user/message` 整条降级 `Unknown`（已修，并由真实帧对账守住）；
- 顺着上一条做全面取证，又补了 5 个真实可达的 kind（`plan-mode` / `model-selection` /
  `user-approval` / `ptc-mode` / `compact-checkpoint`），并把「降级」变成**可识别、可汇总**
  （`Unknown.degraded` + engine 的 `event.degraded` 轨迹 + `scripts/scan-message-sources.mjs`）。

---

## 13. 风险

- **R1 协议漂移**：0.1.x 无兼容承诺 → `Unknown`/`known()` 双层 lossless 兜底 + 锁 runtime 版本；
  同步前跑 `scripts/compare-session-events.mjs` + 上游标签 diff。
- **R2 官方 TS client 永远先行**：新能力（图片等）先到 TS/Python → Rust 侧按需追。
- **R3 测试基线**：协议改动必须带测试；UI 层目前只有少量单测，
  **前端到 state 的数据流没有自动化验证**（需要真实 runtime 全链路，消耗 token）。
- **R4 Iced 0.14 前沿 API**：`Edit::Enter` 无 shift、Tree 非 Clone、无 popover 等——
  改动前查 `iced_widget-0.14.2` 源码。
- **R5 `iced-code-editor` 路径依赖补丁易丢**：重新下载该仓库会带回 wgpu 依赖树
  （判定命令见 §9.16）。
