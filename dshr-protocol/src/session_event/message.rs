//! 消息类事件族。
//!
//! 对应官方 `SessionEventMap` 中 `user/message`、`assistant/chunk`、
//! `assistant/message` 三组；消息本体类型来自官方 `llm/llm/src/message.ts`。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

/// 消息来源（谁生产的这条消息）。
/// 官方：llm/llm/src/message.ts 的 MessageSourceMap
/// 用在 Message.source（wire 上是 {kind:...} 对象）。
/// 简化：官方还有 contextForm/provenance 等扩展字段，反序列化时自动忽略。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MessageSource {
    User,
    Plugin {
        plugin: String,
    },
    Model,
    #[serde(rename_all = "camelCase")]
    Tool {
        // 官方 branded CallId，先用 String。
        call_id: String,
    },
}

/// 消息角色。
/// 官方：llm/llm/src/message.ts 的 Message.role
/// 用在 Message.role。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// 一条不可变消息（官方三个子类的合并形态）。
/// 官方：llm/llm/src/message.ts 的 Message + UserMessage/AssistantMessage/ToolResultMessage
/// 用在 user/message 的 data、assistant/message 的 data.message、tool/result 的 data.message。
/// 简化：官方按 role/source 约束拆三个子类，wire 形状相同，这里合并为一个结构体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// 官方 branded MessageId，先用 String。
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    pub source: MessageSource,
}

/// `user/message` 的 data：一条用户消息（data 就是 Message 本身）。
/// 官方：core/session/src/types.ts 的 SessionEventMap['user/message']
/// 用在用户消息进入会话的事件。
pub type UserMessageData = Message;

/// `assistant/chunk` 的 data：原始流式块（token 级回放保真）。
/// 官方：core/session/src/types.ts 的 SessionEventMap['assistant/chunk']
/// 用在助手流式输出的每个 chunk 事件（聊天流式渲染的数据源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantChunkData {
    pub turn: u64,
    pub step: u64,
    pub chunk: crate::llm::StreamChunk,
}

/// `assistant/message` 的 data：组装好的助手消息 + 该步 token 账目。
/// 官方：core/session/src/types.ts 的 SessionEventMap['assistant/message']
/// 用在助手完整消息事件（监管面板 token 明细的数据源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessageData {
    pub turn: u64,
    pub step: u64,
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::llm::TokenUsage>,
}
