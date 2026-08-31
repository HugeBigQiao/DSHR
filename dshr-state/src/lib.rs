//! `dshr-state`：中间人——UI 与 runtime 之间的总调度。
//!
//! 分层（决策 22，按页对应 UI）：
//! - [`task`]：任务页运行时（Command/UiEvent/AppState/Engine/Bridge/文件树/48 事件处理）
//! - [`core`]：处理层（config/store/session/transcode，唯一"翻译官"，无进程/UI 概念）
//! - [`monitor`]：监控页查询聚合层（对 dshr-data 的 read 结果加工）
//!
//! setting 无独立模块：配置是同步 JSON 读写，UI 直接调 [`core::config`]。

pub mod core;
pub mod monitor;
pub mod task;

pub use task::EventReceiver;
pub use task::app::AppState;
pub use task::bridge::{Bridge, RtInfo, SendOutcome};
pub use task::command::Command;
pub use task::events::{UiEvent, UiFileEntry, UiMessage, UiStatus, UiToolUse};

/// state 统一错误：吸收 runtime（协议/I/O）与 rusqlite（本地库）。
/// 分层的延续：protocol ParseError → runtime Error → 本类型 → UI 提示。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runtime 错误: {0}")]
    Runtime(#[from] dshr_runtime::error::Error),
    #[error("本地库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("尚未初始化")]
    NotStarted,
    #[error("{0}")]
    Other(String),
}
