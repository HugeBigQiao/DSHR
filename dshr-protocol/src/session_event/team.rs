//! 团队扩展事件族：`team/member`、`team/message/queued`、`team/message/delivered`、`team/task`。
//! 官方：packages/experimental/agent-team/src/types.ts（Agent Teams，rc.8 新增）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

/// `team/member` 的 data：团队成员整快照（最后写入者胜，无增量）。
/// 官方：packages/experimental/agent-team/src/types.ts 的 SessionEventMap['team/member']
/// 用在成员生命周期变化事件（只存储在 Team Lead Session 中）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberData {
    pub version: u32,
    /// 根 Session 的 id。
    pub team_id: String,
    pub member: TeamMemberSnapshot,
}

/// 团队成员快照（官方 TeamMemberSnapshot）。
/// 用在 TeamMemberData.member。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberSnapshot {
    /// 成员自己的 Session id。
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: String,
    pub context: TeamMemberContext,
    pub phase: TeamMemberPhase,
    /// 仅 phase == 'failed' 时有值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 成员上下文（官方 'fresh' | 'fork'）。
/// 用在 TeamMemberSnapshot.context。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamMemberContext {
    Fresh,
    Fork,
}

/// 成员阶段（官方 'provisioning' | 'active' | 'failed'）。
/// 用在 TeamMemberSnapshot.phase。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamMemberPhase {
    Provisioning,
    Active,
    Failed,
}

/// `team/message/queued` 的 data：团队消息入队（投递前追加）。
/// 官方：packages/experimental/agent-team/src/types.ts 的 SessionEventMap['team/message/queued']
/// 用在消息入队事件（'wakeup' 会唤醒目标成员，'quiet' 不打断）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessageQueuedData {
    pub version: u32,
    pub team_id: String,
    pub message: TeamMessageSnapshot,
}

/// 团队消息快照（官方 TeamMessageSnapshot）。
/// 用在 TeamMessageQueuedData.message。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessageSnapshot {
    pub id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub target_id: String,
    pub delivery: TeamDelivery,
    pub content: Vec<ContentBlock>,
}

/// 投递方式（官方 'quiet' | 'wakeup'）。
/// 用在 TeamMessageSnapshot.delivery。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamDelivery {
    Quiet,
    Wakeup,
}

/// `team/message/delivered` 的 data：目标 Session 已记录该消息的确认。
/// 官方：packages/experimental/agent-team/src/types.ts 的 SessionEventMap['team/message/delivered']
/// 用在送达确认事件（不含内容，需用 messageId 关联 queued 事件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessageDeliveredData {
    pub version: u32,
    pub team_id: String,
    pub message_id: String,
    pub target_id: String,
}

/// `team/task` 的 data：团队任务整快照（每变更必出现）。
/// 官方：packages/experimental/agent-team/src/types.ts 的 SessionEventMap['team/task']
/// 用在任务变更事件（revision 用于 CAS 冲突检测）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTaskData {
    pub version: u32,
    pub team_id: String,
    pub task: TeamTaskSnapshot,
}

/// 团队任务快照（官方 TeamTaskSnapshot）。
/// 用在 TeamTaskData.task。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTaskSnapshot {
    pub id: String,
    /// 每次变更 +1（CAS 语义）。
    pub revision: u32,
    pub subject: String,
    pub description: String,
    pub status: TeamTaskStatus,
    /// 认领/指派后才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    /// 恒存在，可为空数组。
    pub blocked_by: Vec<String>,
    /// 恒存在，可为空数组。
    pub write_scopes: Vec<String>,
}

/// 任务状态（官方 'pending' | 'in_progress' | 'completed' | 'deleted'）。
/// 用在 TeamTaskSnapshot.status（注意 wire 是 snake_case：in_progress）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}
