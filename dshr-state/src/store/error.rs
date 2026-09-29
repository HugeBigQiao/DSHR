//! store 的错误类型。
//!
//! 主要用途：`Store` 的所有公开方法返回 [`Result`]，本文件的 `StoreError` 是唯一错误类型。
//! 为什么需要：落库失败有三种**不同来源**（sqlite 自身、目录/文件的 I/O、以及「快照不合法」
//! 这种入参问题）。调用方（engine）对它们的处理是同一种——打一行 stderr 后继续跑，
//! 不打断会话——所以合成一个类型，而不是让调用方 match 三个错误家族。
//! 上接：`store.rs`（`Store::*`）、`store/write.rs`（内部写实现）。
//! 下接：`rusqlite`、`std::io`。
//! 官方对应：无（dshr 自己的持久化细节）。
use std::fmt;

/// store 错误：sqlite 错误 / 目录文件 I/O / 入参不合法（三源合一，调用方一个类型处理）。
///
/// 为什么需要：见文件级说明——三种来源、一种处理策略。用 `thiserror`-风格的手写 `Display`
/// 而非引入依赖，是因为这里只有三个变体且不需要 `#[from]` 之外的便利。
#[derive(Debug)]
pub enum StoreError {
    /// sqlite 层错误（建表、事务、语句执行）。
    Sqlite(rusqlite::Error),
    /// 建目录或打开库文件失败。
    Io(std::io::Error),
    /// 快照缺必要字段（如 session_id 为空——快照未接任何通知）。
    InvalidSnapshot(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Sqlite(e) => write!(f, "sqlite 错误: {e}"),
            StoreError::Io(e) => write!(f, "store I/O 错误: {e}"),
            StoreError::InvalidSnapshot(m) => write!(f, "快照不合法: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// store 统一结果类型。
pub type Result<T> = std::result::Result<T, StoreError>;
