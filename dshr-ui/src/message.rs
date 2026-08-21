//! UI 消息（iced 的 Message：用户操作 → update 的输入）。

/// UI 消息。
#[derive(Debug, Clone)]
pub enum Message {
    /// 100ms 轮询 tick（收 AppState 事件）。
    Tick,
    AddPressed,
    CancelAdd,
    NameChanged(String),
    PathChanged(String),
    ConfirmAdd,
    /// 指定 runtime 下开会话（参数 = runtime_id）。
    NewSession(String),
    /// 切换选中会话（参数 = session_id）。
    SelectSession(String),
    InputChanged(String),
    SendPressed,
    /// 归档 runtime（参数 = runtime_id）。
    ArchiveRuntime(String),
    /// 退出（shutdown 全部 runtime）。
    Close,
}
