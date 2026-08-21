# DSH Rust Desktop — 设计文档（v2，单一信息源）

> 本文为全项目唯一文档。官方参考仓库：`D:\DeepseekHarness\deepseek-harness`（本地 clone，源码是唯一权威，本文是施工蓝图 + 决策记录）。
> v2 重构：架构总览与协议类型清单合并；仓库布局逐文件标注官方对应；新增完整流程与数据层设计。

## 1. 定位（一句话）

完整聊天客户端 + 内建监管底座，同源 `session.event`；Rust/Iced 自绘，不套 WebView，不 fork 官方仓库。

## 2. 架构总览 + 协议类型清单

### 2.1 分层架构

```
用户双击 dshr.exe（只启动这一个）
  └─ dshr-ui（Iced，宿主 UI 进程）
       │
       ▼
  dshr-state（中间人：消费事件流 → 解析 → 写本地库；供 UI 查询）
       │
       ▼
  dshr-runtime（管理单个 runtime 进程）
    ├─ client.rs    总装师（薄）：组装 process + transport，暴露类型化方法
    ├─ transport.rs 管道对话：读循环 + id 配对 + 事件通道
    └─ process.rs   进程生死：spawn / stderr 任务 / kill / wait
       │  spawn（stdio 管道）
       ▼
  dsh-jsonrpc-agent（runtime sidecar 子进程，官方对接层 + 内部逻辑黑盒）
       ├─ 阶段 A：node <npm 包>/lib/bin.js + cordis.yml   （开发态/Node 路线）
       └─ 阶段 B：dsh-jsonrpc-agent-pkg-win-x64.exe        （发布态，免 Node）
```

数据流（单向清晰）：dshr ──stdin──▶ runtime（请求）；dshr ◀─stdout── runtime（响应 + 通知）。

### 2.2 wire 方法面（7 种消息，双向）

**请求侧（client → server，3 个）—— 你发的每个请求 `method` 必须是这三个之一：**

| method | params 类型 | result 类型 |
|---|---|---|
| `initialize` | `InitializeParams` | `InitializeResult` |
| `session/prompt` | `SessionPromptParams` | `SessionPromptResult` |
| `shutdown` | 无（wire 上 `{}`） | 空对象 `{}` |

**通知侧（server → client，4 个）—— dsh 主动发的：**

| method | 类型 | payload 形状 |
|---|---|---|
| `session.event` | `SessionEventNotification` | `{ sessionId, event: SessionEvent }` |
| `session.status` | `SessionStatusNotification` | `{ sessionId, status: 'idle' \| 'running' }` |
| `subagent.started` | `SubagentStartedNotification` | `{ parentSessionId, childSessionId }` |
| `subagent.finished` | `SubagentFinishedNotification` | `{ provider, agentId, parent, child, status, stopReason, lastAssistantMessage? }` |

**方向判定规则**：请求带 `id`（配对响应）；通知无 `id` 有 `method`。`rpc::classify` 按此区分。

### 2.3 类型清单（dshr-protocol 目标全集，全部标注官方来源）

来源：`packages/sdk/protocol/src/types.ts` 的两个 Map（`HarnessSdkRequestMap` / `HarnessSdkNotificationMap`）就是清单；顶部 import 是领域类型的门牌号。

**请求侧（你发的）** — `requests/` 模块：

| 类型 | 字段 | 官方 |
|---|---|---|
| `InitializeParams` | `cwd` `provider` `model` `maxTokens?` | types.ts |
| `InitializeResult` | `serverInfo: {name, version}` | types.ts |
| `SessionPromptParams` | `sessionId` `contentBlocks: ContentBlock[]`（未知 id 懒创建） | types.ts |
| `SessionPromptResult` | `messageId` | types.ts |
| `ShutdownResult` | 空对象 | types.ts |

**通知侧（dsh 发的）** — `notifications.rs`（规划中，3d 建）：

| 类型 | 字段 | 官方 |
|---|---|---|
| `SessionEventNotification` | `sessionId` `event: SessionEvent` | types.ts |
| `SessionStatusNotification` | `sessionId` `status` | types.ts |
| `SubagentStartedNotification` | `parentSessionId` `childSessionId` | types.ts |
| `SubagentFinishedNotification` | `provider` `agentId` `parent` `child` `status: SdkRunStatus` `stopReason` `lastAssistantMessage?` | types.ts |

**领域类型（内容物，双向共用）**：

| 组 | 内容 | 官方位置 |
|---|---|---|
| `ContentBlock` | 5 变体：text/reasoning/image/tool-call/tool-result + Unknown | `llm/llm/src/types.ts` 的 ContentBlockMap |
| `SessionEvent` | 信封 + 核心 13 事件 + Unknown fallback | `core/session/src/types.ts` 的 SessionEvent/SessionEventMap |
| `SubagentStopReason` | 5 字面量 + Unknown | `subagent/subagent/src/types.ts` |
| `llm` 共享 | TokenUsage / FinishReason / StreamChunk / LlmFailure | `llm/llm/src/types.ts` |
| 帧层 | RpcResponse / RpcError / ParseError / Notification / classify / build_request / parse | `sdk/protocol/src/transport.ts` |

## 3. 仓库布局（逐文件标注官方对应）

```
dshr/
├── Cargo.toml                # workspace（members 平铺在根目录）
├── .env                      # 本地配置（gitignore）：API key / 路径 / 模型
├── dshr-protocol/            # ① 协议：类型 + 帧层（纯逻辑，仅 serde）
│   └── src/
│       ├── lib.rs            # pub mod 汇总
│       ├── rpc.rs            # 帧层：信封/帧判断/构造/解析 ← 官方 transport.ts
│       ├── requests.rs       # 请求侧 wire 类型根 ← 官方 types.ts 的 RequestMap
│       ├── requests/
│       │   ├── initialize.rs #   InitializeParams/Result/ServerInfo ← types.ts
│       │   ├── session.rs    #   SessionPromptParams/Result ← types.ts
│       │   └── shutdown.rs   #   ShutdownResult ← types.ts
│       ├── content_block.rs  # 内容块根（newtype 变体）← llm/types.ts 的 ContentBlockMap
│       ├── content_block/
│       │   ├── contentblock.rs  # 5 种 Block 字段形状 ← ContentBlockMap
│       │   └── fallback.rs      # 未知块类型兜底（手写 Deserialize）
│       ├── session_event.rs  # SessionEvent 信封 + 判别枚举（48 种）+ turn_step() ← core/session/types.ts
│       ├── session_event/    # 事件 data 按事件族拆（核心 13 + 扩展 35，全结构化）
│       │   ├── turn.rs       #   turn/start·end、step/start·end + TurnEndReason
│       │   ├── message.rs    #   user/message、assistant/chunk·message（含 interrupted）+ Message
│       │   ├── tool.rs       #   tool/call、tool/result、tool/code-dispatch(-start)
│       │   ├── request.rs    #   request/header、request/context
│       │   ├── session.rs    #   session/end-seed
│       │   ├── misc.rs       #   todo/write + feedback/record
│       │   ├── agent.rs      #   agent-preset/selected、agent/inbox/spliced
│       │   ├── approval.rs   #   approval/asked·decided·policy、permission/preset
│       │   ├── command.rs    #   command/run、command/done
│       │   ├── compaction.rs #   compaction/start·end·prune·summary
│       │   ├── descriptor.rs #   subagent/descriptor
│       │   ├── goal.rs       #   goal/change（快照+墓碑）
│       │   ├── hook.rs       #   hook/invoked、hook/result
│       │   ├── mode.rs       #   plan/mode、sandbox/mode
│       │   ├── retry.rs      #   llm/retry、llm/retry-started
│       │   ├── schedule.rs   #   schedule/change
│       │   ├── team.rs       #   team/member、team/message/queued·delivered、team/task
│       │   ├── title.rs      #   session/title、session/title-llm-request
│       │   ├── web.rs        #   web/deepseek-search-llm-request
│       │   ├── workflow.rs   #   tool-workflow/run-start·end、agent-start·end
│       │   └── fallback.rs   #   未知事件兜底（信封 → 分发，lossless）
│       ├── llm.rs            # TokenUsage/FinishReason/StreamChunk/LlmFailure ← llm/types.ts
│       ├── notifications.rs  # 通知侧 wire 类型 + Kind 分发 ← 官方 types.ts 的 NotificationMap
│       └── subagent.rs       # SubagentStopReason ← subagent/types.ts
├── dshr-runtime/             # ② 管理单个 runtime 进程
│   ├── src/
│   │   ├── lib.rs            # 声明文件
│   │   ├── error.rs          # 统一运行时错误（thiserror，From 链吸收 ParseError）
│   │   ├── client.rs         # 总装师：组装 + 类型化方法 API（≈ 官方 client.ts）
│   │   ├── transport.rs      # 管道对话：读循环 + id 配对 + 事件通道（≈ transport.ts 的 I/O 半）
│   │   └── process.rs        # 进程生死：spawn/stderr 任务/kill/wait（≈ dispose.ts 简化）
│   └── Cargo.toml
├── dshr-state/               # ③ 中间人：UI 与 runtime 之间的总调度（三层见 §9.5）
│   ├── src/
│   │   ├── lib.rs            # 声明 + Error（thiserror From 链吸收 runtime/rusqlite）
│   │   ├── ui.rs             # ① UI 对接层入口（pub use 重导出 UiEvent/Command/AppState）
│   │   ├── ui/               #   event.rs（UiEvent）/ command.rs（Command）/ app.rs（AppState）
│   │   ├── core.rs           # ② 处理层入口
│   │   ├── core/             #   config.rs / store.rs / session.rs / transcode.rs
│   │   ├── bridge.rs         # ③ runtime 对接层入口
│   │   └── bridge/           #   bridge.rs（RtInfo/Bridge，state 内唯一 import dshr-runtime）
│   └── tests/full_round.rs   # 真实全链路测试（.env 配置驱动）
├── dshr-data/                # ④ 本地数据层（rusqlite 加工库）：加工索引（TABLES.csv 落地）
│   ├── src/lib.rs            # open()/open_in_memory() + 分层声明
│   ├── src/schema.rs         # 建表：7 张表 + 索引（幂等 IF NOT EXISTS，见 §6 / TABLES.csv）
│   ├── src/write.rs          # 写入：append-only（runtimes/sessions 例外允许 update）
│   ├── src/read.rs           # 读取：监管/历史查询入口（行查询，聚合留给 state/ui）
│   └── tests/roundtrip.rs    # 内存库往返测试（建表+写读一致）
├── data/                     # 本地数据库落盘目录（gitignore）：dshr.db 等
│   └── dshr.db               # 加工索引库（dshr-data::open 的默认路径）
├── dshr-ui/                  # ⑤ Iced UI（bin target）
│   └── src/
│       ├── main.rs           # 入口：iced::application 装配
│       ├── app.rs            # App 状态机：update/apply_event/订阅（view 薄委托）
│       ├── view.rs           # 渲染层：view/sidebar/chat_area（纯渲染）
│       ├── message.rs        # Message 枚举（用户操作 → update）
│       └── model.rs          # 视图模型（RtView/SessionView/MsgView）
├── runtime-manifest.json     # runtime 版本 pin + 获取方式（阶段 B 用）
└── scripts/fetch-runtime.ps1 # 阶段 B：下载官方 wheel 或本地构建 exe
```

## 4. 协议 port 关键决策

1. **判别联合用 `#[serde(tag = "type")]`**：事件 wire 类型带斜杠（`turn/start`），不能用 kebab-case，**每个变体显式 `#[serde(rename = "...")]`**；data 结构体驼峰处 `camelCase`；嵌套联合（`TurnEndReason`）用 `tag = "kind"`。
2. **merge-extensible 必须宽容**：官方已知事件 48 种已全结构化（核心 13 + 插件注册 35），但插件/新版还会继续涨。**手写 `Deserialize`**：信封 → 按 type 分发，未知进 `Unknown`（lossless 保留）。`ignorable` 语义：未知 + ignorable 可跳过；无标记官方要求拒绝（dshr 先宽松）。
3. **transport 划分**：帧逻辑（构造/判断/解析/信封）全在 `protocol/rpc.rs`（零依赖纯函数）；管道 I/O + 配对在 runtime 的 `transport.rs`。
4. **错误分层**：`protocol::rpc::ParseError`（帧层，零依赖手写 Display）+ `runtime::Error`（thiserror，`From` 链吸收 ParseError/io/Json）——不建单独 error crate，跨 crate"共享"靠转换链。
5. **事件通道结构化**：通知以 `Notification { method, params: Value }` 出通道，state 按 method 解析（不做字符串拼接）。
6. **保留项（未实施）**：`RpcRequest` trait（方法名+参数+结果类型绑定）——当前 3 个方法重复度低，将来加方法时再上。
7. **标记项：`EpochHeader.config` 未来大概率要结构化**：目前用 opaque `Value`（数据不丢），监管面板做"请求配置视图"（看每轮配置/system/工具列表）时补 `LlmCallConfig` 形状；`ToolSchema` 同理。

## 5. 完整流程（fn 级调用链）

```
full_round 测试（dshr-state/tests）
│
├─ HarnessClient::spawn(config)              [runtime/client.rs]
│   ├─ RuntimeProcess::spawn(config)         [runtime/process.rs]
│   │   ├─ Command::new().args().current_dir().envs().spawn()   ← 拉起 node
│   │   ├─ child.stdin/stderr/stdout.take()  ← 接管三根管道
│   │   └─ tokio::spawn(stderr 读循环)       ← 后台打日志（防管道堵 + 崩溃可见）
│   └─ Transport::start(stdin, stdout)       [runtime/transport.rs]
│       ├─ pending: HashMap<id, oneshot::Sender>
│       ├─ events: mpsc::UnboundedChannel<Notification>
│       └─ tokio::spawn(读循环)：
│            lines.next_line() → rpc::classify   [protocol/rpc.rs]
│              ├─ Response{id} → pending.remove(id) → tx.send(Ok(line))
│              ├─ Notification{method, params} → events_tx.send(...)
│              └─ None → eprintln
│            EOF → 失败所有 pending（Error::RuntimeExited）
│
├─ client.initialize(&InitializeParams)      [runtime/client.rs]
│   ├─ serde_json::to_string(params) → body
│   ├─ transport.request("initialize", &body)  [runtime/transport.rs]
│   │   ├─ id = next_id++
│   │   ├─ oneshot::channel; pending.insert(id, tx)   ← 先登记防响应先到
│   │   ├─ rpc::build_request("initialize", id, body) ← 信封行
│   │   ├─ stdin.write_all(line + "\n")
│   │   └─ rx.await → 读循环配对后返回响应行
│   └─ rpc::parse::<InitializeResult>(line)  [protocol/rpc.rs]
│       └─ RpcResponse<InitializeResult> → result（ParseError 经 From 进 runtime::Error）
│
├─ client.prompt(&SessionPromptParams)       ← 同 initialize 路径
│   └─ 返回 SessionPromptResult{message_id}
│
├─ 事件消费（state 测试）:
│   client.events().recv() → Notification{method:"session.event", params}
│   └─ params["event"] → 检查 turn/end + completed
│
└─ client.shutdown()                         [runtime/client.rs]
    ├─ transport.request("shutdown", "{}") → parse::<ShutdownResult>
    └─ process.kill_and_wait()               [runtime/process.rs]
        ├─ child.kill()
        └─ child.wait()
```

**一句话**：`client` 三行委托（序列化 → transport.request → rpc.parse），`transport` 管"写+配对"（读循环后台常驻），`process` 管生死，`rpc` 管帧形状。

## 6. 数据层设计（dshr-data 加工库）

原则：**官方文件（jsonl.zstd / sqlite）是源数据，本库是加工索引**——不重复存原始会话日志，只存加工结果 + 配置 + 操作日志。

### 表结构（rusqlite）

**表结构以 `dshr/TABLES.csv` 为单源**；`schema.rs` 的 `init_schema()` 是它的落地实现（幂等 IF NOT EXISTS）。

| 表 | 作用 | 关键列 | 写策略 |
|---|---|---|---|
| `runtimes` | 进程宿主（一个 dshr 对话 = 一个 runtime） | id/name/state(active·closed·archived)/created_at/command/args/current_dir/env | 插入；**允许 update**：改名、归档（删=改 state，不物理删） |
| `sessions` | 会话（挂 runtime 下，parent_session_id 建血缘树） | id/runtime_id/cwd/parent_session_id/created_at/status/last_seq | 插入；update：status、last_seq（增量同步书签） |
| `requests` | 进程级+轮级请求（initialize/session_prompt/shutdown） | runtime_id/session_id/turn_id/time/method/duration_ms/success/error_message | append-only |
| `turns` | 轮（turn/start 开行 → turn/end 回填），token 展开成列 | turn_id（state 生成：runtime_id-session_id-轮号）/reason/usage_input/output/cache_read/cache_write/reasoning/user_text/assistant_text | 开行插入，结束回填 |
| `events` | 事件 lossless 底线：payload 原始 JSON | (session_id,seq) PK/type/time/turn/step/payload | append-only |
| `tool_calls` | 监管命令视图直查表 | call_id/name/arguments/result_text/is_error/duration_ms/meta（工具私有载荷如 fs diff） | append-only |
| `runtime_logs` | runtime 进程日志（stderr 等，GUI 无终端时的崩溃排查依据） | runtime_id/time/level/message | append-only |

**JSON 只留 3 处本质无法展开处**：`events.payload`、`tool_calls.arguments`、`runtimes.args/env`（参数/环境变量是结构不定列表）。`tool_calls.meta` 也是 JSON 字符串（diff 等工具私有展示载荷）。其余全部展开成列，20 列以内都不怕。

### 写入路径（state 做中间人）

```
events 通道（Notification{method, params}）
  → state 按 method 分发：
      session.event → 解析 SessionEvent（fallback 兜未知）
          ├─ turn/start       → turns 开行（turn_id = runtime_id-session_id-轮号）
          ├─ turn/end         → turns 回填（耗时/reason/token 合计）
          ├─ assistant/message→ turns.assistant_text + token 列（usage）
          ├─ user/message     → turns.user_text
          ├─ tool/call·result → tool_calls（监管面板）
          └─ 其他/未知        → events 表（lossless payload）
      session.status → sessions.status
      subagent.started → sessions.parent_session_id（血缘）
  → rusqlite 批量写（事务）
```

### 读路径（给 UI/监管）

| 视图 | 查询 |
|---|---|
| token 明细 | `turns` 按 session 聚合（`read::usage_summary`） |
| 离线算账 | `usage_summary` × 定价表（state 层做） |
| 历史浏览/搜索 | `events` 按 type/time 过滤 |
| 会话树 | `sessions.parent_session_id` |
| 命令执行视图 | `tool_calls` 按 session/turn 过滤 |

**关键设计**：`events` 表存原始 payload（lossless，未知事件也能兜住）；`turns` 是提取出的账务列——两者配合，兼顾"不丢数据"与"查询效率"。

## 7. 监管面板三视图（数据主权核心）

同一份 `session.event` 流的三个切片，零额外采集成本：

| 视图 | 数据源 | 渲染 |
|---|---|---|
| **token 明细** | `assistant/message.usage` + `request/header` | 每轮 input/output/cache/reasoning 计数 + 离线算账 |
| **命令执行视图** | `tool/call`(name/arguments) + `tool/result`(输出) | 终端样式只读面板，非交互 PTY |
| **文件 diff 视图** | `tool/result.meta.diffs` = `[{path, oldText, newText}]` | `similar` crate 行级红删/绿增 |

**文件 diff 关键事实（已查证 `packages/fs/tool-fs`）**：`edit`/`write` 在 `tool/result.meta` 放已算好的 contextual diff；边界：只覆盖 edit/write、是变化点附近上下文、`oldText` 可能为 null。

## 8. 会话数据双通道

| 通道 | 时机 | 数据 | 用途 |
|---|---|---|---|
| **实时 SDK 流** | runtime 运行时 | `session.event` 通知流 | 聊天渲染、实时监管 |
| **历史直读磁盘** | runtime 未运行时 | `<DSH_SESSION_ROOT>/.../session.jsonl.zstd` | 离线算账、备份、跨 runtime 浏览 |

**历史直读 3 个坑（已查证 `session-persistence-jsonl`）**：① zstd 多 frame（Node 只解第一帧，需按 RFC 8878 切）；② packed chunk row（`packChunks` 默认 true，需 `decodeStorageRecord`）；③ 首行是 SessionHeader 非事件。注意：若 cordis.yml 换 sqlite 持久化后端，此通道目标变成 `.db`（schema-17 私有格式，读取成本高，默认不做）。

## 9. Rust 技术选型

| 项 | 选择 | 理由 |
|---|---|---|
| 异步 | tokio | stdio 读异步流；Iced subscription 同构 |
| 序列化 | serde + serde_json | 协议是 JSON |
| 错误 | thiserror（分层 From 链） | 见 §4-4 |
| 本地库 | rusqlite（bundled） | 单文件、查询强、Windows 分发友好（需 C 编译器） |
| UI 桥接 | tokio runtime 独立线程 + mpsc → Iced Subscription | 事件流 → Message 通道 |
| diff 渲染 | `similar` | 文件 diff 视图 |

**crate 划分**：5 crate 平铺，依赖方向 `dshr-ui → dshr-state → dshr-runtime → dshr-protocol`，`dshr-data` 被 state 消费（`dshr-state → dshr-data`）。protocol 仅 serde/serde_json。

## 9.5 state 三层与 UI 简单版设计（M2 前哨，用户拍板）

### state 分层（A：core 做数据转接，UI 保持薄）

```
dshr-state/src/
├── lib.rs         # 声明 + Error（对外只露 ui::AppState）
├── ui.rs          # ① UI 对接层入口（pub use 重导出）
├── ui/            #   event.rs（UiEvent）/ command.rs（Command）/ app.rs（AppState）
├── core.rs        # ② 处理层入口
├── core/          #   config / store / session / transcode
├── bridge.rs      # ③ runtime 对接层入口
└── bridge/        #   bridge.rs（RtInfo/Bridge）
```

**转接原则（core/transcode）**：一切形状转换集中在此——
- UI→runtime：`Command` → bridge 调用参数
- runtime→UI：`SessionEvent`/通知 → `UiEvent`（Msg/ToolUse/Status…）
- UI 只渲染 UiEvent（薄）；runtime 只出协议类型（纯净）；core 是唯一"翻译官"。

### UI 简单版（Iced 0.14，backend-thread 模式）

- **进程模型**：`dshr.exe` 主进程 + 每 runtime 一个 node 子进程；Iced 同步 update + tokio 后台线程 + 双 mpsc（命令/事件通道）。
- **布局**：左 runtime/会话树 + 右聊天区（消息流 + 输入框）。
- **消息流**：user/assistant 消息 + 工具摘要行（🔧 bash 150ms / 📝 edit a.md / ⛔ error）。
- **侧边栏**：runtime 添加/删除（= spawn/archive）；runtime 内多会话；cwd 只读锁死（换工作区 = 删 runtime 重建，数据保留可查）。
- **流式 chunk 渲染**：v2（简单版等 `assistant/message` 组装完再渲染）。
- **会话内详情数据源（已全部就位）**：`tool/call`+`tool/result` → tool_calls 表；`tool/result.meta.diffs` → tool_calls.meta 列（内容级 diff v2）；`assistant/message.usage` → turns 表；`session/title` 已结构化。

### 数据路径（B）

`DSH_DATA_DIR`（.env）→ 缺省 `dshr/data/dshr.db`；未来 env 合并进 setting（toml），相对路径便于打包。

## 10. 里程碑与状态

### 阶段 A — 开发态（node carrier）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** `dshr-protocol` | 全部类型 port + fallback + 帧层 | **完成**：content_block（5+fallback）、session_event（**48 种全结构化**：核心 13 + 扩展 35，含 fallback）、subagent、llm、requests、rpc、notifications（4 通知+Kind 分发）均 ✅ |
| **M1** `dshr-runtime` | HarnessClient + spawn + dispose + smoke | **主体完成**：process/transport/client 正式版 ✅，全链路真实跑通（turn/end completed 断言）；剩余：dispose 阶梯、state 消费 |
| **3d** | 协议接入 client | **大部分完成**：请求/响应类型化 ✅、事件通道结构化 ✅；剩余：state 解析通知（SessionEvent 等） |
| **3e** `dshr-data` | 建表 + 写读层 + 测试 | **完成**：schema（7 表，含 runtime_logs）/write/read + 内存库往返测试 ✅ |
| **3f** state 三层 | ui/core/bridge 骨架 + 类型定义 | **完成**：ui（UiEvent/Command/AppState）+ core（config/store/session/transcode）+ bridge（RtInfo/Bridge）✅ |
| **3g** 消费循环 | events/stderr → 落库 + 转 UiEvent（请求计量 turn_id 回填） | **完成**：RuntimeTask select 三路 + lossless 落库 + 结构化列 ✅ |
| **M2** `dshr-ui` 简单版 | Iced 窗口 + 消息流 + 侧边栏 + 输入框，全流程跑通 | **完成**：分层（main/app/view/message/model）+ 轮询 tick 接 AppState；**用户实测对话跑通** ✅（修过：Status 时序 bug、RtView 创建） |
| **M3** | 工作区/会话树 + 监管面板 | 未开始（下一步） |

### 阶段 B — 发布态（单文件 exe）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M4** | fork 官方 `build-exe-for-python-sdk.ts` 补 Windows 分支 | 未开始 |
| **M5** | release pipeline（CI 产 exe + NOTICE + 安装包） | 未开始 |

## 11. 风险与待验证项

- **R1 长列表虚拟化**：Iced `Scrollable` 不自动虚拟化，5000 条消息需 `widget::lazy`。→ 原型必测。
- **R2 流式文本吞吐**：每 token 一次 update 的渲染吞吐实测（20 token/s 基准）。→ 原型必测。
- **R3 Iced 版本 API 漂移**：0.14 较新，第三方生态跟进滞后；核心控件只用内置。**已验证**：0.14 的 `application(boot,update,view)`/`Task`（替代 Command）/`Subscription::run_with`/`stream::channel` 编译链路 OK（dshr-ui 最小窗口 ✅）。
- **R4 unknown event 宽容**：fallback 已实现，rc.8 的 `team/*` 等扩展事件靠它兜住。
- **R5 审批流缺失**：`ask_user_question` dead，MVP 配 `approval: never`。
- **R6 runtime 分发与版本**：npm 包 `@deepseek-ai/dsh-sdk-jsonrpc-demo` 目前 `0.1.0-rc.8`（pre-release 无兼容承诺，升级前备份 `$DSH_HOME`）；Windows 无官方 carrier → 依赖 Node ≥22.19 或自建 exe；npm 包需锁版本。
- **R7 会话 id 冲突**：复用同名 sessionId 会撞上磁盘历史日志（实测 error 回合）——客户端应使用唯一 id 或先清理会话根。

## 12. 决策记录

1. **形态 = 宿主应用，不是插件**：Rust UI 前端 + harness runtime sidecar 子进程，stdio JSON-RPC。不 fork、不改写官方仓库。
2. **通信通道 = SDK 协议**：`session.event` 通知流全量信封 → 监管数据第一手。
3. **UI 框架倾向 Iced 0.14（待原型验证）**：GPUI 弃选（API 不稳、crates.io 滞后）。
4. **路线 = 渐进**：宿主应用 → 替换边界清晰的组件 → 未来才评估自研 minimal loop。
5. **更新机制**：自己仓库自更新；runtime 走 npm/pip 锁版本，不 git pull 官方；升级前备份 `$DSH_HOME`。
6. **runtime 获取（A1 主推 / A2 备用 / B 废除）**：A1 = 依赖系统 node（Node ≥22.19），官方 npm 包 `@deepseek-ai/dsh-sdk-jsonrpc-demo`（bin: `dsh-jsonrpc-agent`）`npm install --prefix` 锁版本装到 dshr 管理目录；启动时 `resolve_runtime()`：检测 node → 比对已装包版本 → 缺失/不符则 npm 安装/更新 → spawn `node <管理目录>/.../lib/bin.js <cordis.yml>`。**A2（备用，1.0 稳定版后评估）** = 内置 node 可移植包，真零依赖。**B（单文件 exe）废除**：官方 pkg 打包把整个 node_modules 闭包内嵌进 exe（构建时刻固化），用户无法后续加插件，违背插件自由原则（详见 §13）。不 `npx @deepseek-ai/dsh web`（全家桶不是 headless runtime）、不 git clone、不 commit .exe。
7. **定位与插件边界**：工具/能力插件全在 runtime 进程内跑，往 cordis.yml 加即可，Rust 侧零改动；插件安装 = 改 yml + npm install + 重启。
8. **runtime 分层 = 按职责不按方向**：`process`（进程生死）/ `transport`（管道对话）/ `client`（总装师）。send/receive 方向拆已验证会变空壳（请求-响应配对本质跨方向）。
9. **错误分层**：`protocol::rpc::ParseError` + `runtime::Error`（thiserror From 链），不建单独 error crate。
10. **事件通道结构化**：`Notification{method, params}` 出通道，state 按 method 解析。
11. **存储后端选择**：runtime 用 jsonl（默认，格式已查证）；dshr 查询能力靠自己的 rusqlite 加工库，不 port 官方 sqlite schema。
12. **dshr 自身配置用 TOML**（用户数据目录，如 `~/.config/dshr/config.toml`）：配置需要注释、可手改，toml crate 纯 Rust 打包无忧；`toml` 和 `json` 一样是运行时读取，不进二进制，打包后照常工作。
13. **state 分层 = ui/core/bridge**：core/transcode 负责一切形状转换（UI→runtime、runtime→UI），UI 保持薄，runtime 保持纯净（§9.5）。
14. **数据路径 = `DSH_DATA_DIR`**（.env）→ 缺省 `dshr/data/dshr.db`；未来 env 并入 setting（toml），相对路径便于打包（§9.5）。
15. **工作区锁死**：spawn 时 `current_dir` + `InitializeParams.cwd` 钉死；cordis.yml 配 workspace-write 沙箱；UI 上 cwd 只读，换工作区 = 删 runtime 重建（archive 保留数据）。
16. **stderr 落库**：process.rs 把 stderr 行转进 mpsc 通道 → state 写 `runtime_logs` 表（GUI 无终端时的崩溃排查依据）。

## 13. 关键事实与坑（已查证）

- **审批/询问交互流在 SDK 通道是死的**：`ask_user_question` 无 provider，MVP 配 `approval: never`；扩展需 runtime 侧 TS 插件转发 `ctx.approval`。
- **web_fetch 默认禁用**（SSRF 未防护），`web_search` 可用（60s 超时）。
- **会话日志格式**：`.jsonl.zstd` = 多独立 Zstandard frames（Node 只解第一帧，按 RFC 8878 切）。
- **当前环境**：`DSH_HOME=C:\Users\qiaoy\.dsh`；web profile = `@deepseek-ai/dsh-base` + `dsh-web-app`。
- **官方 bundled runtime exe 无 Windows carrier**：`platforms.json` 仅 linux-x64/arm64、macos-arm64；`scripts/build-exe-for-python-sdk.ts` 里 `PLATFORMS = ['linux', 'macos']`，注释明确 Windows 是 non-goal。
- **官方 exe 插件固化（已查证 build-exe-for-python-sdk.ts）**：`@yao-pkg/pkg --sea` 打包，`ASSET_GLOBS` 把整个 node_modules 的 js/cjs/mjs/json/node/wasm 内嵌进 exe，Cordis 的 bare 插件 import 走内嵌虚拟 fs——**构建时刻的插件才可用，用户无法后续加插件**。这是 B 废除的根因（决策 6）。官方 Python carrier 是另一种内置：闭包复制进 wheel 的 `node/` 目录（`packaged-bin.ts`："Bare plugins resolve from the installed runtime closure"）。
- **官方参考组合**：`examples/jsonrpc-agent`（bash/read/write/edit/subagent/todo_write + jsonl 持久化 + compaction + token-meter）。
- **Windows 沙箱现状**：TS + koffi FFI（`dsh-sandbox-windows-acl`），ABI 漂移痛点 → 未来 Rust 化候选。
- **Excel 工具**：calamine（读）+ rust_xlsxwriter（写），纯 Rust。
- **`session/prompt` 响应不保证先到**：第一次 prompt 先 `getOrCreateSession`（慢，期间发事件），client 必须按 id/method 区分而非顺序。
- **rc.8 更新**：版本 rc.5→rc.8；新增 4 个 `team/*` 事件（Agent Teams，插件包模块扩展注册，不在核心 SessionEventMap）；llm-deepseek 多模态支持（验证 ContentBlock::Image）；SQLite 持久化 v2。

## 14. 协作方式

用户正在学 Rust，**不要一次大批量生成代码**。小步结对：讨论思路/小片段，用户在自己编辑器里写，agent 负责 review + 最小修复建议。本文是唯一信息源，改动随进展更新。

## 15. 下一步计划（按序）

1. ~~`notifications.rs`~~（protocol）：4 个通知类型 + Kind 分发 ✅ 已完成
2. ~~**dshr-data 建表**~~：`open()` + `init_schema()` + 写入/读取层 + 往返测试 ✅ 已完成（TABLES.csv 为单源，见 §6）
3. ~~**协议全量结构化**~~：48 种事件全结构化（核心 13 + 扩展 35）+ `turn_step()` + `interrupted` ✅（tests/event_parse.rs 全绿）
4. ~~**stderr 落库**~~：runtime stderr 通道 + `runtime_logs` 表 ✅（决策 16）
5. ~~**3f state 三层骨架**~~：ui/core/bridge 类型定义 ✅（§9.5）
6. ~~**core 转接 + 消费循环**~~：SessionEvent → UiEvent + 请求计量（turn_id 回填）+ 落库 ✅
7. ~~**Iced 最小窗口**~~：验证 0.14 编译链路（R3）✅
8. ~~**UI 简单版**~~：侧边栏 + 消息流 + 输入框，全流程跑通 ✅（用户实测）
9. **M1 收尾**：dispose 阶梯（EOF→SIGTERM→SIGKILL）
10. **resolve_runtime / fetch（A1 落地）**：node 检测（≥22.19，缺失引导安装）→ npm 包版本比对 → `npm install --prefix` 锁版本到管理目录 → spawn；UI 首次启动引导页显示进度（决策 6）
11. **M3 监管面板**：工作区/会话树完善 + 数据面板（依赖 dshr-data 查询）
12. **优化候选**：`RpcRequest` trait（消 client 方法重复，方法多了再上）
