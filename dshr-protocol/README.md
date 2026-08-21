# dshr-protocol

dshr 的**协议层**：把官方 DeepSeek Harness SDK 的 wire 类型 + JSON-RPC 帧逻辑移植成 Rust 类型。

**零 I/O、零依赖（仅 serde/serde_json）**——本 crate 只回答"一行 JSON 长什么样、怎么解析"，管道读写由 `dshr-runtime` 负责。

## 职责边界

| 做 | 不做 |
|---|---|
| 定义协议类型（请求/响应/通知/事件） | 发请求、读管道（那是 runtime 的活） |
| JSON-RPC 帧判断/构造/解析（纯函数） | 管理进程（那是 runtime 的活） |
| merge-extensible 兜底（未知类型 lossless） | 业务逻辑 |

## 模块结构（逐文件官方对应）

| 模块 | 内容 | 官方对应 |
|---|---|---|
| `rpc.rs` | 帧层：信封/帧判断/请求构造/响应解析 | `packages/sdk/protocol/src/transport.ts` |
| `requests/` | 请求侧：initialize / session/prompt / shutdown | `packages/sdk/protocol/src/types.ts` 的 `HarnessSdkRequestMap` |
| `notifications.rs` | 通知侧：4 种通知 + `Kind` 分发 | `packages/sdk/protocol/src/types.ts` 的 `HarnessSdkNotificationMap` |
| `session_event.rs` | 会话事件信封 + 判别枚举（**48 种**） | `packages/core/session/src/types.ts` 的 `SessionEventMap` |
| `session_event/` | 事件 data 按事件族拆（turn/message/tool/…15 个文件） | 各插件包 `declare module` 注册 |
| `content_block.rs` | 内容块（text/reasoning/image/tool-call/tool-result + Unknown） | `packages/llm/llm/src/types.ts` 的 `ContentBlockMap` |
| `llm.rs` | TokenUsage / StreamChunk / FinishReason / LlmFailure | `packages/llm/llm/src/types.ts` |
| `subagent.rs` | SubagentStopReason | `packages/subagent/subagent/src/types.ts` |

## 关键设计：merge-extensible 兜底

官方协议是**开放扩展**的（插件可注册新事件类型、版本会继续涨），而 Rust 枚举是封闭集合。解法：

1. **手写 `Deserialize`**（`session_event/fallback.rs`、`content_block/fallback.rs`）：通用信封 → 按 `type` 字符串分发
2. **已知 48 种 → 类型化变体**；**未知 → `Unknown` 变体**（原始字段 lossless 保留）
3. 新增事件类型时：加变体 + 加 fallback 分发 + 加测试样本（见下）

## 用法示例

```rust
use dshr_protocol::rpc;

// ① 判断一行帧：有 id = 响应；有 method 无 id = 通知
let frame = rpc::classify(line).expect("合法 JSON-RPC 行");
match frame {
    rpc::Frame::Response { id } => { /* 配对挂起请求 */ }
    rpc::Frame::Notification(n) => { /* method + params(Value) */ }
}

// ② 解析响应 result
let result: InitializeResult = rpc::parse(&resp_line)?;

// ③ 通知 → 类型化 Kind（state 的分发入口）
let kind = notifications::parse(&notification)?;  // Option<Kind>
match kind {
    Some(Kind::SessionEvent(n)) => { /* n.session_id + n.event */ }
    _ => {}
}

// ④ 事件辅助
event.event_type(); // "turn/end"（落库 events 表）
event.seq();        // 会话内序号
event.turn_step();  // (Option<turn>, Option<step>)
```

## 测试

`tests/event_parse.rs`：真实 stdout 样本 + 合成样本验证 48 种事件解析与 Unknown 兜底。

```bash
cargo test -p dshr-protocol
```

## 维护提示

- **新增官方事件**：`session_event/<族>.rs` 加 data 结构体 → `session_event.rs` 加变体（显式 `#[serde(rename = "...")]`）→ `fallback.rs` 加分发 → `seq()/time()/event_type()` 三个 match 补分支 → 测试补样本
- **命名**：wire 类型带斜杠（`turn/start`）不能用 kebab-case，**每个变体显式 rename**；data 结构体驼峰处用 `camelCase`
- **判别联合**：内层判别用 `#[serde(tag = "kind"/"mode"/"operation")]`，如 `TurnEndReason`、`LlmRetryData`、`GoalChangeData`
- **Branded 类型**（`CallId` 等）运行时就是普通字符串，统一用 `String`
