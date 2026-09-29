//! 消息类事件族。
//!
//! 主要用途：`user/message`、`developer/message`、`system/message`、`assistant/message`、
//! `assistant/attempt` 五组事件的 data 类型，以及它们共用的 `Message` / `MessageRole`。
//! 为什么需要：这五种是「什么进了模型上下文」的完整记录（surface 写入），是 UI 对话流的
//! 直接数据源，与工具/审批等旁路事件职责不同；`Message` 与角色枚举被 tool/result、
//! subagent 等多处引用，集中在此避免重复定义，也让官方 message.ts 的变更只影响一处。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `session_event/tool.rs`、`session_event/agent.rs`、`session_event/title.rs`
//!       （都复用 `Message`）；`dshr-state` 的 fold（消息流投影）。
//! 下接：`crate::content_block::ContentBlock`（消息内容块）、
//!       `crate::llm::{AssistantStreamRecord, TokenUsage}`（assistant/message 的流与账目）、
//!       `message_source::MessageSource`（消息来源，本文件再出口）。
//!
//! 官方对应：`SessionEventMap` 中 `user/message`、`developer/message`（0.1.7-rc.2 新增）、
//! `system/message`、`assistant/message`、`assistant/attempt`；
//! 消息本体类型来自官方 `packages/llm/llm/src/message.ts`。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

pub use super::message_source::MessageSource;

/// 消息角色。
/// 官方：packages/llm/llm/src/message.ts 的 MessageRoleMap
/// 用在 Message.role。`Developer` 为 0.1.7-rc.2 新增（`developer/message` 事件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    /// 兜底：官方 MessageRoleMap 是 merge-extensible（并已从 3 成员扩到 5），
    /// 未来新增 role 时不整体解析失败（与 TurnEndReason::Other 同款策略）。
    #[serde(other)]
    Other,
}

/// 一条不可变消息（官方三个子类的合并形态）。
/// 官方：packages/llm/llm/src/message.ts 的 Message + UserMessage/AssistantMessage/ToolResultMessage
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
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['user/message']
/// 用在用户消息进入会话的事件。
pub type UserMessageData = Message;

/// `developer/message` 的 data：在指定 turn/step 被接纳的增量 agent 会话变更。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['developer/message']
///     （0.1.7-rc.2 新增；content 里承载 tool-addition / tool-removal 块）
/// 用在工具声明中途增删的事件；`headerSeq` 指向定义这些新增工具的更早
/// request/header——**仅当 content 含 additions 时才必需**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperMessageData {
    pub turn: u64,
    pub step: u64,
    pub message: Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_seq: Option<u64>,
}

/// `system/message` 的 data：渲染后的系统提示消息。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['system/message']
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemMessageData {
    pub turn: u64,
    pub step: u64,
    pub message: Message,
}

/// `assistant/message` 的 data：组装好的助手消息 + 该步 token 账目 + 精确流记录。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['assistant/message']
/// 新日志必带 `stream`；这里用 Option 兼容旧日志缺字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessageData {
    pub turn: u64,
    pub step: u64,
    pub message: Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<Vec<crate::llm::AssistantStreamRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::llm::TokenUsage>,
    /// turn 中途取消时，本消息是已交付的文本/reasoning 前缀（wire: `interrupted: true`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
}

/// `assistant/attempt` 的 data：一次没有提交 surface 消息的模型尝试。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['assistant/attempt']
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantAttemptData {
    pub turn: u64,
    pub step: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<Vec<crate::llm::AssistantStreamRecord>>,
}
