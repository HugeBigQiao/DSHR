//! UI 侧视图模型（纯数据结构，无行为）。

use dshr_state::ui::UiStatus;

/// 侧边栏里的一个 runtime（渲染树）。
pub struct RtView {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub status: UiStatus,
    pub sessions: Vec<SessionView>,
}

/// 侧边栏里的一个会话。
pub struct SessionView {
    pub id: String,
    pub title: String,
}

/// 聊天区的一行（简单版：消息 / 工具摘要 / 轮结束 / 信息）。
pub enum MsgView {
    User(String),
    Assistant {
        text: String,
        reasoning: Option<String>,
    },
    Tool(String),
    TurnEnd(String),
    Info(String),
}
