# DSH Rust Desktop — 设计草案（v0，待评审）

> 本文是施工蓝图，不是决策记录。决策见 `HANDOFF.md`。评审通过后再开 Cargo 骨架。
> 依据：`packages/sdk/protocol`（wire 协议）、`packages/core/session`（SessionEvent）、`packages/llm`（ContentBlock）、`packages/subagent`（StopReason）、`packages/sdk/client`（参考实现）。

## 1. 定位（一句话）

完整聊天客户端 + 内建监管底座，同源 `session.event`；Rust/Iced 自绘，不套 WebView，不 fork 官方仓库。

## 2. 架构总览

```
用户双击 dshr.exe（只启动这一个）
  └─ dshr-ui（Iced，宿主 UI 进程）
       │  spawn（stdio 管道）
       └─▶ dsh-jsonrpc-agent（runtime sidecar 子进程）
             ├─ 阶段 A：node .../packaged-bin.js   （开发态）
             └─ 阶段 B：dsh-jsonrpc-agent-pkg-win-x64.exe（发布态）

数据流（单向清晰）：
  dshr ──stdin──▶ runtime    initialize / session/prompt / shutdown
  dshr ◀─stdout── runtime    session.event / session.status / subagent.*
```

- **会话内**：dshr 是传话筒 + 观察者，工具调用/执行 100% 在 runtime。
- **会话外**：dshr 是宿主，管进程生命周期 + 插件安装（改 cordis.yml + 装包 + 重启）。
- 传输 = 换行分隔 JSON-RPC 2.0，每行一个完整 JSON 对象。

## 3. 仓库布局

```
dshr/
├── Cargo.toml                # workspace
├── crates/
│   ├── dshr-protocol/        # ① 纯协议 port：类型 + transport，无 I/O 副作用
│   │   └── src/
│   │       ├── rpc.rs            # JSON-RPC 帧（Request/Response/Notification）
│   │       ├── transport.rs      # 换行分隔 transport（≈ transport.ts，~200 行）
│   │       ├── requests.rs       # initialize / session/prompt / shutdown
│   │       ├── notifications.rs  # 4 个 server→client 通知
│   │       ├── session_event.rs  # SessionEvent（13 事件类型判别联合）
│   │       ├── content_block.rs  # ContentBlock（5 变体判别联合）
│   │       ├── message.rs        # User/Assistant/ToolResultMessage + StreamChunk
│   │       └── llm.rs            # TokenUsage / FinishReason / EpochHeader 等
│   ├── dshr-runtime/         # ② 子进程管理 + HarnessClient（依赖 tokio）
│   │   └── src/
│   │       ├── client.rs         # HarnessClient（≈ client.ts 的 Rust 版）
│   │       ├── spawn.rs          # 定位 runtime：exe 优先，node 显式回退
│   │       ├── dispose.rs        # EOF→SIGTERM→SIGKILL 阶梯 + 崩溃恢复
│   │       └── subscriptions.rs  # 通知订阅/过滤（含 session 树血缘）
│   ├── dshr-state/           # ③ 应用状态：会话树 + event 缓存 + 监管聚合（纯逻辑）
│   │   └── src/
│   │       ├── session_tree.rs   # subagent.started 血缘 → 会话父子关系
│   │       ├── event_log.rs      # 内存 event 序列（seq 单调）
│   │       ├── log_reader.rs     # 历史日志直读：zstd frame 切分 + packed row 解码 + header 识别
│   │       └── accounting.rs     # token 聚合 + 离线算账（token × 定价表）
│   └── dshr-ui/              # ④ Iced UI（bin target）
│       └── src/
│           ├── main.rs           # 入口
│           ├── app.rs            # Elm Model/Message/update/view
│           ├── bridge.rs         # tokio runtime ↔ Iced Subscription 桥接
│           ├── theme.rs
│           ├── views/
│           │   ├── chat.rs       # 聊天视图（流式）
│           │   ├── workspace.rs  # 工作区/会话列表
│           │   ├── supervisor.rs # 监管面板（token/工具/时序）
│           │   └── settings.rs   # provider/model/runtime 路径/API key
│           └── widgets/
│               ├── message_list.rs  # 消息列表（虚拟化，见风险 R1）
│               └── tool_call.rs     # 工具调用折叠卡片
├── runtime-manifest.json     # runtime 版本 pin + 获取方式（阶段 B 用）
├── scripts/fetch-runtime.ps1 # 阶段 B：下载官方 wheel 或本地构建 exe
└── .github/workflows/release.yml  # 阶段 B：CI 产 exe + NOTICE + 安装包
```

**依赖方向**：`dshr-ui → dshr-state → dshr-runtime → dshr-protocol`（protocol 是地基，零依赖）。

## 4. 协议 port 清单（阶段①评估结论）

### 4.1 wire 面（很小，已确认）

| 方向 | 方法 | 参数/结果 |
|---|---|---|
| C→S | `initialize` | cwd/provider/model/maxTokens? → serverInfo |
| C→S | `session/prompt` | sessionId + contentBlocks → messageId |
| C→S | `shutdown` | — → {} |
| S→C | `session.event` | sessionId + event |
| S→C | `session.status` | sessionId + idle/running |
| S→C | `subagent.started` | parentSessionId + childSessionId |
| S→C | `subagent.finished` | provider/agentId/parent/child/status/stopReason/lastAssistantMessage? |

transport ≈ `transport.ts` 逻辑，Rust 里 ~200 行，用 `tokio::io::BufReader` 逐行 + `serde_json`。

### 4.2 富类型（port 工作量主体，已查实）

**`SessionEvent`**（`packages/core/session`，13 事件类型的判别联合，每个带 `type/seq/time/data` + 可选 `ignorable/sourceEventSeqs/surfaceOp`）：

`turn/start` `turn/end` `step/start` `step/end` `user/message` `assistant/chunk` `assistant/message` `tool/call` `tool/result` `todo/write` `request/header` `request/context` `session/end-seed`

**`ContentBlock`**（`packages/llm`，5 变体）：`text` `reasoning` `image` `tool-call` `tool-result`

**`SubagentStopReason`**：5 个字面量 `completed/aborted/error/max-tokens/refusal`

**隐藏工作量（辅助类型）**：`UserMessage/AssistantMessage/ToolResultMessage` + `StreamChunk` + `TokenUsage`（usage 明细，监管核心）+ `FinishReason` + `EpochHeader/RequestHeaderReason/RequestContext` + `TurnEndReason` + `TodoItem` + branded id（`CallId/SessionId`）+ `JsonValue`（tool/result 的 opaque meta）。

### 4.3 port 关键决策（两个坑，必须先定）

1. **判别联合用 serde 的 `#[serde(tag = "type")]`**：`SessionEvent` 和 `ContentBlock` 都是 `type` 字段打标的 discriminated union，Rust 用内部 tagged enum 天然表达。
2. **merge-extensible 必须宽容**：TS 里 `SessionEventMap` 和 `ContentBlockMap` 都是 **plugin 可扩展**的（工具插件能注册新事件类型/新 content block 类型）。Rust port 不能只硬编码 13+5 种，必须留 **unknown fallback variant**（`#[serde(other)]` + 保留原始 JSON），否则 runtime 装了某个产生新事件类型的插件时，dshr 解析 `session.event` 直接崩。**这是 port 最容易翻车的地方。**

### 4.4 监管面板三视图（数据主权核心）

同一份 `session.event` 流的三个切片，零额外采集成本：

| 视图 | 数据源 | 渲染 |
|---|---|---|
| **token 明细** | `assistant/message.usage` + `request/header` | 每轮 input/output/cache/reasoning token 计数 + 离线算账（token × 定价表） |
| **命令执行视图** | `tool/call`(name=bash/pwsh, arguments=命令) + `tool/result`(输出) | 终端样式只读面板，**非交互 PTY** |
| **文件 diff 视图** | `tool/result.meta.diffs` = `[{path, oldText, newText}]`（官方已算好的 result-time contextual diff，持久化在 log） | Rust `similar` crate 渲染行级红删/绿增 |

**文件 diff 视图关键事实（已查证 `packages/fs/tool-fs`）**：
- `edit`/`write` 工具在 `tool/result` 的 `meta` 放 `{ diffs: [...] }`，随 `session.event` 直接发到 dshr，是**已算好的 before/after 上下文 hunk**，不是让 dshr 自己拼。
- 边界：① 只覆盖 edit/write 工具（bash/pwsh 直接改文件无 diff）；② 是变化点附近上下文，非全文件；③ `oldText` 可能为 null（新建/整文件替换）。
- 渲染用 `similar::TextDiff`（unified + inline 两种视图）。

### 4.5 会话数据双通道（dshr 的差异化地基）

dshr 读会话有**两条互不依赖的通道**：

| 通道 | 时机 | 数据 | 用途 |
|---|---|---|---|
| **实时 SDK 流** | runtime 运行时 | `session.event` 通知流（新增事件） | 聊天渲染、实时监管 |
| **历史直读磁盘** | runtime 未运行时 | 直接读 `<DSH_SESSION_ROOT>/<project>/<session>/session.jsonl.zstd` | 离线算账、备份、跨 runtime 浏览 |

**历史直读的 3 个坑（已查证 `session-persistence-jsonl`）**：
1. **zstd 多 frame**：header 帧 + 每批 append 一帧，Node 只解第一帧 → 按 RFC 8878 切边界逐个解（复用最早的转换脚本逻辑）。
2. **packed chunk row**：`packChunks` 默认 true，连续 `assistant/chunk` 打包成一条 row（seq/time 增量），需 `decodeStorageRecord` 还原。
3. **首行是 SessionHeader**：第一逻辑行 `{type:'session', id, cwd, ...}`，非 event，读时要识别跳过。

**结论**：实时走 SDK，历史直接读文件，dshr 因此比 web 端多一层"数据真正在自己手里"（离线可算账、可备份、可浏览全部历史）。

## 5. Rust 技术选型（初定，待验证）

| 项 | 选择 | 理由 |
|---|---|---|
| 异步 | tokio | stdio 读是异步流；Iced subscription 也是 async |
| 序列化 | serde + serde_json | 协议是 JSON |
| UI 桥接 | tokio runtime 跑在独立线程，`mpsc::channel` → Iced `Subscription` | 事件流 → Message 的天然通道 |
| 进程管理 | `tokio::process` | spawn/kill/EOF 阶梯 |
| diff 渲染 | `similar`（TextDiff） | 文件 diff 视图，unified + inline |

## 6. 两阶段里程碑

### 阶段 A — 开发态（node carrier，目标：全链路跑通）

- **M0** `dshr-protocol`：transport + 全部类型 port（含 unknown fallback）+ serde 往返测试。
- **M1** `dshr-runtime`：HarnessClient + spawn + dispose 阶梯 + `session/prompt` smoke（headless 打印 event 流）。
- **M2** `dshr-ui` 骨架：Iced app + 聊天视图 + 流式渲染（先测 R1/R2 两原型）。
- **M3** 工作区/文件树/会话树 + 监管面板三视图（token 明细 / 命令执行视图 / 文件 diff 视图）。

### 阶段 B — 发布态（单文件 exe，目标：免 Node 分发）

- **M4** fork 官方 `scripts/build-exe-for-python-sdk.ts` 补 Windows 分支，产 `dsh-jsonrpc-agent-pkg-win-x64.exe`。
- **M5** release pipeline：CI 构建 exe + 抓第三方 NOTICE → 打进安装包 → Release asset（仓库不 commit exe）。

## 7. 风险与待验证项

- **R1 长列表虚拟化**：Iced `Scrollable` 不自动虚拟化，5000 条消息需 `widget::lazy` 或第三方方案，否则滚动卡。→ 原型必测。
- **R2 流式文本吞吐**：每 token 一次 update 的渲染吞吐要实测（20 token/s 基准）。→ 原型必测。
- **R3 Iced 版本 API 漂移**：0.14 较新，第三方 `iced_aw` 等生态跟进可能滞后；核心控件只用内置，降低风险。
- **R4 unknown event 宽容**：见 4.3-2，不处理则 runtime 加插件即崩。
- **R5 审批流缺失**：`ask_user_question` dead，MVP 配 `approval: never`，交互审批留到阶段②协议扩展。

## 8. 评审要点（需拍板）

1. crate 划分（4 crate / 更少？）是否认可。
2. 阶段 A 里程碑 M0→M3 的顺序是否认可，是否要先做 M0+M1 的 headless smoke 再上 UI。
3. R1/R2 两个原型是否并入 M2 一起做，还是单独先做（对应 handoff 决策 3 的"待办：写两个原型对比"）。
