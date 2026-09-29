//! crate 根：`dsh-sdk-protocol` 的模块清单（wire 类型 + 帧层）。
//!
//! 主要用途：声明并汇总协议层的全部公开模块；消费方统一从 crate 根进入
//!（`dsh_sdk_protocol::rpc` / `::requests` / `::notifications` / `::session_event` / …）。
//! 为什么需要：协议层必须能脱离客户端单独编译与单测（纯逻辑、只依赖 serde/serde_json），
//! 所以「协议面有哪些模块」收敛在这一个文件里；任何 I/O、进程、状态管理都不许进这个
//! crate，否则 dsh-sdk-client / dshr-state 的分层与「不起进程的单测」都会失去。
//! 上接：`dsh-sdk-client`（client / transport / api / subscription）、`dshr-state`（raw 层）
//!       以 crate 依赖进入。
//! 下接：无——叶子 crate，外部只依赖 serde / serde_json。
//!
//! 官方对应：packages/sdk/protocol/src/index.ts 的导出面（`types.ts` 的
//! `HarnessSdkRequestMap` / `HarnessSdkNotificationMap`，`transport.ts` 的
//! `JsonRpcLineTransport` / `JsonRpcResponseError`），分别由 `rpc.rs`、`requests.rs`、
//! `notifications.rs`、`session_event.rs` 承接。
pub mod content_block;
pub mod llm;
pub mod notifications;
pub mod requests;
pub mod rpc;
pub mod session_event;
pub mod subagent;
