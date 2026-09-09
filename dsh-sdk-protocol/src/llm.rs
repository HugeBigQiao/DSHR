//! LLM 侧共享类型（官方 `packages/llm/llm/src/types.ts`）。
//!
//! 被 `session_event/` 各事件族引用（assistant/attempt 的 AssistantStreamRecord、
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

/// 一次模型尝试的紧凑流记录（durable assistant stream record）。
/// 官方：packages/llm/llm/src/assistant-stream.ts 的 AssistantStreamRecord
/// 用在 assistant/message 与 assistant/attempt 的 `stream` 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantStreamRecord {
    #[serde(rename_all = "camelCase")]
    TextChunks {
        time0: u64,
        index: u32,
        dt: Vec<u64>,
        texts: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    ReasoningChunks {
        time0: u64,
        index: u32,
        dt: Vec<u64>,
        texts: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallChunks {
        time0: u64,
        index: u32,
        dt: Vec<u64>,
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        args: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Chunk { time: u64, chunk: StreamChunk },
}

/// 一条带原始时间戳的流 chunk（`AssistantStreamRecord` 展开后的形态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedStreamChunk {
    pub time: u64,
    pub chunk: StreamChunk,
}

impl AssistantStreamRecord {
    /// 展开成原始带时间戳的 chunk 序列（dshr 用于统计/回放；不参与持久化）。
    pub fn expand(&self) -> Vec<TimedStreamChunk> {
        match self {
            AssistantStreamRecord::TextChunks {
                time0,
                index,
                dt,
                texts,
            } => expand_deltas(*time0, *index, dt, texts, |index, text| {
                StreamChunk::TextDelta { index, text }
            }),
            AssistantStreamRecord::ReasoningChunks {
                time0,
                index,
                dt,
                texts,
            } => expand_deltas(*time0, *index, dt, texts, |index, text| {
                StreamChunk::ReasoningDelta { index, text }
            }),
            AssistantStreamRecord::ToolCallChunks {
                time0,
                index,
                dt,
                id,
                name,
                args,
            } => {
                let mut chunks = Vec::with_capacity(args.len());
                let mut time = *time0;
                for (offset, arguments_delta) in args.iter().enumerate() {
                    if offset > 0 {
                        time = time.saturating_add(dt.get(offset - 1).copied().unwrap_or(0));
                    }
                    chunks.push(TimedStreamChunk {
                        time,
                        chunk: StreamChunk::ToolCallDelta {
                            index: *index,
                            id: id.clone(),
                            name: name.clone(),
                            arguments_delta: arguments_delta.clone(),
                        },
                    });
                }
                chunks
            }
            AssistantStreamRecord::Chunk { time, chunk } => vec![TimedStreamChunk {
                time: *time,
                chunk: chunk.clone(),
            }],
        }
    }
}

/// 展开 text/reasoning 的 `(time0, index, dt, texts)` 紧凑记录。
fn expand_deltas(
    time0: u64,
    index: u32,
    dt: &[u64],
    texts: &[String],
    make: impl Fn(u32, String) -> StreamChunk,
) -> Vec<TimedStreamChunk> {
    let mut chunks = Vec::with_capacity(texts.len());
    let mut time = time0;
    for (offset, text) in texts.iter().enumerate() {
        if offset > 0 {
            time = time.saturating_add(dt.get(offset - 1).copied().unwrap_or(0));
        }
        chunks.push(TimedStreamChunk {
            time,
            chunk: make(index, text.clone()),
        });
    }
    chunks
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
/// 用在流记录中的原始 chunk。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_text_chunks_preserves_timestamps() {
        let record = AssistantStreamRecord::TextChunks {
            time0: 100,
            index: 0,
            dt: vec![10, 5],
            texts: vec!["a".into(), "b".into(), "c".into()],
        };
        let chunks = record.expand();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].time, 100);
        assert_eq!(chunks[1].time, 110);
        assert_eq!(chunks[2].time, 115);
        assert!(matches!(&chunks[1].chunk, StreamChunk::TextDelta { text, .. } if text == "b"));
    }

    #[test]
    fn expand_tool_call_chunks_rebuilds_deltas() {
        let record = AssistantStreamRecord::ToolCallChunks {
            time0: 200,
            index: 1,
            dt: vec![7],
            id: "call-1".into(),
            name: Some("read".into()),
            args: vec!["{\"path\"".into(), ":\"a\"}".into()],
        };
        let chunks = record.expand();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].time, 200);
        assert_eq!(chunks[1].time, 207);
        assert!(matches!(
            &chunks[1].chunk,
            StreamChunk::ToolCallDelta { id, arguments_delta, .. }
                if id == "call-1" && arguments_delta == ":\"a\"}"
        ));
    }
}
