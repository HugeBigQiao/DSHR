//! LLM 侧共享类型（官方 `packages/llm/llm/src/types.ts`）。
//!
//! 主要用途：定义「一次模型调用」相关的公共类型——token 账目（`TokenUsage`）、
//! 紧凑流记录与展开形态（`AssistantStreamRecord` / `TimedStreamChunk` / `StreamChunk`）、
//! 停止原因（`FinishReason`）与结构化失败（`LlmFailure`），并提供 `expand()` 展开实现。
//! 为什么需要：这些类型被消息、turn 结束、重试等多个事件族**横向共享**，任何一族
//! 单独持有都会造成重复定义与「同一字段两种形状」。单列一个文件也把「官方 llm 包的
//! 变更面」隔离出来：llm/types.ts 改动时只需审这一个文件 + `session_event/` 的引用点。
//! 上接：`session_event/message.rs`（usage/stream）、`session_event/turn.rs`
//!       （`TurnEndReason::Error` 的 LlmFailure）、`session_event/retry.rs`（failure）、
//!       `session_event/compaction.rs`（summary 的 usage）；`dshr-state` 的统计/fold 层。
//! 下接：`content_block::ContentBlock`（`StreamChunk::BlockEnd` 的载荷）。
//!
//! 官方对应：`packages/llm/llm/src/types.ts` 的 `TokenUsage` / `FinishReasonMap` /
//! `StreamChunk` / `LlmFailure` / `IMAGE_OFFLOAD_REQUIRED_CODE`，以及
//! `packages/llm/llm/src/assistant-stream.ts` 的 `AssistantStreamRecord`。
//!
//! 被 `session_event/` 各事件族引用（assistant/attempt 的 AssistantStreamRecord、
//! assistant/message 的 TokenUsage、turn/end 的 LlmFailure 等）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

/// 一次模型调用的 token 明细。
///
/// 为什么需要：计费与容量判断的唯一数据源；六桶拆分（input/output/total/cacheRead/
/// cacheWrite/reasoning）必须保持官方语义，否则算出来的账与官方对不上。
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
///
/// 为什么需要：紧凑记录里的时间是相对量（`time0` + `dt` 差分），统计（首 token 延迟、
/// 单 chunk 间隔）必须还原成绝对时间才有意义；`TimedStreamChunk` 就是 `expand()` 的
/// 输出单元，只服务于统计/回放，不参与持久化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedStreamChunk {
    pub time: u64,
    pub chunk: StreamChunk,
}

impl AssistantStreamRecord {
    /// 展开成原始带时间戳的 chunk 序列（dshr 用于统计/回放；不参与持久化）。
    ///
    /// # 返回
    /// 按记录内顺序还原的 `TimedStreamChunk` 列表：`TextChunks` / `ReasoningChunks` /
    /// `ToolCallChunks` 会按 `dt`（相对前一个 chunk 的毫秒差）累加出每个 chunk 的绝对时间
    /// `time0 + Σdt`；`Chunk` 变体原样返回一条。`texts`/`args` 为空时返回空列表。
    ///
    /// # 注意
    /// `dt` 比元素数少 1（首个元素的时间就是 `time0`），缺项按 0 处理；时间累加用
    /// `saturating_add`，溢出时饱和而不是 panic（日志可能是坏数据）。
    ///
    /// 为什么需要：官方为了省空间把逐 chunk 压成「首时间 + 相对间隔 + 文本数组」，
    /// dshr 要做首 token 延迟/时长/字符数统计就必须能还原成等价序列——这是唯一还原点。
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
    /// 当 `code == "IMAGE_OFFLOAD_REQUIRED"` 时：该路由还需要卸载多少个**最旧的**
    /// 保留图片出现位置，同一个请求才能装进它的精确字节核算。插件据此记录一次
    /// `image/offload` 事件并重试该步。
    /// 官方 0.1.7-rc.2 新增（LlmFailure.offloadImages）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offload_images: Option<u64>,
}

/// 图片卸载被要求的失败码（官方 LlmFailure.code 取值，导出给消费方比较用）。
///
/// 为什么需要：官方把这个码留在字符串里（没有枚举），消费方（插件/桌面端）需要与
/// `LlmFailure.code` 做等值比较来决定「记录一次 image/offload 并重试」；
/// 导出常量可避免各处手写字面量写错。
/// 官方：packages/llm/llm/src/types.ts 的 IMAGE_OFFLOAD_REQUIRED_CODE。
pub const IMAGE_OFFLOAD_REQUIRED_CODE: &str = "IMAGE_OFFLOAD_REQUIRED";
