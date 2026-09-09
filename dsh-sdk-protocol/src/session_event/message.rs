//! 消息类事件族。
//!
//! 对应官方 `SessionEventMap` 中 `user/message`、`system/message`、
//! `assistant/message`、`assistant/attempt`；消息本体类型来自官方 `packages/llm/llm/src/message.ts`。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

pub use super::message_source::MessageSource;

/// 消息角色。
/// 官方：packages/llm/llm/src/message.ts 的 Message.role
/// 用在 Message.role。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::session_event::SessionEvent;

    #[test]
    fn system_message_event_roundtrips() {
        let wire = json!({
            "type": "system/message",
            "seq": 1,
            "time": 10,
            "data": {
                "turn": 1,
                "step": 1,
                "message": {
                    "id": "sys-1",
                    "role": "system",
                    "content": [{"type": "text", "text": "prompt"}],
                    "source": {"kind": "plugin", "plugin": "system-prompt"}
                }
            }
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("system/message should parse");
        match &event {
            SessionEvent::SystemMessage { data, .. } => {
                assert_eq!(data.turn, 1);
                assert_eq!(data.step, 1);
            }
            other => panic!("expected SystemMessage, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }

    #[test]
    fn assistant_attempt_event_roundtrips() {
        let wire = json!({
            "type": "assistant/attempt",
            "seq": 2,
            "time": 20,
            "data": {
                "turn": 1,
                "step": 1,
                "stream": []
            }
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("assistant/attempt should parse");
        match &event {
            SessionEvent::AssistantAttempt { data, .. } => {
                assert_eq!(data.stream.as_ref().map(Vec::len), Some(0));
            }
            other => panic!("expected AssistantAttempt, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }
}
