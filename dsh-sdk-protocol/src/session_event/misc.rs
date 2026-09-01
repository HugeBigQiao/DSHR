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
