//! `dsh-sdk-client`：驱动一个 DeepSeek Harness runtime 子进程的 Rust SDK 客户端。
//!
//! 分层：`process`（进程生死）→ `transport`（管道对话）→ `client`（总装师）。
//! `subscription`（事件订阅/会话树）、`api`（run receipt-to-idle）在总装师之上。
//! 对应官方 TS 客户端：packages/sdk/client/src/client.ts 的 HarnessClient + api.ts 的 DeepSeekHarness。
pub mod api;
pub mod client;
pub mod error;
pub mod process;
pub mod subscription;
pub mod transport;
