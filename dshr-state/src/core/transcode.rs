//! 数据转接：一切形状转换的"翻译官"（DESIGN §9.5）。
//!
//! runtime → UI：`SessionEvent` → `UiEvent`（同时推进 [`SessionTracker`]）。
//! turn 生命周期的**状态推进**拆成 on_turn_start/on_turn_end（RuntimeTask 拿返回值落库），
//! [`event_to_ui`] 只负责"要渲染的东西"，两不混淆。

use dshr_protocol::content_block::ContentBlock;
use dshr_protocol::session_event::message::MessageRole;
use dshr_protocol::session_event::{SessionEvent, message::Message};

use crate::core::session::{SessionTracker, TurnFinalize};
use crate::task::events::{UiEvent, UiMessage, UiToolUse};

/// turn/start：推进状态并返回 turn_id。
/// 接收：归属 + turn 号 + 事件时间 + 状态机。
/// 生成：turn_id（`runtime_id-session_id-turn`）——调用方开 turns 行 + 回填 requests。
pub fn on_turn_start(
    runtime_id: &str,
    session_id: &str,
    turn: u64,
    time: i64,
    tracker: &mut SessionTracker,
) -> String {
    tracker.on_turn_start(runtime_id, session_id, turn, time)
}

/// turn/end：收行并取回填数据。
/// 接收：结束原因（事件 data.reason）+ 结束时间 + 状态机。
/// 生成：Option<(TurnFinalize, reason 字符串)>——调用方 finish_turn + 拼 UiEvent::TurnEnd。
pub fn on_turn_end(
    reason: &dshr_protocol::session_event::turn::TurnEndReason,
    time: i64,
    tracker: &mut SessionTracker,
) -> Option<(TurnFinalize, String)> {
    let fin = tracker.take_turn_finalize(time)?;
    let reason = turn_reason_str(reason).to_string();
    Some((fin, reason))
}

/// 把一条结构化事件转成给 UI 的事件（可能没有），并推进非 turn 状态。
/// 接收：来源（runtime_id/session_id）+ 事件 + 该会话的状态机。
/// 处理：消息类转 UiMessage 并缓存文本、工具类配对转 UiToolUse、标题转 Title，其余 None。
/// 生成：Option<UiEvent>（None = 只有落库价值，UI 不渲染）。
/// 注意：turn/start、turn/end 不在这里处理（见 on_turn_start/on_turn_end）。
pub fn event_to_ui(
    runtime_id: &str,
    session_id: &str,
    event: &SessionEvent,
    tracker: &mut SessionTracker,
) -> Option<UiEvent> {
    match event {
        SessionEvent::UserMessage { time, data, .. } => {
            let (text, _) = extract_text(&data.content);
            tracker.on_user_message(&text);
            Some(UiEvent::Message {
                runtime_id: runtime_id.to_string(),
                session_id: session_id.to_string(),
                msg: UiMessage {
                    role: MessageRole::User,
                    text,
                    reasoning: None,
                    time: *time,
                    seq: event.seq(),
                },
            })
        }
        SessionEvent::AssistantMessage { time, data, .. } => {
            let (text, reasoning) = extract_text(&data.message.content);
            tracker.on_assistant_message(&text);
            tracker.add_usage(data.usage.as_ref());
            Some(UiEvent::Message {
                runtime_id: runtime_id.to_string(),
                session_id: session_id.to_string(),
                msg: UiMessage {
                    role: MessageRole::Assistant,
                    text,
                    reasoning: (!reasoning.is_empty()).then_some(reasoning),
                    time: *time,
                    seq: event.seq(),
                },
            })
        }
        SessionEvent::ToolCall { time, data, .. } => {
            tracker.on_tool_call(&data.call_id, *time as i64, &data.name);
            None
        }
        SessionEvent::ToolResult { time, data, .. } => {
            let (name, duration_ms) = tracker
                .take_tool_start(&data.message.tool_call_id(), *time as i64)
                .unwrap_or_default();
            let (result, _) = extract_text(&data.message.content);
            let is_error = data.error.is_some() || data.message.is_error();
            Some(UiEvent::ToolUse {
                runtime_id: runtime_id.to_string(),
                session_id: session_id.to_string(),
                tool: UiToolUse {
                    name,
                    arguments: None,
                    result: (!result.is_empty()).then_some(result),
                    is_error,
                    duration_ms: Some(duration_ms),
                    meta: data.meta.clone(),
                    seq: event.seq(),
                },
            })
        }
        SessionEvent::SessionTitle { data, .. } => Some(UiEvent::Title {
            runtime_id: runtime_id.to_string(),
            session_id: session_id.to_string(),
            title: data.title.clone(),
        }),
        _ => None,
    }
}

/// turn 结束原因 → 简短字符串（UI 显示 + turns.reason 落库）。
pub fn turn_reason_str(reason: &dshr_protocol::session_event::turn::TurnEndReason) -> &'static str {
    use dshr_protocol::session_event::turn::TurnEndReason as R;
    match reason {
        R::Completed => "completed",
        R::Aborted { .. } => "aborted",
        R::Blocked => "blocked",
        R::Error { .. } => "error",
        R::MaxTokens => "max-tokens",
        R::Interrupted => "interrupted",
    }
}

/// 从消息内容里提取 (text 拼接, reasoning 拼接)。
/// 接收：ContentBlock 列表。
/// 处理：text 块 → text，reasoning 块 → reasoning，其余块忽略（v2 再补）。
/// 生成：两个拼接字符串。
fn extract_text(blocks: &[ContentBlock]) -> (String, String) {
    let mut text = String::new();
    let mut reasoning = String::new();
    for block in blocks {
        match block {
            ContentBlock::Text(b) => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&b.text);
            }
            ContentBlock::Reasoning(b) => {
                if !reasoning.is_empty() {
                    reasoning.push('\n');
                }
                reasoning.push_str(&b.text);
            }
            _ => {}
        }
    }
    (text, reasoning)
}

/// Message 的便捷访问（tool/result 的 message 是 ToolResultMessage）。
trait ToolMessageExt {
    fn tool_call_id(&self) -> String;
    fn is_error(&self) -> bool;
}

impl ToolMessageExt for Message {
    fn tool_call_id(&self) -> String {
        // 官方约定 tool/result 的 message.content 是单个 ToolResultBlock。
        self.content
            .iter()
            .find_map(|b| match b {
                ContentBlock::ToolResult(b) => Some(b.tool_call_id.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn is_error(&self) -> bool {
        self.content.iter().any(|b| match b {
            ContentBlock::ToolResult(b) => b.is_error == Some(true),
            _ => false,
        })
    }
}
