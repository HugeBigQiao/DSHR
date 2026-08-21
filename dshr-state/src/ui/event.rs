//! runtime → UI 的事件（state 整理后的形状，UI 只渲染这些）。
//!
//! 转接原则（DESIGN §9.5）：core/transcode 把协议类型翻译成这里的 UiEvent，
//! UI 不 import protocol 的任何类型，保持薄。
//! 会话级事件带 runtime_id + session_id（UI 侧边栏树 + 按会话过滤渲染）。

use dshr_protocol::llm::TokenUsage;
use dshr_protocol::session_event::message::MessageRole;

/// state → UI 的完整事件流（简单版就这几族，将来按需加）。
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// 连接状态变化（runtime 级；首次到达时 UI 据此创建侧边栏节点）。
    Status {
        runtime_id: String,
        status: UiStatus,
        /// 显示名（首次创建节点用）。
        name: String,
        /// 工作区路径（只读展示）。
        workspace: String,
    },
    /// 会话已创建（prompt 懒创建后回报）。
    SessionCreated {
        runtime_id: String,
        session_id: String,
    },
    /// 会话标题更新（session/title）。
    Title {
        runtime_id: String,
        session_id: String,
        title: String,
    },
    /// 一条完整消息（user/assistant；简单版等 assistant/message 组装完再渲染，流式 v2）。
    Message {
        runtime_id: String,
        session_id: String,
        msg: UiMessage,
    },
    /// 工具调用摘要行（tool/call + tool/result 配对）。
    ToolUse {
        runtime_id: String,
        session_id: String,
        tool: UiToolUse,
    },
    /// 一轮结束（turn/end，带 token 汇总）。
    TurnEnd {
        runtime_id: String,
        session_id: String,
        turn: u64,
        reason: String,
        usage: Option<TokenUsage>,
    },
    /// runtime stderr 行（调试面板）。
    Log {
        runtime_id: String,
        level: String,
        message: String,
    },
    /// 全局错误（spawn 失败等）。
    Error(String),
}

/// 连接状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiStatus {
    /// 进程已拉起，initialize 还没完成。
    Connecting,
    /// initialize 完成，可以发消息。
    Ready,
    /// 已关闭。
    Closed,
}

/// 一条完整消息（UI 渲染聊天区的原料）。
#[derive(Debug, Clone, PartialEq)]
pub struct UiMessage {
    pub role: MessageRole,
    /// text 块拼接（assistant 时不含 reasoning）。
    pub text: String,
    /// reasoning 块拼接（assistant 专属，user 为 None）。
    pub reasoning: Option<String>,
    /// 事件 time（dsh 侧 epoch ms）。
    pub time: u64,
    /// 事件 seq（排序键）。
    pub seq: u64,
}

/// 工具调用摘要（监管命令视图的一行）。
#[derive(Debug, Clone, PartialEq)]
pub struct UiToolUse {
    pub name: String,
    /// tool/call 的原始参数 JSON。
    pub arguments: Option<String>,
    /// tool/result 的文本摘要（content 里的 text 块拼接）。
    pub result: Option<String>,
    pub is_error: bool,
    /// result.time - call.time（state 配对算）。
    pub duration_ms: Option<i64>,
    /// 工具私有载荷（如 fs 工具的 diff），原样 JSON。
    pub meta: Option<serde_json::Value>,
    pub seq: u64,
}
