# DSH Rust Desktop — 设计文档（v1，单一信息源）

> 本文合并自原 `DESIGN.md`（v0 设计草案）+ `HANDOFF.md`（决策与记忆交接），为全项目唯一文档。
> 官方参考仓库：`D:\DeepseekHarness\deepseek-harness`（本地 clone，源码是唯一权威，本文是施工蓝图）。
> 依据：`packages/sdk/protocol`（wire 协议）、`packages/core/session`（SessionEvent）、`packages/llm`（ContentBlock）、`packages/subagent`（SubagentStopReason）、`packages/sdk/client`（参考客户端）、`packages/sdk/server`（runtime 侧对接层）。

## 1. 定位（一句话）

完整聊天客户端 + 内建监管底座，同源 `session.event`；Rust/Iced 自绘，不套 WebView，不 fork 官方仓库。

## 2. 架构总览

```
用户双击 dshr.exe（只启动这一个）
  └─ dshr-ui（Iced，宿主 UI 进程）
       │  spawn（stdio 管道）
       └─▶ dsh-jsonrpc-agent（runtime sidecar 子进程）
             ├─ 阶段 A：node <npm 包>/lib/bin.js + cordis.yml   （开发态/Node 路线）
             └─ 阶段 B：dsh-jsonrpc-agent-pkg-win-x64.exe        （发布态，免 Node）

数据流（单向清晰）：
  dshr ──stdin──▶ runtime    initialize / session/prompt / shutdown
  dshr ◀─stdout── runtime    session.event / session.status / subagent.*
```

- **会话内**：dshr 是传话筒 + 观察者，工具调用/执行 100% 在 runtime。
- **会话外**：dshr 是宿主，管进程生命周期 + 插件安装（改 cordis.yml + 装包 + 重启）。
- 传输 = 换行分隔 JSON-RPC 2.0，每行一个完整 JSON 对象。
- **分层认知**：runtime 进程 = 官方对接层（`sdk/server`，收到请求→switch 分发→调 agent→回响应/发通知）+ 内部逻辑（agent 循环/工具/LLM）的黑盒；dshr 只写"对面那一半"——类型契约 + 传输。

## 3. 仓库布局（最新：4 crate 平铺）

```
dshr/
├── Cargo.toml                # workspace（members 平铺在根目录）
├── dshr-protocol/            # ① 协议：类型 + 帧判断（纯逻辑，零依赖）
│   └── src/
│       ├── lib.rs                # pub mod 汇总
│       ├── content_block.rs      # 工具池 1：ContentBlock（5 变体判别联合 + fallback）
│       ├── session_event.rs      # 工具池 2：SessionEvent 信封 + 判别枚举（Unknown fallback）
│       ├── session_event/        # 事件 data 按事件族拆子模块（45 种事件）
│       │   ├── turn.rs           # turn/start、turn/end、step/start、step/end + TurnEndReason
│       │   ├── message.rs        # user/message、assistant/chunk、assistant/message
│       │   ├── tool.rs           # tool/call、tool/result + tool-workflow/*
│       │   ├── request.rs        # request/header、request/context
│       │   ├── session.rs        # session/end-seed、session/title、agent/*、subagent/descriptor
│       │   ├── approval.rs       # approval/*、permission/preset
│       │   ├── compaction.rs     # compaction/*
│       │   └── misc.rs           # todo/write、command/*、hook/*、llm/retry 等其余
│       └── subagent.rs           # 工具池 3：SubagentStopReason（5 字面量）
│       # 规划中：rpc.rs（帧类型 + 帧判断）、requests.rs（方向 1）、notifications.rs（方向 2）、
│       #         message.rs / llm.rs（UserMessage/TokenUsage 等，被 session_event 引用）
├── dshr-runtime/             # ② 子进程管理 + HarnessClient（依赖 tokio）
│   ├── src/
│   │   ├── lib.rs
│   │   └── client.rs             # HarnessClient（spawn/initialize/prompt/shutdown 已落地）
│   └── tests/
│       └── init_smoke.rs         # spawn → initialize → session/prompt → shutdown
├── dshr-state/               # ③ 应用状态：会话树 + event 缓存 + 监管聚合（纯逻辑）
│   └── src/
│       ├── session_tree.rs   # subagent.started 血缘 → 会话父子关系
│       ├── event_log.rs      # 内存 event 序列（seq 单调）
│       ├── log_reader.rs     # 历史日志直读：zstd frame 切分 + packed row 解码 + header 识别
│       └── accounting.rs     # token 聚合 + 离线算账（token × 定价表）
├── dshr-ui/                  # ④ Iced UI（bin target）
│   └── src/
│       ├── main.rs           # 入口
│       ├── app.rs            # Elm Model/Message/update/view
│       ├── bridge.rs         # tokio runtime ↔ Iced Subscription 桥接
│       ├── theme.rs
│       ├── views/
│       │   ├── chat.rs       # 聊天视图（流式）
│       │   ├── workspace.rs  # 工作区/会话列表
│       │   ├── supervisor.rs # 监管面板（token/工具/时序）
│       │   └── settings.rs   # provider/model/runtime 路径/API key
│       └── widgets/
│           ├── message_list.rs  # 消息列表（虚拟化，见风险 R1）
│           └── tool_call.rs     # 工具调用折叠卡片
├── runtime-manifest.json     # runtime 版本 pin + 获取方式（阶段 B 用）
├── scripts/fetch-runtime.ps1 # 阶段 B：下载官方 wheel 或本地构建 exe
└── .github/workflows/release.yml  # 阶段 B：CI 产 exe + NOTICE + 安装包
```

> 注：`spawn.rs`（exe 优先/node 回退定位）、`dispose.rs`（EOF→SIGTERM→SIGKILL 阶梯）、`subscriptions.rs`（通知订阅/过滤）为规划中的模块；当前 `client.rs` 已含 spawn/initialize/prompt/shutdown，transport 读循环逻辑暂内联在 `prompt()`。

## 4. 协议：wire 契约与类型清单

### 4.1 wire 方法面（7 种消息，双向）

**请求侧（client → server，3 个）—— 你通过 stdin 发的每个请求，`method` 必须是这三个之一：**

| method | params 类型 | result 类型 |
|---|---|---|
| `initialize` | `InitializeParams` | `InitializeResult` |
| `session/prompt` | `SessionPromptParams` | `SessionPromptResult` |
| `shutdown` | 无 params | 空对象 `{}` |

**通知侧（server → client，4 个）—— dsh 主动发的，你没有这个方向：**

| method | 类型 | payload 形状 |
|---|---|---|
| `session.event` | `SessionEventNotification` | `{ sessionId, event: SessionEvent }` |
| `session.status` | `SessionStatusNotification` | `{ sessionId, status: 'idle' \| 'running' }` |
| `subagent.started` | `SubagentStartedNotification` | `{ parentSessionId, childSessionId }` |
| `subagent.finished` | `SubagentFinishedNotification` | `{ provider, agentId, parentSessionId, childSessionId, status: SdkRunStatus, stopReason: SubagentStopReason, lastAssistantMessage?: ContentBlock[] }` |

**方向判定规则**：请求带 `id`（配对响应）；通知无 `id`、只有 `method`。client 的读循环按此区分——`有 id → 响应配对`；`无 id 有 method → 通知`。

### 4.2 类型清单（dshr-protocol 目标全集）

来源：`packages/sdk/protocol/src/types.ts` 的两个 Map（`HarnessSdkRequestMap` / `HarnessSdkNotificationMap`）就是类型清单；顶部 import 是领域类型的门牌号（顺着 import 跳进领域包）。

**请求侧类型（你发的）**：

| 类型 | 字段 |
|---|---|
| `InitializeParams` | `cwd: string` `provider: string` `model: string` `maxTokens?: number` |
| `InitializeResult` | `serverInfo: { name: string, version: string }` |
| `SessionPromptParams` | `sessionId: string` `contentBlocks: ContentBlock[]`（未知 sessionId 懒创建 session） |
| `SessionPromptResult` | `messageId: string` |

**通知侧类型（dsh 发的）**：

| 类型 | 字段 |
|---|---|
| `SessionEventNotification` | `sessionId: string` `event: SessionEvent` |
| `SessionStatusNotification` | `sessionId: string` `status: 'idle' \| 'running'` |
| `SubagentStartedNotification` | `parentSessionId: string` `childSessionId: string` |
| `SubagentFinishedNotification` | `provider` `agentId` `parentSessionId` `childSessionId` `status: SdkRunStatus('ok'\|'error')` `stopReason: SubagentStopReason` `lastAssistantMessage?: ContentBlock[]` |

**领域类型 3 组**（被上面引用，import 指向）：

| 组 | 位置 | 内容 |
|---|---|---|
| `ContentBlock` | `packages/llm/llm/src/types.ts` | 5 变体：text / reasoning / image / tool-call / tool-result |
| `SessionEvent` | `packages/core/session/src/types.ts` | 信封 + 45 种事件（见 4.3） |
| `SubagentStopReason` | `packages/subagent/subagent/src/types.ts` | 5 字面量：completed / aborted / error / max-tokens / refusal |

### 4.3 富类型详情

**`SessionEvent` 信封**（每个事件共有）：`type`（判别）、`seq`（会话内单调）、`time`（epoch 毫秒）、`data`（按 type 变）+ 可选 `ignorable` / `sourceEventSeqs` / `surfaceOp`（surface 事件）。

**事件清单（官方 `known-event-types.ts` 已声明 45 种，merge-extensible）**。核心 13 种（`SessionEventMap`）：

`turn/start` `turn/end` `step/start` `step/end` `user/message` `assistant/chunk` `assistant/message` `tool/call` `tool/result` `todo/write` `request/header` `request/context` `session/end-seed`

扩展示例（实测已见 `agent/inbox/spliced`）：`tool-workflow/run-start` `tool-workflow/run-end` `approval/asked` `approval/decided` `compaction/start` `compaction/end` `hook/invoked` `command/run` `command/done` `session/title` `goal/change` `plan/mode` `feedback/record` `schedule/change` `llm/retry` `sandbox/mode` `subagent/descriptor` `agent-preset/selected` `permission/preset` `web/deepseek-search-llm-request` …

**`ContentBlock`**（5 变体）：`text`（text）`reasoning`（text）`image`（attachment: ImageAttachmentRef）`tool-call`（id/name/arguments）`tool-result`（toolCallId/content/isError?）。

**隐藏工作量（辅助类型）**：`UserMessage/AssistantMessage/ToolResultMessage` + `MessageSource` + `StreamChunk` + `TokenUsage`（usage 明细，监管核心；计数不相交：inputTokens 不含 cache，计费 = 三者之和）+ `FinishReason` + `EpochHeader/RequestHeaderReason/RequestContext` + `TurnEndReason`（kind 打标：completed/aborted/blocked/error/max-tokens/interrupted，aborted 带 cancel cause，error 带 LlmFailure）+ `TodoItem`（content + status: pending/in_progress/completed）+ branded id（`CallId/SessionId`）+ `JsonValue`（tool/result 的 opaque meta）。

### 4.4 port 关键决策

1. **判别联合用 `#[serde(tag = "type")]`**：`SessionEvent` 和 `ContentBlock` 都是 `type` 字段打标。事件 wire 类型带斜杠（`turn/start`），不能用 `rename_all = "kebab-case"`（只会出连字符），**每个变体显式 `#[serde(rename = "turn/start")]`**；data 结构体字段驼峰处用 `#[serde(rename_all = "camelCase")]`；嵌套联合（如 `TurnEndReason`）用 `#[serde(tag = "kind")]`。
2. **merge-extensible 必须宽容**：官方 45 种事件会继续涨。Rust port 必须留 **unknown fallback**——**手写 `Deserialize`**：先解成通用信封（`type/seq/time/data/ignorable` 全保留），再按 `type` 分发到类型化变体，未知类型进 `Unknown { 全字段保留 }`。不能用 `#[serde(other)]`（会丢字段，破坏 lossless）。`ignorable` 语义：未知 + `ignorable:true` 可安全跳过；未知且无标记时官方要求拒绝重建——dshr 作为观察者先宽松处理，语义记在注释里。
3. **transport 划分（重要）**：官方 `transport.ts` 的功能 = 帧判断（脑）+ 管道 I/O（手脚），在 Rust 里拆开——**dshr-protocol = 类型 + 帧判断纯函数**（`rpc.rs`：RpcRequest/RpcResponse/RpcNotification + `classify` 判断函数，零依赖，只有 serde/serde_json）；**dshr-runtime = 管道 I/O + 读循环 + id 配对**（`client.rs`，依赖 tokio）。protocol 保持零依赖，UI 等消费方不必背 tokio。

### 4.5 监管面板三视图（数据主权核心）

同一份 `session.event` 流的三个切片，零额外采集成本：

| 视图 | 数据源 | 渲染 |
|---|---|---|
| **token 明细** | `assistant/message.usage` + `request/header` | 每轮 input/output/cache/reasoning token 计数 + 离线算账（token × 定价表） |
| **命令执行视图** | `tool/call`(name=bash/pwsh, arguments=命令) + `tool/result`(输出) | 终端样式只读面板，**非交互 PTY** |
| **文件 diff 视图** | `tool/result.meta.diffs` = `[{path, oldText, newText}]`（官方已算好的 result-time contextual diff） | Rust `similar` crate 渲染行级红删/绿增 |

**文件 diff 视图关键事实（已查证 `packages/fs/tool-fs`）**：`edit`/`write` 工具在 `tool/result` 的 `meta` 放 `{ diffs: [...] }`，已算好的 before/after 上下文 hunk；边界：① 只覆盖 edit/write（bash/pwsh 直接改文件无 diff）；② 是变化点附近上下文非全文件；③ `oldText` 可能为 null。渲染用 `similar::TextDiff`（unified + inline）。

### 4.6 会话数据双通道（dshr 的差异化地基）

| 通道 | 时机 | 数据 | 用途 |
|---|---|---|---|
| **实时 SDK 流** | runtime 运行时 | `session.event` 通知流（新增事件） | 聊天渲染、实时监管 |
| **历史直读磁盘** | runtime 未运行时 | 直接读 `<DSH_SESSION_ROOT>/<project>/<session>/session.jsonl.zstd` | 离线算账、备份、跨 runtime 浏览 |

**历史直读的 3 个坑（已查证 `session-persistence-jsonl`）**：
1. **zstd 多 frame**：header 帧 + 每批 append 一帧，Node 只解第一帧 → 按 RFC 8878 切边界逐个解；
2. **packed chunk row**：`packChunks` 默认 true，连续 `assistant/chunk` 打包成一条 row（seq/time 增量），需 `decodeStorageRecord` 还原；
3. **首行是 SessionHeader**：第一逻辑行 `{type:'session', id, cwd, ...}`，非 event，读时要识别跳过。

**结论**：实时走 SDK，历史直接读文件——"数据真正在自己手里"（离线可算账、可备份、可浏览全部历史）。

## 5. Rust 技术选型

| 项 | 选择 | 理由 |
|---|---|---|
| 异步 | tokio | stdio 读是异步流；Iced subscription 也是 async |
| 序列化 | serde + serde_json | 协议是 JSON |
| UI 桥接 | tokio runtime 跑在独立线程，`mpsc::channel` → Iced `Subscription` | 事件流 → Message 的天然通道 |
| 进程管理 | `tokio::process` | spawn/kill/EOF 阶梯 |
| diff 渲染 | `similar`（TextDiff） | 文件 diff 视图，unified + inline |

**crate 划分**：4 crate 平铺（见 §3），依赖方向 `dshr-ui → dshr-state → dshr-runtime → dshr-protocol`（protocol 零依赖）。

## 6. 里程碑与状态

### 阶段 A — 开发态（node carrier，目标：全链路跑通）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** `dshr-protocol` | transport + 全部类型 port（含 unknown fallback）+ serde 往返测试 | **进行中**：ContentBlock ✅（缺 image + fallback）；SessionEvent 3a（13 核心）→ 3b（fallback）→ 3c（message/llm）→ 3d（接入 client） |
| **M1** `dshr-runtime` | HarnessClient + spawn + dispose 阶梯 + `session/prompt` smoke | **主体完成**：spawn/initialize/prompt/shutdown + `init_smoke` ✅（真实 runtime 全链路）；剩余：dispose 阶梯（EOF→SIGTERM→SIGKILL）、持续读事件流 |
| **M2** `dshr-ui` 骨架 | Iced app + 聊天视图 + 流式渲染 | 未开始（先做 R1/R2 原型） |
| **M3** | 工作区/文件树/会话树 + 监管面板三视图 | 未开始 |

### 阶段 B — 发布态（单文件 exe，目标：免 Node 分发）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M4** | fork 官方 `scripts/build-exe-for-python-sdk.ts` 补 Windows 分支，产 `dsh-jsonrpc-agent-pkg-win-x64.exe` | 未开始 |
| **M5** | release pipeline：CI 构建 exe + 抓第三方 NOTICE → 打进安装包 → Release asset | 未开始 |

## 7. 风险与待验证项

- **R1 长列表虚拟化**：Iced `Scrollable` 不自动虚拟化，5000 条消息需 `widget::lazy` 或第三方方案。→ 原型必测。
- **R2 流式文本吞吐**：每 token 一次 update 的渲染吞吐要实测（20 token/s 基准）。→ 原型必测。
- **R3 Iced 版本 API 漂移**：0.14 较新，第三方 `iced_aw` 等跟进滞后；核心控件只用内置。
- **R4 unknown event 宽容**：见 4.4-2，不处理则 runtime 加插件即崩。
- **R5 审批流缺失**：`ask_user_question` dead，MVP 配 `approval: never`，交互审批留到协议扩展。
- **R6 runtime 分发与版本**：官方 npm 包 `@deepseek-ai/dsh-sdk-jsonrpc-demo` 目前 `0.1.0-rc.5`（pre-release 无兼容承诺，升级前备份 `$DSH_HOME`）；Windows 无官方 carrier（`platforms.json` 仅 linux-x64/arm64、macos-arm64）→ 依赖 Node ≥22.19 或自建 exe；npm 包需锁版本。

## 8. 决策记录（原 HANDOFF 决策 1-7）

1. **形态 = 宿主应用，不是插件**：Rust UI 作前端，harness runtime 作 sidecar 子进程，stdio JSON-RPC 通信。不 fork、不改写官方仓库。
2. **通信通道 = SDK 协议**：`session.event` 通知流把完整 session 日志信封流给客户端 → 监管数据第一手全量。官方另有 Python SDK（`deepseek-harness`，bundled runtime 免 Node）。
3. **UI 框架倾向 Iced 0.14（待原型验证）**：API 稳定、subscription 与事件流同构、Windows/Linux 一等公民、wgpu GPU 渲染。GPUI 弃选（API 不稳、crates.io 滞后、文档少）。**待办：写两个原型对比**（5000 条消息滚动 + 每秒 20 token 流式）。
4. **路线 = 渐进**：宿主应用 → 用 Rust 替换边界清晰的组件（Excel 工具、Windows 沙箱 runner）→ 未来才评估自研 minimal loop。彻底重写 = 不选。
5. **更新机制**：自己仓库 git pull/Release 自更新；runtime 走 npm/pip 依赖**锁版本**，**不 git pull 官方**；升级前备份 `$DSH_HOME`（pre-release 无兼容承诺）。
6. **runtime 获取（A/B 双轨）**：A = node carrier（长期开发态，Node ≥22.19，spawn `node <npm 包>/lib/bin.js <cordis.yml>`）；B = 单文件 exe（发布态，免 Node，自建 Windows 分支）。**官方 npm 包 = `@deepseek-ai/dsh-sdk-jsonrpc-demo`（bin: `dsh-jsonrpc-agent`）**，用 `npm install --prefix <dshr>/runtime` 锁版本装到 dshr 管理目录；**不 `npx @deepseek-ai/dsh web`**（那是 web 全家桶，不是 headless runtime）、**不 git clone**（monorepo + pnpm + 不锁版本）。启动时 `resolve_runtime()`：找 exe → 检测 node（`node --version`）→ npm 安装 → spawn；都没有则友好报错。开源仓库只放源码 + 构建脚本 + runtime 版本声明，**不 commit 任何 .exe**；exe 属发布产物并随附官方第三方 NOTICE（MIT 合规）。
7. **定位与插件边界**：dshr = 完整聊天客户端（对话、工作区、多会话、流式、工具调用展示），监管面板是内建差异化。工具/能力插件（`dsh-tool-*`、core、persistence、compaction 等）全保留——在 runtime 进程内跑，**往 cordis.yml 加即可，Rust 侧零改动**；插件安装 = dshr 改 yml + 触发 npm install + 重启进程（加载是 runtime 的事）。审批通道（`ask_user_question`）是独立缺失项（见 R5）。

## 9. 关键事实与坑（已查证）

- **审批/询问交互流在 SDK 通道是死的**：server→client 请求是 dead capability；`ask_user_question` 无 provider。配 `approval: never` 或扩展协议（runtime 侧 TS 插件转发 `ctx.approval`，不用 fork 全仓）。
- **web_fetch 默认禁用**：`tool-web` 配了 `fetch: false`（SSRF 未防护），`web_search` 可用（DeepSeek 官方，60s 超时）。
- **会话日志格式**：`.jsonl.zstd` = 多个独立 Zstandard frames 拼接（header 帧 + 每 append 批次一帧）。Node 内置 zstd 只解第一帧，需按 RFC 8878 切 frame 逐个解。
- **当前环境**：`DSH_HOME=C:\Users\qiaoy\.dsh`；web profile bundles = `@deepseek-ai/dsh-base` + `@deepseek-ai/dsh-web-app`；用户 patch 层为空。
- **官方 bundled runtime exe 无 Windows carrier**：`deepseek-harness-runtime-bin` wheel 含单文件 `dsh-jsonrpc-agent-pkg-<platform>-<arch>`（免 Node），但 `platforms.json` 仅 linux-x64 / linux-arm64 / macos-arm64。
- **官方参考组合**：`examples/jsonrpc-agent`（minimal：bash/read/write/edit/subagent/todo_write + 持久化 + compaction，无 UI 插件）；npm 发布形态 `@deepseek-ai/dsh-sdk-jsonrpc-demo`。
- **Windows 沙箱现状**：TS + koffi FFI 调 Win32 API（`dsh-sandbox-windows-acl`），有 ABI 漂移痛点 → 未来 Rust 化候选。
- **Excel 工具**：Rust 生态 = calamine（读）+ rust_xlsxwriter（写），纯 Rust 无 C 依赖；交叉编译用 cargo-zigbuild。
- **`session/prompt` 响应不保证先到**：第一次 prompt 时 runtime 先 `getOrCreateSession`（慢，期间已发 `session.event` 通知），响应是"入队回执"（messageId）——所以 client 必须按 id/method 区分而非按顺序。

## 10. 协作方式

用户正在学 Rust，**不要一次大批量生成代码**。采用小步结对：讨论出思路/小片段，用户在自己编辑器里写，agent 负责 **review 检查 + 指出问题 + 给最小修复建议**。每次只推进一个小模块。文档方面：本文是唯一信息源，改动随进展更新。

## 11. 下一步计划（按序）

1. **3a**：`SessionEvent` 核心 13 事件类型化变体（当前 8 个纯 data 事件先写，5 个依赖 message/llm 的等 3c）✅ 进行中
2. **3b**：unknown fallback（手写 Deserialize，保字段；用 `agent/inbox/spliced` 真实样例测）
3. **3c**：`message.rs` + `llm.rs`（UserMessage/AssistantMessage/ToolResultMessage/StreamChunk/TokenUsage/FinishReason）+ `ContentBlock` 补 image
4. **3d**：接入 `HarnessClient`——`prompt` 收 `SessionPromptParams`、响应/事件流反序列化（读循环正式化，等待 `session.status: idle` 再收尾）
5. **M1 收尾**：dispose 阶梯（EOF→SIGTERM→SIGKILL）+ 事件流持续订阅
6. **resolve_runtime / fetch**：npm 包安装 + node 检测 + exe 优先（决策 6 落地）
7. **M2 前置**：Iced vs GPUI 原型对比（R1/R2）
