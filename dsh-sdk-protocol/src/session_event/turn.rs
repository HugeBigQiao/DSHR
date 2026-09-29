//! turn/step 生命周期事件族。
//!
//! 主要用途：`turn/start`、`turn/end`、`step/start`、`step/end` 四组事件的 data 类型
//!（含 `TurnEndReason` / `TurnEndCancelCause` 两个嵌套联合）。
//! 为什么需要：这四种是会话的**骨架事件**——turn/step 编号与结束原因是所有统计
//!（轮数、时长、失败率）的分组键，与「某插件注册的扩展事件」变化频率完全不同，故单列文件；
//! 也让 `TurnEndReason` 的 `#[serde(other)]` 兜底策略有明确归属。
//! 上接：`session_event.rs` 的判别枚举（TurnStart/TurnEnd/StepStart/StepEnd）与
//!       `session_event/fallback.rs` 的分发；`dshr-state` 的 fold / store（轮次统计与落库）。
//! 下接：`crate::llm::LlmFailure`（`TurnEndReason::Error` 的载荷）。
//!
//! 官方对应：packages/core/session/src/types.ts 的 `SessionEventMap` 中
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
    Aborted {
        reason: TurnEndCancelCause,
    },
    Blocked,
    // 结构化失败（LlmFailure 定义在 crate::llm）。
    Error {
        error: LlmFailure,
    },
    MaxTokens,
    Interrupted,
    /// fork 种子构造时关闭了在 fork 边界仍未结束的 turn（只有 fork 种子带此标记，
    /// 主循环从不发出；边界之前的事件在子会话中保持完整）。
    /// 官方 0.1.7-rc.2 新增（TurnEndReasonMap.forked）。
    Forked,
    /// 兜底：官方 merge-extensible，未来新增 kind 时不整体解析失败
    ///（保持 dshr 既有策略：只关心 Api 真需要的语义，其余 lossless 退化为
    /// `Other`；`Unknown` 事件级兜底在 fallback.rs）。
    #[serde(other)]
    Other,
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
