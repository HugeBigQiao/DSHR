//! ① UI 对接层：定义"UI 看到的形状"，不碰协议、不碰数据库。
//!
//! - [`event`]：runtime → UI 的事件（UiEvent）
//! - [`command`]：UI → state 的命令（Command）
//! - [`app`]：AppState（把命令接进来 → 调 core → 结果转 UiEvent 回给 UI）

pub mod app;
pub mod command;
pub mod event;

pub use app::AppState;
pub use command::Command;
pub use event::{UiEvent, UiMessage, UiStatus, UiToolUse};
