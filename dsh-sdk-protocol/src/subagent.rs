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
