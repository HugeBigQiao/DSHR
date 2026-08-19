//! `SessionEvent`：会话日志事件（`type` 字段打标的判别联合）。
//!
//! 信封 + 判别枚举放本文件，事件 data 按事件族拆到子模块：
//! 每个子模块对应官方 `packages/core/session/src/types.ts` 的 `SessionEventMap` 一组事件。
pub mod approval;
pub mod compaction;
pub mod message;
pub mod misc;
pub mod request;
pub mod session;
pub mod tool;
pub mod turn;
