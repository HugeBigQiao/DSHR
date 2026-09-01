//! 压缩类扩展事件族：`compaction/start`、`compaction/end`、`compaction/prune`、`compaction/summary`。
//! 官方：packages/compaction/compaction/src/types.ts。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;
use crate::llm::TokenUsage;

/// `compaction/start` 的 data：标记压缩开始（持有锁直到 compaction/end）。
/// 官方：packages/compaction/compaction/src/types.ts 的 SessionEventMap['compaction/start']
/// 用在压缩事务开始（turn 可空：null = turn 之间的独立手动事务）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionStartData {
    /// 一次 start/summary/end 事务共享的 id。
    pub compaction_id: String,
    /// 手动发起时的人命令。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_command_id: Option<String>,
    /// 编号的属主严格封闭在 open turn 内；null = 独立手动事务。
    pub turn: Option<u64>,
}

/// `compaction/end` 的 data：释放压缩锁（属主与 start 匹配）。
/// 官方：packages/compaction/compaction/src/types.ts 的 SessionEventMap['compaction/end']
/// 用在压缩事务结束（error 存在表示压缩未成功）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEndData {
    pub compaction_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_command_id: Option<String>,
    pub turn: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `compaction/prune` 的 data：一次"无模型剪枝替换"的影子价格。
/// 官方：packages/compaction/compaction/src/types.ts 的 SessionEventMap['compaction/prune']
/// 用在剪枝计量事件（契约：紧随其后的 surface replace 事件取其价格为代价）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPruneData {
    /// 被替换范围的首尾 surface 节点 seq（surface 位置跨度，start 可能大于 end）。
    pub shadowed_range: ShadowedRange,
    /// 所有被遮蔽 surface 节点的 seq。
    pub shadowed_seqs: Vec<u64>,
    /// token-meter 固定估算器算出的启发式价格。
    pub shadowed_token_count: u64,
}

/// `compaction/summary` 的 data：压缩总结（真正的 surface 替换由紧随的 user/message 事件完成）。
/// 官方：packages/compaction/compaction/src/types.ts 的 SessionEventMap['compaction/summary']
/// 用在压缩总结事件（summary 是后端产出的总结块；rawOutput + llmStreamCall 标识一次 LLM 调用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryData {
    pub compaction_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_command_id: Option<String>,
    /// 后端产出的总结内容块。
    pub summary: Vec<ContentBlock>,
    pub shadowed_range: ShadowedRange,
    pub shadowed_seqs: Vec<u64>,
    pub shadowed_token_count: u64,
    /// 写出总结的 provider 路由。
    pub provider: String,
    /// 写出总结的模型。
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// 完整 provider 输出（未标记模板/远程等总结器时为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Vec<ContentBlock>>,
    /// 唯一标识一次 ctx.llm.stream() 调用。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_stream_call: Option<bool>,
}

/// 被遮蔽的 surface 节点 seq 范围。
/// 用在 CompactionPruneData / CompactionSummaryData。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowedRange {
    pub start: u64,
    pub end: u64,
}
