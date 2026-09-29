//! `subagent/catalog`：父 Session 记录直接子 agent 的目录事实。
//!
//! 主要用途：给出 `SubagentCatalogData`——按 mode（one-shot / continuable）判别的
//! 完整目录快照（子会话 id、创建时间、label）。
//! 为什么需要：它是父会话侧「我生过哪些子 agent」的**权威目录**（与运行期的
//! `subagent.started` 通知不同，后者只在当前进程内有效），UI 的会话树重建要靠它；
//! 与 descriptor.rs（子代理组成声明）是同一域的两件事，故分开成文件。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（会话树）与 store（sessions.parent 列）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/subagent/subagent/src/catalog.ts 的 `SessionEventMap['subagent/catalog']`
//!（SUBAGENT_CATALOG_VERSION = 0）。
use serde::{Deserialize, Serialize};

/// `subagent/catalog` 的 data：按 mode 判别的完整目录事实。
///
/// 为什么需要：官方用 mode 判别两种目录条目而不是可空字段，因为 one-shot 子代理
/// 没有可续跑组成（label 可空），与 continuable（label 必填、可冷恢复）在语义上不同类；
/// 枚举化能让消费方在编译期被迫处理两者。
/// 官方：packages/subagent/subagent/src/catalog.ts 的 `SessionEventMap['subagent/catalog']`
///（SUBAGENT_CATALOG_VERSION = 0，version 字段逐字校验）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum SubagentCatalogData {
    /// 不可冷恢复的一次性子代理（label 可选）。
    #[serde(rename_all = "camelCase")]
    OneShot {
        version: u32,
        child_id: String,
        child_created_at: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// 可冷恢复的子代理（label 必填，用于持久枚举）。
    #[serde(rename_all = "camelCase")]
    Continuable {
        version: u32,
        child_id: String,
        child_created_at: u64,
        label: String,
    },
}
