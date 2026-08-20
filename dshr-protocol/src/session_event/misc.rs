//! 杂项事件族。
//!
//! 对应官方 `SessionEventMap` 中 `todo/write`（其余 `command/*`、`hook/*`、
//! `llm/retry` 等扩展在 3a 不 port，由 3b fallback 兜住）。
use serde::{Deserialize, Serialize};

/// `todo/write` 的 data：整表快照（最后写入者胜）。
/// 官方：core/session/src/types.ts 的 SessionEventMap['todo/write']
/// 用在 todo 列表变化的事件（仅日志 UI 状态，不参与历史重建）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoWriteData {
    pub todos: Vec<TodoItem>,
}

/// 一条待办（刻意最小化：无 id/优先级，整表替换）。
/// 官方：core/session/src/types.ts 的 TodoItem
/// 用在 TodoWriteData.todos。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoItemStatus,
}

/// 待办生命周期状态。
/// 官方：core/session/src/types.ts 的 TodoItem.status
/// 用在 TodoItem.status（注意 wire 是 snake_case：in_progress）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoItemStatus {
    Pending,
    InProgress,
    Completed,
}
