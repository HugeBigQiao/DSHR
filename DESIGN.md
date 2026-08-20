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
│       ├── session_event.rs  # SessionEvent 信封 + 判别枚举 ← core/session/types.ts
│       ├── session_event/    # 事件 data 按事件族拆
│       │   ├── turn.rs       #   turn/start·end、step/start·end + TurnEndReason
│       │   ├── message.rs    #   user/message、assistant/chunk·message + Message
│       │   ├── tool.rs       #   tool/call、tool/result
│       │   ├── request.rs    #   request/header、request/context
│       │   ├── session.rs    #   session/end-seed
│       │   ├── misc.rs       #   todo/write + TodoItem
│       │   ├── approval.rs   #   扩展事件占位（fallback 兜住）
│       │   ├── compaction.rs #   扩展事件占位（fallback 兜住）
│       │   └── fallback.rs   #   未知事件兜底（信封 → 分发，lossless）
│       ├── llm.rs            # TokenUsage/FinishReason/StreamChunk/LlmFailure ← llm/types.ts
│       └── subagent.rs       # SubagentStopReason ← subagent/types.ts
├── dshr-runtime/             # ② 管理单个 runtime 进程
│   ├── src/
│   │   ├── lib.rs            # 声明文件
│   │   ├── error.rs          # 统一运行时错误（thiserror，From 链吸收 ParseError）
│   │   ├── client.rs         # 总装师：组装 + 类型化方法 API（≈ 官方 client.ts）
│   │   ├── transport.rs      # 管道对话：读循环 + id 配对 + 事件通道（≈ transport.ts 的 I/O 半）
│   │   └── process.rs        # 进程生死：spawn/stderr 任务/kill/wait（≈ dispose.ts 简化）
│   └── Cargo.toml
├── dshr-state/               # ③ 中间人：消费事件流 → 解析 → 写本地库（纯逻辑 + 集成测试）
│   ├── src/lib.rs
│   └── tests/full_round.rs   # 真实全链路测试（.env 配置驱动）
├── dshr-data/                # ④ 本地数据层（rusqlite 加工库）：账务/索引/配置/审计
│   └── src/lib.rs            # open() + 规划表（见 §6）
├── dshr-ui/                  # ⑤ Iced UI（bin target，未开工）
│   └── src/main.rs
├── runtime-manifest.json     # runtime 版本 pin + 获取方式（阶段 B 用）
└── scripts/fetch-runtime.ps1 # 阶段 B：下载官方 wheel 或本地构建 exe
```

## 4. 协议 port 关键决策

1. **判别联合用 `#[serde(tag = "type")]`**：事件 wire 类型带斜杠（`turn/start`），不能用 kebab-case，**每个变体显式 `#[serde(rename = "...")]`**；data 结构体驼峰处 `camelCase`；嵌套联合（`TurnEndReason`）用 `tag = "kind"`。
2. **merge-extensible 必须宽容**：官方 48 种事件会继续涨（rc.8 的 `team/*` 甚至不在核心包）。**手写 `Deserialize`**：信封 → 按 type 分发，未知进 `Unknown`（lossless 保留）。`ignorable` 语义：未知 + ignorable 可跳过；无标记官方要求拒绝（dshr 先宽松）。
3. **transport 划分**：帧逻辑（构造/判断/解析/信封）全在 `protocol/rpc.rs`（零依赖纯函数）；管道 I/O + 配对在 runtime 的 `transport.rs`。
4. **错误分层**：`protocol::rpc::ParseError`（帧层，零依赖手写 Display）+ `runtime::Error`（thiserror，`From` 链吸收 ParseError/io/Json）——不建单独 error crate，跨 crate"共享"靠转换链。
5. **事件通道结构化**：通知以 `Notification { method, params: Value }` 出通道，state 按 method 解析（不做字符串拼接）。
6. **保留项（未实施）**：`RpcRequest` trait（方法名+参数+结果类型绑定）——当前 3 个方法重复度低，将来加方法时再上。

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

```sql
-- 会话 + 血缘（subagent 父子）
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,              -- sessionId
    cwd TEXT NOT NULL,
    parent_session_id TEXT,           -- 从 subagent.started 填
    created_at INTEGER NOT NULL,      -- epoch ms
    last_seq INTEGER NOT NULL DEFAULT 0,
    status TEXT                       -- idle / running（session.status）
);

-- 事件索引（回放/搜索，payload lossless——Unknown 事件也能存）
CREATE TABLE events (
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    type TEXT NOT NULL,               -- turn/start, tool/call…
    time INTEGER NOT NULL,
    payload TEXT NOT NULL,            -- 原始 data JSON
    PRIMARY KEY (session_id, seq)
);

-- token 账务（监管核心）
CREATE TABLE usage (
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,             -- assistant/message 的 seq
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    model TEXT,                       -- 从 request/context 补
    PRIMARY KEY (session_id, seq)
);

-- 离线算账（token × 定价表）
CREATE TABLE accounting (
    session_id TEXT NOT NULL,
    date TEXT NOT NULL,               -- YYYY-MM-DD
    input_tokens INTEGER, output_tokens INTEGER,
    cache_tokens INTEGER, reasoning_tokens INTEGER,
    cost REAL,
    PRIMARY KEY (session_id, date)
);

-- 配置 + 操作日志（官方没有的数据）
CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    time INTEGER NOT NULL,
    event TEXT NOT NULL,              -- runtime_started / request_sent / shutdown…
    detail TEXT
);
```

### 写入路径（state 做中间人）

```
events 通道（Notification{method, params}）
  → state 按 method 分发：
      session.event → 解析 SessionEvent（fallback 兜未知）
          ├─ turn/start·end → sessions.status / 回合计数
          ├─ assistant/message → 消息记录 + usage 表
          ├─ request/context → model 补全
          ├─ tool/call·result → 命令记录（监管面板）
          └─ 其他 → events 表（lossless payload）
      subagent.started → sessions.parent_session_id（血缘）
  → rusqlite 批量写（事务）
```

### 读路径（给 UI/监管）

| 视图 | 查询 |
|---|---|
| token 明细 | `usage` 表按 session/日期聚合 |
| 离线算账 | `accounting`（token × 定价，后台算） |
| 历史浏览/搜索 | `events` 表按 type/time 过滤 |
| 会话树 | `sessions.parent_session_id` |

**关键设计**：`events` 表存原始 payload（lossless），查询时按需解析；`usage` 是提取出的账务列——两者配合，兼顾"不丢数据"与"查询效率"。

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

## 10. 里程碑与状态

### 阶段 A — 开发态（node carrier）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** `dshr-protocol` | 全部类型 port + fallback + 帧层 | **主体完成**：content_block（5+fallback）、session_event（13 核心+fallback）、subagent、llm、requests、rpc 均 ✅；缺 `notifications.rs`（4 通知类型，3d 建） |
| **M1** `dshr-runtime` | HarnessClient + spawn + dispose + smoke | **主体完成**：process/transport/client 正式版 ✅，全链路真实跑通（turn/end completed 断言）；剩余：dispose 阶梯、state 消费 |
| **3d** | 协议接入 client | **大部分完成**：请求/响应类型化 ✅、事件通道结构化 ✅；剩余：state 解析通知（SessionEvent 等） |
| **M2** `dshr-ui` 骨架 | Iced app + 聊天视图 + 流式渲染 | 未开始（先 R1/R2 原型） |
| **M3** | 工作区/会话树 + 监管面板 | 未开始（依赖 dshr-data） |

### 阶段 B — 发布态（单文件 exe）

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M4** | fork 官方 `build-exe-for-python-sdk.ts` 补 Windows 分支 | 未开始 |
| **M5** | release pipeline（CI 产 exe + NOTICE + 安装包） | 未开始 |

## 11. 风险与待验证项

- **R1 长列表虚拟化**：Iced `Scrollable` 不自动虚拟化，5000 条消息需 `widget::lazy`。→ 原型必测。
- **R2 流式文本吞吐**：每 token 一次 update 的渲染吞吐实测（20 token/s 基准）。→ 原型必测。
- **R3 Iced 版本 API 漂移**：0.14 较新，第三方生态跟进滞后；核心控件只用内置。
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
6. **runtime 获取（A/B 双轨）**：A = node carrier（长期开发态，Node ≥22.19，`node <npm 包>/lib/bin.js <cordis.yml>`）；B = 单文件 exe（发布态，免 Node，自建 Windows 分支）。**官方 npm 包 = `@deepseek-ai/dsh-sdk-jsonrpc-demo`（bin: `dsh-jsonrpc-agent`）**，`npm install --prefix` 锁版本装到 dshr 管理目录；不 `npx @deepseek-ai/dsh web`（全家桶不是 headless runtime）、不 git clone。启动时 `resolve_runtime()`：找 exe → 检测 node → npm 安装 → spawn。不 commit .exe；exe 属发布产物并附官方 NOTICE（MIT）。
7. **定位与插件边界**：工具/能力插件全在 runtime 进程内跑，往 cordis.yml 加即可，Rust 侧零改动；插件安装 = 改 yml + npm install + 重启。
8. **runtime 分层 = 按职责不按方向**：`process`（进程生死）/ `transport`（管道对话）/ `client`（总装师）。send/receive 方向拆已验证会变空壳（请求-响应配对本质跨方向）。
9. **错误分层**：`protocol::rpc::ParseError` + `runtime::Error`（thiserror From 链），不建单独 error crate。
10. **事件通道结构化**：`Notification{method, params}` 出通道，state 按 method 解析。
11. **存储后端选择**：runtime 用 jsonl（默认，格式已查证）；dshr 查询能力靠自己的 rusqlite 加工库，不 port 官方 sqlite schema。

## 13. 关键事实与坑（已查证）

- **审批/询问交互流在 SDK 通道是死的**：`ask_user_question` 无 provider，MVP 配 `approval: never`；扩展需 runtime 侧 TS 插件转发 `ctx.approval`。
- **web_fetch 默认禁用**（SSRF 未防护），`web_search` 可用（60s 超时）。
- **会话日志格式**：`.jsonl.zstd` = 多独立 Zstandard frames（Node 只解第一帧，按 RFC 8878 切）。
- **当前环境**：`DSH_HOME=C:\Users\qiaoy\.dsh`；web profile = `@deepseek-ai/dsh-base` + `dsh-web-app`。
- **官方 bundled runtime exe 无 Windows carrier**：`platforms.json` 仅 linux-x64/arm64、macos-arm64。
- **官方参考组合**：`examples/jsonrpc-agent`（bash/read/write/edit/subagent/todo_write + jsonl 持久化 + compaction + token-meter）。
- **Windows 沙箱现状**：TS + koffi FFI（`dsh-sandbox-windows-acl`），ABI 漂移痛点 → 未来 Rust 化候选。
- **Excel 工具**：calamine（读）+ rust_xlsxwriter（写），纯 Rust。
- **`session/prompt` 响应不保证先到**：第一次 prompt 先 `getOrCreateSession`（慢，期间发事件），client 必须按 id/method 区分而非顺序。
- **rc.8 更新**：版本 rc.5→rc.8；新增 4 个 `team/*` 事件（Agent Teams，插件包模块扩展注册，不在核心 SessionEventMap）；llm-deepseek 多模态支持（验证 ContentBlock::Image）；SQLite 持久化 v2。

## 14. 协作方式

用户正在学 Rust，**不要一次大批量生成代码**。小步结对：讨论思路/小片段，用户在自己编辑器里写，agent 负责 review + 最小修复建议。本文是唯一信息源，改动随进展更新。

## 15. 下一步计划（按序）

1. **`notifications.rs`**（protocol）：4 个通知类型（`SessionEventNotification` 等）——state 解析的前置
2. **dshr-data 建表**：`open()` + `init_schema()`（§6 的表）+ 基本写入函数
3. **state 消费循环**：events 通道 → 按 method 解析 → 写 dshr-data（3d 收尾）
4. **M1 收尾**：dispose 阶梯（EOF→SIGTERM→SIGKILL）
5. **resolve_runtime / fetch**：npm 包安装 + node 检测 + exe 优先（决策 6 落地）
6. **M2 前置**：Iced vs GPUI 原型对比（R1/R2）
7. **优化候选**：`RpcRequest` trait（消 client 方法重复，方法多了再上）
