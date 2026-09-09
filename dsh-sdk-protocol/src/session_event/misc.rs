//! 杂项事件族：`todo/write`、`feedback/record`。
//!
//! 对应官方 `SessionEventMap` 扩展（`todo/write` 0.1.2-alpha.x 起归 todo 插件所有，
//! 见 packages/todo/tool-todo/src/types.ts；`feedback/record` 由
//! `packages/feedback/command-feedback/src/index.ts` 注册）。
use serde::{Deserialize, Serialize};

/// `todo/write` 的 data：整表快照（最后写入者胜）。
/// 官方：packages/todo/tool-todo/src/types.ts 的 SessionEventMap['todo/write']
///      （0.1.2-alpha.x 前在 packages/core/session/src/types.ts，形状一致）
/// 用在 todo 列表变化的事件（仅日志 UI 状态，不参与历史重建）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoWriteData {
    pub todos: Vec<TodoItem>,
}

/// 一条待办（刻意最小化：无 id/优先级，整表替换）。
/// 官方：packages/core/session/src/types.ts 的 TodoItem
/// 用在 TodoWriteData.todos。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoItemStatus,
}

/// 待办生命周期状态。
/// 官方：packages/core/session/src/types.ts 的 TodoItem.status
/// 用在 TodoItem.status（注意 wire 是 snake_case：in_progress）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoItemStatus {
    Pending,
    InProgress,
    Completed,
}

/// `feedback/record` 的 data：一条用户反馈文本。
/// 官方：packages/feedback/command-feedback/src/index.ts 的 SessionEventMap['feedback/record']
/// 用在用户反馈事件（log-only，永不进模型上下文/历史；trim 后空文本会被拒绝写入）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackRecordData {
    pub text: String,
}

/// `feedback/message-put` 的 data：对一条助手消息写入或替换反馈。
/// 官方：packages/feedback/message-feedback/src/types.ts 的 MessageFeedbackPut
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackMessagePutData {
    pub session_id: String,
    pub item: FeedbackMessageItem,
}

/// `feedback/message-delete` 的 data：删除一条助手消息的反馈。
/// 官方：packages/feedback/message-feedback/src/types.ts 的 MessageFeedbackDelete
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackMessageDeleteData {
    pub session_id: String,
    pub message_id: String,
}

/// 消息反馈的完整当前值。
/// 官方：packages/feedback/message-feedback/src/types.ts 的 MessageFeedbackItem
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackMessageItem {
    pub message_id: String,
    pub rating: FeedbackRating,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub version: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// 反馈评级。
/// 官方：packages/feedback/message-feedback/src/types.ts 的 MessageFeedbackRating
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackRating {
    Positive,
    Negative,
}
