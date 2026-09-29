//! `dsh-sdk-client`：驱动一个 DeepSeek Harness runtime 子进程的 Rust SDK 客户端。
//!
//! 主要用途：把「一个有状态的 runtime 子进程」（spawn / 握手 / 发消息 / 收事件 / 收尸）
//! 收敛成可替换的 Rust 接口，供 dshr-state 的 raw 层与桌面端使用。
//! 为什么需要：这是唯一允许碰进程与管道的 crate——协议形状在 dsh-sdk-protocol（纯逻辑），
//! 状态与落库在 dshr-state；把它单独成 crate 才能做到「协议/状态都能不起进程地测」，
//! 也才能让官方 SDK 发版时只动这一层。crate 内约束：协议类型一律从 dsh-sdk-protocol 取，
//! 不在此重复定义 wire 形状。
//! 上接：`dshr-state`（raw 层经 `SessionDriver` 语义调用）、集成测试（重建后放 `tests/`）。
//! 下接：`dsh-sdk-protocol`（`rpc` / `requests` / `notifications` / `content_block` /
//!       `session_event`）、tokio（进程与管道）。
//!
//! 分层：`process`（进程生死）→ `transport`（管道对话）→ `client`（总装师）。
//! `subscription`（事件订阅/会话树）、`api`（run receipt-to-idle）在总装师之上。
//!
//! 官方对应：packages/sdk/client/src/client.ts 的 HarnessClient + api.ts 的 DeepSeekHarness
//!（另有 launch.ts 的进程启动与 dispose.ts 的收尸阶梯）。
pub mod api;
pub mod client;
pub mod error;
pub mod process;
pub mod subscription;
pub mod transport;
