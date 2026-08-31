//! `dshr-data`：dshr 的本地数据层（加工索引库）。
//!
//! 职责：把"观察到的一切"加工成可查询的本地记录，供监管面板/离线算账/历史浏览使用。
//! 原则：**官方文件（jsonl.zstd / sqlite）是源数据，本库是加工索引**——
//! 不重复存原始会话日志，只存加工结果 + 配置 + 操作日志。
//!
//! 分层：
//! - [`schema`]：建表（以 `dshr/TABLES.csv` 为单源）
//! - [`write`]：写入（append-only，runtimes 表例外允许 update）
//! - [`read`]：读取（监管面板/历史浏览的查询入口）

pub mod read;
pub mod schema;
pub mod write;

/// 打开（或创建）dshr 本地数据库，并确保 schema 已建好。
///
/// 接收：数据库文件路径（如 `~/.local/share/dshr/dshr.db`）。
/// 处理：打开连接 → 执行幂等建表（IF NOT EXISTS，重复调用安全）。
/// 生成：一个可用的 rusqlite 连接（由调用方持有，进程内单例）。
pub fn open(path: &str) -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    schema::init_schema(&conn)?;
    Ok(conn)
}

/// 打开一个内存库（测试用）：同样的建表流程，不落盘。
pub fn open_in_memory() -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_in_memory()?;
    schema::init_schema(&conn)?;
    Ok(conn)
}
