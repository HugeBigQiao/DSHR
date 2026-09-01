//! 目标扩展事件族：`goal/change`。
//! 官方：packages/goal/goal/src/domain.ts（类型在 packages/goal/goal/src/types.ts）。
use serde::{Deserialize, Serialize};

/// `goal/change` 的 data：目标整值快照 + 墓碑（kind 恒为 "goal/change"，按 operation 判别）。
/// 官方：packages/goal/goal/src/types.ts 的 SessionEventMap['goal/change']
/// 用在目标变更事件（快照：每次变更携带完整 post-mutation 状态；clear：写墓碑）。
/// 简化：kind 字段反序列化时忽略（dshr 是观察者，不重放转发）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum GoalChangeData {
    #[serde(rename_all = "camelCase")]
    Create {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    #[serde(rename_all = "camelCase")]
    Edit {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    #[serde(rename_all = "camelCase")]
    Pause {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    #[serde(rename_all = "camelCase")]
    Resume {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    #[serde(rename_all = "camelCase")]
    Complete {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    #[serde(rename_all = "camelCase")]
    Block {
        version: u32,
        goal: GoalSnapshot,
        rounds_started: u32,
        created_at: u64,
        updated_at: u64,
    },
    /// clear 墓碑（无 goal 快照，只有引用与时间）。
    #[serde(rename_all = "camelCase")]
    Clear {
        version: u32,
        cleared: GoalRef,
        cleared_at: u64,
    },
}

/// 目标快照（官方 GoalSnapshot）。
/// 用在 GoalChangeData 的 goal 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSnapshot {
    pub id: String,
    /// 每次持久变更 +1，作为 compare-and-set 身份。
    pub revision: u32,
    pub objective: String,
    pub phase: GoalPhase,
    /// 仅 phase == 'blocked' 时存在。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<GoalBlockedReason>,
    pub max_goal_rounds: u32,
}

/// 目标阶段（官方 GoalPhase）。
/// 用在 GoalSnapshot.phase。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GoalPhase {
    Active,
    Paused,
    Blocked,
    Complete,
}

/// 目标阻塞原因（官方 blockedReason: { code, message }）。
/// 用在 GoalSnapshot.blocked_reason。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalBlockedReason {
    pub code: String,
    pub message: String,
}

/// 目标引用（官方 GoalRef：{ id, revision }）。
/// 用在 GoalChangeData::Clear 的 cleared 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalRef {
    pub id: String,
    pub revision: u32,
}
