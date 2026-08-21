//! `dshr-state`：中间人——UI 与 runtime 之间的总调度。
//!
//! 三层（见 DESIGN §9.5）：
//! - [`ui`]：① UI 对接层（UiEvent/Command 定义 + AppState 收发，UI 保持薄）
//! - [`core`]：② 处理层（config/store/session/transcode，唯一"翻译官"）
//! - [`bridge`]：③ runtime 对接层（state 内唯一 import dshr-runtime 的地方）

pub mod bridge;
pub mod core;
pub mod ui;

pub use ui::AppState;

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
