//! LLM 重试扩展事件族：`llm/retry`、`llm/retry-started`。
//! 官方：packages/llm/llm-retry/src/types.ts。
use serde::{Deserialize, Serialize};

use crate::llm::LlmFailure;

/// `llm/retry` 的 data：一次已排程的 LLM 重试（按 mode 判别的联合）。
/// 官方：packages/llm/llm-retry/src/types.ts 的 SessionEventMap['llm/retry']
/// 用在重试排程事件（等待开始前写入；对离线算账重要：重试 = 额外 token 消耗）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum LlmRetryData {
    /// 有界重试（可重试错误码 + maxRetries）。
    #[serde(rename_all = "camelCase")]
    Normal {
        /// 同一 request-step 重试链的所有尝试共享。
        retry_id: String,
        turn: u64,
        step: u64,
        provider: String,
        /// 路由这次重试的策略配置的完整 JSON 指纹。
        policy_key: String,
        /// 第几次重试（0 起）。
        retry: u32,
        max_retries: u32,
        delay_ms: u64,
        failure: LlmFailure,
    },
    /// 无界重试（无 maxRetries）。
    #[serde(rename_all = "camelCase")]
    Always {
        retry_id: String,
        turn: u64,
        step: u64,
        provider: String,
        policy_key: String,
        retry: u32,
        delay_ms: u64,
        failure: LlmFailure,
    },
}

/// `llm/retry-started` 的 data：重试等待结束后、下一次请求尝试开始前的过渡记录。
/// 官方：packages/llm/llm-retry/src/types.ts 的 SessionEventMap['llm/retry-started']
/// 用在重试真正开始事件（只有定位字段，无 provider/failure）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRetryStartedData {
    pub retry_id: String,
    pub turn: u64,
    pub step: u64,
    pub retry: u32,
}
