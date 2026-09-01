//! 会话生命周期事件族。
//!
//! 对应官方 `SessionEventMap` 中 `session/end-seed`（其余 `session/title` 等
//! 扩展在 3a 不 port，由 3b fallback 兜住）。
use serde::{Deserialize, Serialize};

/// `session/end-seed` 的 data：空对象，位置和 time 携带含义。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['session/end-seed']
/// 用在构造种子结束的事件（之前的 seq 均来自种子：resume/fork/replay）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEndSeedData {}
