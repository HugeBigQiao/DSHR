//! runtime → UI 的事件（state 整理后的形状，UI 只渲染这些）。
//!
//! 转接原则（DESIGN §9.5）：core/transcode 把协议类型翻译成这里的 UiEvent，
//! UI 不 import protocol 的任何类型，保持薄。
//! 会话级事件带 runtime_id + session_id（UI 侧边栏树 + 按会话过滤渲染）。
//! 归属：任务页专用（setting/monitor 不消费事件流）。

use dshr_protocol::llm::TokenUsage;
use dshr_protocol::session_event::message::MessageRole;

/// 弹窗级错误（操作/全局：spawn 失败、设置工作区失败等）。
/// UI 收到后弹窗显示（[`UiEvent::Toast`]）。
#[macro_export]
macro_rules! toast {
    ($tx:expr, $($arg:tt)*) => {{
        let _ = $tx.send($crate::task::events::UiEvent::Toast(format!($($arg)*)));
    }};
}

/// 对话内错误（发送失败、通知解析失败等），UI 红色字体显示在对话流。
#[macro_export]
macro_rules! inline_err {
    ($tx:expr, $($arg:tt)*) => {{
        let _ = $tx.send($crate::task::events::UiEvent::InlineError(format!($($arg)*)));
    }};
}

/// state → UI 的完整事件流（简单版就这几族，将来按需加）。
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// 连接状态变化（runtime 级；首次到达时 UI 据此创建侧边栏节点）。
    Status {
        runtime_id: String,
        status: UiStatus,
        /// 显示名（首次创建节点用）。
        name: String,
        /// 工作区路径（只读展示；None = 尚未设置）。
        workspace: Option<String>,
    },
    /// runtime 显示名更新（手动改名 / 自动命名后同步侧边栏）。
    RuntimeRenamed { runtime_id: String, name: String },
    /// 会话已创建（prompt 懒创建后回报）。
    SessionCreated {
        runtime_id: String,
        session_id: String,
    },
    /// 会话已归档/删除（UI 从侧边栏移除该节点）。
    SessionRemoved {
        runtime_id: String,
        session_id: String,
    },
    /// 会话标题更新（session/title；自动命名钩子也在这）。
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
    /// 工作区目录内容（文件树刷新）。
    FileTree {
        runtime_id: String,
        /// 相对工作区的目录（"" = 根）。
        path: String,
        entries: Vec<UiFileEntry>,
    },
    /// dsh 下载进度行（首次安装/更新的弹窗显示）。
    FetchProgress(String),
    /// dsh 下载完成/失败（弹窗切换成结果态）。
    FetchDone { ok: bool, message: String },
    /// runtime stderr 行（调试面板）。
    Log {
        runtime_id: String,
        level: String,
        message: String,
    },
    /// 全局错误 → 弹窗（操作/启动失败等；区别于对话内错误）。
    Toast(String),
    /// 对话内错误 → 红色字体显示在对话流（发送失败等）。
    InlineError(String),
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

/// 文件树一项（工作区目录的直接子项）。
#[derive(Debug, Clone, PartialEq)]
pub struct UiFileEntry {
    pub name: String,
    pub is_dir: bool,
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
