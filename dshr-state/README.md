# dshr-state

桌面端 **state 层**：dshr-ui 与 DSH runtime 之间的全部中间逻辑——配置、runtime 获取、
与 SDK 沟通、数据折叠、落库、全程记录。

- 整体设计与决策：[`../DESIGN.md`](../DESIGN.md)（state 分层见 §3）
- 过程中的踩坑与设想：[`../AI-LOG.md`](../AI-LOG.md)

---

## 1. 这个 crate 干什么

一句话：**dshr-ui 只画页面，所有数据流都走本 crate。**

```
dshr-ui（纯页面）
   │ Cmd          ▲ Event
   ▼              │
┌──────────────────────────────────────────────────────────┐
│ dshr-state                                               │
│                                                          │
│  engine  核心数据处理：多 runtime/会话路由、脏检测、落库节流 │
│    │ ▲                                                   │
│  raw     与 SDK 沟通：进程生死 + wire 协议 + 订阅 + WireLog │
│                                                          │
│  fold    纯投影：事件流 → 内存快照（被 engine 调用）        │
│  store   sqlite 加工库                                    │
│  （旁路：config / secrets / runtime / record / workspace） │
└──────────────────────────┬───────────────────────────────┘
                           ▼ SessionDriver
                   dsh-sdk-client → dsh --profile sdk
```

**为什么要这一层**（而不是让 UI 直接用 SDK）：

1. **UI 不该碰进程与协议**——那会让 UI 无法单测，且把协议漂移的影响直接透到页面。
2. **多 runtime / 多会话需要持状态**——官方 wire 协议里没有「runtime」概念（只有一条连接），
   注册表、生命周期监督、命令路由只能由某一层持有。
3. **数据要落盘**——fold 的内存快照 + sqlite 事实表是历史查询与监控页的数据源。
4. **能离线回归**——WireLog 回放走同一条 fold，不 spawn runtime、不烧 token。

---

## 2. 三大块

依赖方向**单向**：`ui → engine → raw → dsh-sdk-client`。
`fold` / `snapshot` / `store` 是被 engine 调用的**纯数据层**，不在这条链上从属关系。

### 2.1 `raw` — 与 SDK 沟通

| 文件 | 职责 |
|---|---|
| `raw.rs` | `Runtime`：**一个实例 = 一个 runtime 子进程**。持有 `Box<dyn SessionDriver>` + 事件广播接收端 + 当前会话态 + 落库 Store；`EngineCmd`/`EngineEvent` 的**当前**定义处（M3 起迁到 engine） |
| `raw/driver.rs` | `SessionDriver` trait：一个 runtime 的完整可观测面（三进四出 + `stderr` + `runtime_status`），加 `BoxFuture` 别名（为 dyn 兼容） |
| `raw/client_driver.rs` | `ClientDriver`：把 `HarnessClient` 适配成 `SessionDriver`（消化「收尾消费 self」的所有权差异） |
| `raw/mode.rs` | Fake / Real 判定与 spawn 装配（`resolve_mode` / `kit` / `workspace_root`）：Fake = node 跑 `fake_runtime.mjs`；Real = `config::load` + `runtime::ensure` + env 对齐 |

- **上接**：`crate::engine`（`Engine` 按 runtime 路由命令、聚合各 runtime 的事件流）。
- **下接**：`dsh-sdk-client`（经 `ClientDriver` 适配 `HarnessClient`）、`fold`（`Folder`）、`store`、`snapshot`、
  `config` / `runtime`（经 `mode`）。
- **为什么单独一层**：见 DESIGN §3.2 / §3.3。要点是它的变化频率跟**官方发版**走，
  且它是唯一碰进程与管道的层——合并进 engine 会使数据管道无法用假 driver 单测。

### 2.2 `engine` — 核心数据处理

> **状态：M1 阶段是 `raw` 的再出口**（`pub mod engine { pub use crate::raw::{…} }`），
> 目的是让 `dshr_state::engine::{EngineCmd, EngineEvent}` 这条既有公开路径不变、UI 免于二次改动。
> **M3 起**这里落成真正的 `Engine`：多 runtime 注册表 + 每会话态，并把现在散在 raw 里的
> `emit_snapshot` / `persist` / `flush_persist` 搬进来。

| 文件 | 职责 |
|---|---|
| `engine.rs` | 类型层：`EngineCmd` / `EngineEvent` / `SessionId`（+ 协议类型再出口） |
| `engine/registry.rs` | `Engine`：runtime 注册表 + 命令路由 + 事件循环（多路复用用 `FuturesUnordered`） |
| `engine/session.rs` | 单会话态：`Folder` + 脏标记 + 上次快照 |

### 2.3 `fold` — UI 数据投影（**纯函数，无副作用**）

| 文件 | 职责 |
|---|---|
| `fold.rs` | `Folder`：折叠状态机。`push_event` / `push_notification` / `push_wire_line` 喂入，`snapshot()` 产出 `SessionSnapshot` |
| `fold/event.rs` | 单条事件 → 折叠态推进（`on_*` / `push_row` / `push_notice`） |
| `fold/render.rs` | 纯渲染与解析（`text_of` / `reasoning_of` / `reason_text` / `stream_summary` / `fold_diffs`） |

- **同源同巡**：在线（SDK 通知）与离线（WireLog JSONL 回放）走**同一套**折叠语义。
- **为什么要它纯**：纯才能用「事件 JSON → 快照相等」断言；协议漂移/字段改名/新事件类型
  全靠它兜住而不炸 UI。见 DESIGN §3.4。

### 2.4 数据与持久化

| 文件 | 职责 |
|---|---|
| `snapshot.rs` | **fold 的输出类型**（`SessionSnapshot` / `MsgItem` / `TurnStat` / `ToolItem` / `FileDiff` / `StreamSummary` / `UsageAgg`）。这是「UI 模型」，engine 只搬运不解释 |
| `store.rs` | `Store` 门面：`open` / `open_in_memory` / `init_schema` / `persist_snapshot` / `session_summaries` |
| `store/schema.rs` | §8.2 表集 DDL + §8.3 聚合查询 SQL |
| `store/write.rs` | persist 内部实现（`upsert_session` / `replace_turns` / `replace_tool_calls` / `replace_file_ops` / `infer_op`） |
| `store/convert.rs` | rusqlite 整数列 ↔ u64 转换、时间戳、状态串 |
| `store/error.rs` | `StoreError` + `Result` |
| `record.rs` | WireLog 装载（`Recorder`：`cat="dsh"` 线级记录 + `cat="app"` 应用轨迹） |

**落库语义 = 会话整体重放**：`persist_snapshot` 在一个事务内 UPSERT `sessions` +
DELETE+INSERT `turns`/`tool_calls`/`file_ops`，同一快照重复 persist **行数不变**（幂等）。

### 2.5 旁路支撑

| 文件 | 职责 | 上接 / 下接 |
|---|---|---|
| `config.rs` | 配置加载（`config.json`：provider / model / dsh-version） | 上接 `main.rs` / `raw/mode`；下接 文件系统 |
| `secrets.rs` | API key（`data/secrets.json`，Unix 0600） | 上接 `raw/mode` / UI 设置页；下接 文件系统 |
| `runtime.rs` | runtime 获取：node 版本校验 + 锁定版本 `pnpm install --ignore-scripts` | 上接 `raw/mode`；下接 node / pnpm |
| `workspace.rs` | 工作区文件读写（**仅限工作区内相对路径**） | 上接 UI 文件页；下接 文件系统 |

> 本 crate **不提供可执行文件**（`src/main.rs` 与 `src/session.rs` 已于 2026-09-29 删除）：
> 程序入口只有 `dshr-ui/src/main.rs`，它经 bridge → engine 驱动同一条链路。

---

## 3. 一趟数据的完整路径

```
用户在 UI 输入 → BridgeCmd::Prompt
  → engine（路由到目标 runtime/session）
  → raw::Runtime::prompt → HarnessClient::prompt → 「session/prompt」请求 → dsh runtime
                                                                    │
                    通知流（session.event × N、session.status）◀────┘
  → raw 广播接收端 → engine 分发
       ├─→ fold::Folder 折叠 → SessionSnapshot（内存快照，UI 按需读）
       └─→ store::Store.persist_snapshot（落库为事实表）
  → EngineEvent → UI bridge → app.rs → model.rs（视图模型）→ 渲染
```

**全程落盘**（`data/wire-logs/*.jsonl`）：请求/响应/每条通知（细到 eventType）
+ 应用侧轨迹。它是 **lossless 源**——出问题时以它为准，db 只是加工结果。

---

## 4. 目录速查

```
dshr-state/
├── src/
│   ├── lib.rs            模块清单 + 分层说明
│   ├── raw.rs            与 SDK 沟通：Runtime（一个实例 = 一个 runtime 子进程）
│   ├── raw/
│   │   ├── driver.rs          SessionDriver trait（可替换驱动）
│   │   ├── client_driver.rs   HarnessClient → SessionDriver 适配
│   │   └── mode.rs            Fake/Real 判定与 spawn 装配
│   ├── engine.rs         类型层：EngineCmd / EngineEvent / SessionId
│   ├── engine/
│   │   ├── registry.rs   Engine：注册表 + 命令路由 + 事件循环
│   │   └── session.rs    单会话态（Folder + 脏检测 + 上次快照）
│   ├── fold.rs           纯投影：Folder（折叠状态机）
│   ├── fold/
│   │   ├── event.rs      事件 → 折叠态
│   │   └── render.rs     纯渲染/解析
│   ├── snapshot.rs       fold 的输出类型（UI 模型）
│   ├── store.rs          sqlite 加工库门面
│   ├── store/
│   │   ├── error.rs      StoreError + Result
│   │   ├── schema.rs     DDL + 聚合查询
│   │   ├── convert.rs    i64 ↔ u64 / 时间戳 / 状态串
│   │   └── write.rs      upsert / replace_*
│   ├── record.rs         WireLog（cat=dsh / cat=app）
│   ├── config.rs         配置加载
│   ├── secrets.rs        API key
│   ├── runtime.rs        runtime 获取（锁版本 pnpm）
│   └── workspace.rs      工作区文件读写
├── tests/                契约测试（2026-09-29 起按此目录重建）
│   ├── engine_flow.rs        engine 主链路 / 路由隔离 / stderr 与退出落盘
│   ├── fold_projection.rs    fold 投影语义 + 在线/离线同源同巡
│   ├── store_persistence.rs  落库幂等与替换语义
│   ├── engine_session.rs     真实会话逐步透明账本（DSHR_LIVE=1）
│   └── _conventions.md       本 crate 的测试约定（下划线前缀 → 不是测试目标）
├── Cargo.toml
└── README.md             本文件
```

---

## 5. 约定

- **文件风格**：Rust 2018+（`x.rs` + `x/`），**不用 `mod.rs`**。
- **注释三级制**：文件级（功能 / 用途 / 为何需要 / 上接 / 下接）+ 函数与 trait 级
  （为何需要 / 入参出参 / 主要功能）+ 官方引用钉到具体文件与函数。
- **单文件平均 ≤350 行**；超了拆（`fold` 与 `store` 已按此拆分）。
- **测试**：协议改动必须带回归测试。2026-09-29 全部测试被清空并重建为
  「各 crate 一个 `tests/` 目录」——本 crate 的约定见
  [`tests/_conventions.md`](tests/_conventions.md)（含两类测试的区分与 11 个
  **无门控**测试接缝的清单）。真实会话的逐步透明账本在
  [`tests/engine_session.rs`](tests/engine_session.rs)（`DSHR_LIVE=1` 才跑）。
- **改完跑**：`cargo test --workspace` + `cargo fmt --all -- --check`。
  注意 PowerShell 会把 cargo 的 stderr 误判为命令失败（看输出而非裸退出码）。
