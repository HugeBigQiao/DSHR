//! bridge：UI 与 state/SDK 之间的数据桥（转接原则：UI 只消费 `model` 视图模型）。
//!
//! state 冻结期间用 `PlaceholderBridge`（内置演示数据，让 UI 先渲染出形）；
//! 接 dshr-state 后实现真实桥：多 runtime（每 runtime 一个 dsh 子进程）→ 事件流 → AppData。
//! TODO(bridge)：事件 → 视图模型的翻译器放 state 层（旧 DESIGN §9.5 的 core/transcode）。
use crate::model::{AppData, ChatState, MsgKind, MsgView, RuntimeView, SessionView, ToolView};

/// 占位桥：返回一份演示数据（两个 runtime + 各自会话 + 一轮完整对话）。
#[derive(Debug)]
pub struct PlaceholderBridge;

impl PlaceholderBridge {
    pub fn new() -> Self {
        Self
    }

    /// 当前数据快照（真实桥 = 事件流累积后的状态）。
    pub fn snapshot(&self) -> AppData {
        AppData {
            runtimes: vec![
                RuntimeView {
                    id: "rt-1".to_string(),
                    name: "示例 runtime A".to_string(),
                    expanded: true,
                    sessions: vec![
                        SessionView { id: "s-1".to_string(), title: "会话 1".to_string() },
                        SessionView { id: "s-2".to_string(), title: "会话 2".to_string() },
                    ],
                    selected_session: Some("s-1".to_string()),
                },
                RuntimeView {
                    id: "rt-2".to_string(),
                    name: "示例 runtime B".to_string(),
                    expanded: true,
                    sessions: vec![SessionView {
                        id: "s-3".to_string(),
                        title: "会话 1".to_string(),
                    }],
                    selected_session: Some("s-3".to_string()),
                },
            ],
            chat: ChatState {
                session_id: "s-1".to_string(),
                status: "idle".to_string(),
                stats: "deepseek-v4-flash · in 12 · out 34 · 1.2s".to_string(),
                messages: vec![
                    MsgView {
                        kind: MsgKind::User,
                        text: "写一个 Rust 冒泡排序".to_string(),
                        reasoning: None,
                        tool: None,
                        time_label: "10:02".to_string(),
                    },
                    MsgView {
                        kind: MsgKind::Reasoning,
                        text: String::new(),
                        reasoning: Some("用户要一个经典算法示例，直接给出实现并说明复杂度。".to_string()),
                        tool: None,
                        time_label: "10:02".to_string(),
                    },
                    MsgView {
                        kind: MsgKind::Tool,
                        text: String::new(),
                        reasoning: None,
                        tool: Some(ToolView {
                            name: "bash".to_string(),
                            duration_ms: 120,
                            summary: "exit 0（无输出）".to_string(),
                            is_error: false,
                            expanded: false,
                        }),
                        time_label: "10:02".to_string(),
                    },
                    MsgView {
                        kind: MsgKind::Assistant,
                        text: "```rust\nfn bubble_sort<T: Ord>(a: &mut [T]) {\n    for i in 0..a.len() {\n        for j in 0..a.len() - 1 - i {\n            if a[j] > a[j + 1] { a.swap(j, j + 1); }\n        }\n    }\n}\n```\n\n这是经典冒泡排序，平均复杂度 O(n²)。".to_string(),
                        reasoning: None,
                        tool: None,
                        time_label: "10:03".to_string(),
                    },
                ],
            },
        }
    }
}
