//! `dshr-data`：dshr 的本地数据层（加工库）。
//!
//! 职责：把"观察到的一切"加工成可查询的本地记录，供监管面板/离线算账/历史浏览使用。
//! 原则：**官方文件（jsonl.zstd / sqlite）是源数据，本库是加工索引**——
//! 不重复存原始会话日志，只存加工结果 + 配置 + 操作日志。
//!
//! 规划中的表：
//! - `sessions`：会话元数据（id、cwd、父子血缘 → 会话树）
//! - `events`：事件索引（session_id, seq, type, time, payload → 搜索/回放）
//! - `usage`：token 账务（input/output/cache/reasoning → 计费）
//! - `accounting`：离线算账结果（token × 定价表）
//! - `config`：dshr 配置（runtime 版本、定价表、插件配置副本）
//! - `audit_log`：操作日志（runtime 启停、请求记录 —— 官方没有的数据）

/// 打开（或创建）dshr 本地数据库。
pub fn open(path: &str) -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    // TODO(3e)：初始化 schema（sessions/events/usage/accounting/config/audit_log）
    Ok(conn)
}
