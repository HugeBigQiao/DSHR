//! UI → state 的命令（用户在界面上能做的所有事）。
//!
//! 简单版范围：添加/删除 runtime、runtime 内开会话、发消息、改名、退出。
//! 工作区锁死（DESIGN 决策 15）：cwd 只在 Start 时设置，之后只读。

/// UI → state 的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// 添加一个 runtime：名字 + 工作区路径（spawn + initialize，cwd 从此锁死）。
    Start { name: String, cwd: String },
    /// 在指定 runtime 下开一个新会话。
    NewSession { runtime_id: String },
    /// 向会话发一条消息（简单版：纯文本 → text 块）。
    Send { session_id: String, text: String },
    /// runtime 改名（runtimes 表 update 入口）。
    RenameRuntime { runtime_id: String, name: String },
    /// 删除 runtime（archive 标记 + kill 进程，数据保留可查）。
    ArchiveRuntime { runtime_id: String },
    /// 退出（shutdown 全部 runtime）。
    Shutdown,
}
