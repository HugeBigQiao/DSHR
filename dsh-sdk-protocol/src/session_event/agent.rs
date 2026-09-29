//! agent 侧扩展事件族：`agent-preset/selected`、`agent/inbox/spliced`。
//!
//! 主要用途：preset 选择（最后写入者胜）与消息 inbox 的 splice 增量两组事件的 data 类型。
//! 为什么需要：这两组是 UI 侧「当前 preset / 待发消息队列」的状态来源，却由两个不同插件
//! 各自 `declare module` 注册（agent-presets 与 core/agent），单列文件便于按插件包核对同步；
//! 其中 `agent/inbox/spliced` 的 `inserted` 还是 dsh-sdk-client 判定 prompt 回执的依据。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dsh-sdk-client` 的 api.rs（用 `AgentInboxSpliced.inserted` 认回执）；
//!       `dshr-state` 的 fold（inbox 重放）。
//! 下接：`session_event/message.rs::Message`（inbox 载荷里的用户消息）。
//!
//! 官方对应：packages/preset/agent-presets/src/session.ts 的
//! `SessionEventMap['agent-preset/selected']`、packages/core/agent/src/types.ts 的
//! `SessionEventMap['agent/inbox/spliced']`。
use serde::{Deserialize, Serialize};

use crate::session_event::message::Message;

/// `agent-preset/selected` 的 data：会话选中的 agent preset id（最后写入者胜）。
/// 官方：packages/preset/agent-presets/src/session.ts 的 SessionEventMap['agent-preset/selected']
/// 用在 preset 变更事件（重建时取最后一个，否则回退创建头 agentPreset）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPresetSelectedData {
    /// preset id。
    pub agent_preset: String,
}

/// `agent/inbox/spliced` 的 data：消息 inbox 的规范化 splice 增量。
/// 官方：packages/core/agent/src/types.ts 的 SessionEventMap['agent/inbox/spliced']
/// 用在消息入队/移除事件（重放时逐条 apply，start + removedCount 语义与 Array.splice 一致）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxSplicedData {
    /// 目标 inbox：'next-turn' | 'next-step'。
    pub target: InboxTarget,
    /// splice 起始位置。
    pub start: u64,
    /// 删除条数（为 0 时字段缺省）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_count: Option<u64>,
    /// 插入的用户消息。
    pub inserted: Vec<Message>,
    /// 'canceled' = 因取消而丢弃的 pending 消息。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<InboxSpliceOutcome>,
}

/// inbox 目标（官方 InboxTarget）。
/// 用在 InboxSplicedData.target。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxTarget {
    NextTurn,
    NextStep,
}

/// splice 结果标记（官方 outcome，目前仅 'canceled'）。
/// 用在 InboxSplicedData.outcome。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxSpliceOutcome {
    Canceled,
}
