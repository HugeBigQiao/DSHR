//! 子代理停止原因（`SubagentStopReason`）。
//!
//! 主要用途：给 `subagent.finished` 通知的 `stopReason` 字段提供类型化枚举。
//! 为什么需要：单独成文件是因为子代理域类型既不属于 session_event（事件在
//! `subagent/*` 族里，但 descriptor/catalog 是另一组端口），也不属于 requests；
//! 它被 notifications.rs 直接引用，并靠 `#[serde(other)]` 兜住官方 backends 的扩展值——
//! 这一「封闭枚举 + 宽容兜底」的写法是本 crate 的通用约定，值得有独立出处。
//! 上接：`notifications.rs`（SubagentFinishedNotification.stop_reason）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/subagent/subagent/src/types.ts 的 `SubagentStopReason`。
use serde::{Deserialize, Serialize};

/// 子代理运行结束的原因。
/// 官方：packages/subagent/subagent/src/types.ts 的 SubagentStopReason
/// 用在 subagent.finished 通知的 stopReason 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubagentStopReason {
    Completed,
    Aborted,
    Error,
    /// wire: "max-tokens"（kebab-case 自动转）
    MaxTokens,
    Refusal,
    /// 未知停止原因（官方标注 backends 会扩展 merge-extensible）。
    /// `#[serde(other)]` 兜住任何其他字符串；纯字符串无载荷，不丢数据。
    #[serde(other)]
    Unknown,
}
