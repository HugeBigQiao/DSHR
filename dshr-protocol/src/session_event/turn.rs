//! turn/step 生命周期事件族。
//!
//! 对应官方 `packages/core/session/src/types.ts` 的 `SessionEventMap` 中
//! `turn/start`、`turn/end`、`step/start`、`step/end` 四组。
use serde::{Deserialize, Serialize};

use crate::llm::LlmFailure;

/// `turn/start` 的 data：打开 turn `turn`。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['turn/start']
/// 用在会话回合开始的事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnStartData {
    pub turn: u64,
}

/// `turn/end` 的 data：关闭 turn `turn`，附结束原因。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['turn/end']
/// 用在会话回合结束的事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnEndData {
    pub turn: u64,
    pub reason: TurnEndReason,
}

/// `step/start` 的 data：打开 step（一次模型调用 + 其工具执行）。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['step/start']
/// 用在步骤开始的事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepStartData {
    pub turn: u64,
    pub step: u64,
}

/// `step/end` 的 data：关闭 step。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['step/end']
/// 用在步骤结束的事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepEndData {
    pub turn: u64,
    pub step: u64,
}

/// 一轮 turn 为什么结束。
/// 官方：packages/core/session/src/types.ts 的 TurnEndReasonMap
/// 用在 TurnEndData.reason（wire 上是 {kind:...} 对象，merge-extensible）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TurnEndReason {
    Completed,
    Aborted { reason: TurnEndCancelCause },
    Blocked,
    // 结构化失败（LlmFailure 定义在 crate::llm）。
    Error { error: LlmFailure },
    MaxTokens,
    Interrupted,
}

/// `aborted` 的取消原因。
/// 官方：packages/core/session/src/types.ts 的 TurnEndCancelCause
/// 用在 TurnEndReason::Aborted 的 reason（wire 上是 {kind:...} 对象）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TurnEndCancelCause {
    User,
    Parent,
    Hook { reason: String },
    Disposed,
    Legacy,
}
