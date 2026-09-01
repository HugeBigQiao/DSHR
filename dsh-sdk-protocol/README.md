# dsh-sdk-protocol

DeepSeek Harness SDK 的 Rust wire 协议层：官方 TS 包 `@deepseek-ai/dsh-sdk-protocol` 的类型 + 帧逻辑的 Rust port。

**零 I/O、零依赖（仅 serde/serde_json）**——本 crate 只回答"一行 JSON 长什么样、怎么解析"，管道读写由 `dsh-sdk-client` 负责。

## 职责边界

| 做 | 不做 |
|---|---|
| 定义协议类型（请求/响应/通知/事件） | 发请求、读管道（那是 client 的活） |
| JSON-RPC 帧判断/构造/解析（纯函数） | 管理进程（那是 client 的活） |
| merge-extensible 兜底（未知类型 lossless） | 业务逻辑 |

## 模块结构（逐文件官方对应）

| 模块 | 内容 | 官方对应 |
|---|---|---|
| `rpc.rs` | 帧层：信封/帧判断/请求构造/响应解析 | `packages/sdk/protocol/src/transport.ts` |
| `requests/` | 请求侧：initialize / session/prompt / shutdown | `packages/sdk/protocol/src/types.ts` 的 `HarnessSdkRequestMap` |
| `notifications.rs` | 通知侧：4 种通知 + `Kind` 分发 | `types.ts` 的 `HarnessSdkNotificationMap` |
| `session_event.rs` | 会话事件信封 + 判别枚举 + `turn_step()` | `packages/core/session/src/types.ts` 的 `SessionEventMap` |
| `session_event/` | 事件 data 按事件族拆（48 种结构化 + Unknown 兜底） | 各插件包 `declare module` 注册 |
| `content_block.rs` | 内容块（text/reasoning/image/tool-call/tool-result + Unknown） | `packages/llm/llm/src/types.ts` 的 `ContentBlockMap` |
| `llm.rs` | TokenUsage / StreamChunk / FinishReason / LlmFailure | `packages/llm/llm/src/types.ts` |
| `subagent.rs` | SubagentStopReason | `packages/subagent/subagent/src/types.ts` |

## 关键设计：merge-extensible 兜底

官方协议是开放扩展的（插件可注册新事件、版本会继续涨），Rust 枚举是封闭集合。解法：

1. **手写 `Deserialize`**（`session_event/fallback.rs`、`content_block/fallback.rs`）：通用信封 → 按 `type` 字符串分发
2. **已知 → 类型化变体；未知 → `Unknown` 变体**（原始字段 lossless 保留）
3. 字符串联合枚举加 `#[serde(other)] Unknown`（参照 `subagent.rs` 的 `SubagentStopReason`）

新增事件类型时：加变体 + 加 fallback 分发 + 加回归测试（硬约束，见 DESIGN.md §4-8）。

## 用法示例

```rust
use dsh_sdk_protocol::rpc;
// rpc::classify / build_request / parse 为纯函数，配合 dsh-sdk-client 使用；
// 事件解析入口：dsh_sdk_protocol::notifications::parse
```

详细用法见 [dsh-sdk-client](../dsh-sdk-client/README.md)。
