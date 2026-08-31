//! UI → state 的命令（用户在任务页能做的所有事）。
//!
//! 归属：任务页专用（setting 直调 config.rs、monitor 走查询，都不经过这里）。
//! 方向：dshr → dsh（UI 操作 → Engine 分发 → RuntimeTask → Bridge）。

/// UI → state 的命令。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// 添加 runtime：名字 + 可选工作区（决策 21：工作区可不设，一旦设置锁死）。
    Start { name: String, cwd: Option<String> },
    /// 补设工作区（仅限尚无工作区的 runtime；= 带新 cwd 重新 initialize，官方握手幂等）。
    SetWorkspace { runtime_id: String, cwd: String },
    /// 在指定 runtime 下开一个新会话。
    NewSession { runtime_id: String },
    /// 向会话发一条消息（简单版：纯文本 → text 块）。
    Send { session_id: String, text: String },
    /// runtime 改名（手动改名后停止自动命名跟随）。
    RenameRuntime { runtime_id: String, name: String },
    /// runtime 归档（历史保留，侧边栏隐藏）。
    ArchiveRuntime { runtime_id: String },
    /// runtime 彻底删除（决策 20：物理删 + 连坐全部数据）。
    DeleteRuntime { runtime_id: String },
    /// 会话改名。
    RenameSession { session_id: String, name: String },
    /// 会话归档（历史保留，侧边栏隐藏）。
    ArchiveSession { session_id: String },
    /// 会话彻底删除（决策 20：物理删 + 连坐数据）。
    DeleteSession { session_id: String },
    /// 读工作区目录（文件树，path 相对工作区，"" = 根）。
    ListWorkspace { runtime_id: String, path: String },
    /// 下载/更新 dsh 无头运行时（决策 23：npm install 到 workspace/dsh/）。
    FetchDsh,
    /// 退出（shutdown 全部 runtime）。
    Shutdown,
}
