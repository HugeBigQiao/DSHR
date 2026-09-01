//! 会话标题扩展事件族：`session/title`、`session/title-llm-request`。
//! 官方：packages/session/session-title/src/index.ts、packages/session/session-title-llm/src/index.ts。
use serde::{Deserialize, Serialize};

use crate::session_event::message::Message;

/// `session/title` 的 data：会话标题整值快照（最后写入者胜）。
/// 官方：packages/session/session-title/src/index.ts 的 SessionEventMap['session/title']
/// 用在标题变更事件（UI 会话列表标题的数据源；source.kind==='user' 会 pin 住标题）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleData {
    pub title: String,
    /// 用于推导标题的人类 user/message seqs；用户手动重命名时为 []。
    pub message_seqs: Vec<u64>,
    pub source: SessionTitleSource,
}

/// 标题来源（官方 SessionTitleSource，按 kind 判别）。
/// 用在 SessionTitleData.source。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SessionTitleSource {
    Fallback,
    #[serde(rename_all = "camelCase")]
    Provider {
        provider: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<SessionTitleModelProvenance>,
    },
    User,
}

/// 标题模型的来源证明（官方 SessionTitleModelProvenance：{ provider, model }）。
/// 用在 SessionTitleSource::Provider 的 model 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTitleModelProvenance {
    pub provider: String,
    pub model: String,
}

/// `session/title-llm-request` 的 data：标题生成的 LLM 请求完整快照（dispatch 前记录）。
/// 官方：packages/session/session-title-llm/src/index.ts 的 SessionEventMap['session/title-llm-request']
/// 用在标题请求事件（与 session/title 配对使用，记录精确的辅助模型请求）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleLlmRequestData {
    /// 注册的 title provider 身份。
    pub title_provider: String,
    /// messages 中对应的精确人类 user/message seqs。
    pub message_seqs: Vec<u64>,
    pub route: SessionTitleModelProvenance,
    /// 精确的辅助 system prompt。
    pub system: String,
    pub messages: Vec<Message>,
    /// 精确输出 token 上限。
    pub max_tokens: u64,
}
