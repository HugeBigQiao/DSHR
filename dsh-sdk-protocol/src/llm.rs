//! LLM 侧共享类型（官方 `packages/llm/llm/src/types.ts`）。
//!
//! 被 `session_event/` 各事件族引用（assistant/chunk 的 StreamChunk、
//! assistant/message 的 TokenUsage、turn/end 的 LlmFailure 等）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

/// 一次模型调用的 token 明细。
/// 官方：packages/llm/llm/src/types.ts 的 TokenUsage
/// 用在 assistant/message 的 data.usage 与 StreamChunk 的 usage 变体（监管面板核心）。
/// 注意：计数不相交——inputTokens 不含缓存，计费 = input + cacheRead + cacheWrite。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 整次调用的精确总 token（含聚合 prompt+输出；adapter 无权威值时省略）。
    /// 官方：packages/llm/llm/src/types.ts 的 TokenUsage.totalTokens（L135-147）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

/// 模型为什么停止输出。
/// 官方：packages/llm/llm/src/types.ts 的 FinishReasonMap
/// 用在 StreamChunk 的 finish 变体（wire 上是 {kind:...} 对象）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    Aborted { failure: LlmFailure },
    Error { failure: LlmFailure },
}

/// 流式输出的一块（token 级回放保真）。
/// 官方：packages/llm/llm/src/types.ts 的 StreamChunk
/// 用在 assistant/chunk 事件的 data.chunk。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StreamChunk {
    #[serde(rename_all = "camelCase")]
    BlockStart {
        index: u32,
        // 官方是 ContentBlockType 联合，先用 String。
        block_type: String,
    },
    TextDelta {
        index: u32,
        text: String,
    },
    ReasoningDelta {
        index: u32,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallDelta {
        index: u32,
        /// 官方 branded CallId，先用 String。
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments_delta: String,
    },
    BlockEnd {
        index: u32,
        block: ContentBlock,
    },
    Usage {
        usage: TokenUsage,
    },
    #[serde(rename_all = "camelCase")]
    Finish {
        reason: FinishReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        replay_state: Option<serde_json::Value>,
    },
}

/// 结构化失败（provider/transport 错误事实）。
/// 官方：packages/llm/llm/src/types.ts 的 LlmFailure
/// 用在 turn/end 的 data.error 与 FinishReason 的 failure。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmFailure {
    pub message: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}
