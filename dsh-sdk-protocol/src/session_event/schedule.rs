//! 调度扩展事件族：`schedule/change`。
//!
//! 主要用途：调度创建/删除/派发三组变更（按 operation 判别，带 version）与调度记录
//! （after/at/every 三种 kind）的 data 类型。
//! 为什么需要：官方把它设计成**版本化变更流而非快照**，重放时要校验完整的 session-local
//! 转换序列；这条语义与 goal（快照）相反，必须写在明处，否则容易被当成「取最后一条即可」。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（定时任务列表）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/schedule/schedule/src/types.ts 的
//! `SessionEventMap['schedule/change']`。
use serde::{Deserialize, Serialize};

/// `schedule/change` 的 data：调度变更（版本化变更流而非快照，按 operation 判别）。
/// 官方：packages/schedule/schedule/src/types.ts 的 SessionEventMap['schedule/change']
/// 用在调度创建/删除/派发事件（重放时校验完整 session-local 转换流）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum ScheduleChangeData {
    #[serde(rename_all = "camelCase")]
    Create {
        version: u32,
        schedule: ScheduleRecord,
    },
    #[serde(rename_all = "camelCase")]
    Delete { version: u32, id: String },
    /// dispatch：一次性只有 id；固定频率带 acceptedAt（决策时刻，跳过错过的 occurrence）。
    #[serde(rename_all = "camelCase")]
    Dispatch {
        version: u32,
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        accepted_at: Option<String>,
    },
}

/// 调度记录（按 kind 判别的联合）。
/// 用在 ScheduleChangeData::Create 的 schedule 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ScheduleRecord {
    #[serde(rename_all = "camelCase")]
    After {
        id: String,
        prompt: String,
        after_seconds: u64,
        /// RFC3339 UTC。
        scheduled_at: String,
    },
    #[serde(rename_all = "camelCase")]
    At {
        id: String,
        prompt: String,
        scheduled_at: String,
    },
    #[serde(rename_all = "camelCase")]
    Every {
        id: String,
        prompt: String,
        every_seconds: u64,
        scheduled_at: String,
    },
}
