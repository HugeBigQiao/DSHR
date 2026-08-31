//! 读取层：监管面板/历史浏览的查询入口。
//!
//! 查询以"行"为单位返回，具体消费（聚合/算账/渲染）由调用方（state/ui）做。
use rusqlite::{Connection, Result, params};

/// 一个会话的摘要行。
pub struct SessionRow {
    pub id: String,
    pub runtime_id: String,
    pub cwd: String,
    pub parent_session_id: Option<String>,
    pub created_at: i64,
    pub status: Option<String>,
    pub state: String,
    pub last_seq: i64,
}

/// 一个事件行（lossless payload）。
pub struct EventRow {
    pub seq: i64,
    pub event_type: String,
    pub time: i64,
    pub turn: Option<i64>,
    pub step: Option<i64>,
    pub payload: String,
}

/// 一轮的摘要行（含 token 合计）。
pub struct TurnRow {
    pub turn_id: String,
    pub turn: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub reason: Option<String>,
    pub usage_input: Option<i64>,
    pub usage_output: Option<i64>,
    pub usage_cache_read: Option<i64>,
    pub usage_cache_write: Option<i64>,
    pub usage_reasoning: Option<i64>,
    pub user_text: Option<String>,
    pub assistant_text: Option<String>,
}

/// 一个工具调用行（监管命令视图）。
pub struct ToolCallRow {
    pub call_id: String,
    pub name: String,
    pub arguments: Option<String>,
    pub result_text: Option<String>,
    pub is_error: bool,
    pub duration_ms: Option<i64>,
    pub meta: Option<String>,
}

/// 一条 runtime 日志行（审计）。
pub struct LogRow {
    pub id: i64,
    pub time: i64,
    pub level: String,
    pub message: String,
}

/// 某 runtime 下的全部会话。
pub fn sessions_by_runtime(conn: &Connection, runtime_id: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, runtime_id, cwd, parent_session_id, created_at, status, state, last_seq
         FROM sessions WHERE runtime_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![runtime_id], |r| {
        Ok(SessionRow {
            id: r.get(0)?,
            runtime_id: r.get(1)?,
            cwd: r.get(2)?,
            parent_session_id: r.get(3)?,
            created_at: r.get(4)?,
            status: r.get(5)?,
            state: r.get(6)?,
            last_seq: r.get(7)?,
        })
    })?;
    rows.collect()
}

/// 某会话的全部事件（按 seq 升序）。
pub fn events_by_session(conn: &Connection, session_id: &str) -> Result<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT seq, type, time, turn, step, payload
         FROM events WHERE session_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(EventRow {
            seq: r.get(0)?,
            event_type: r.get(1)?,
            time: r.get(2)?,
            turn: r.get(3)?,
            step: r.get(4)?,
            payload: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// 某会话的全部轮。
pub fn turns_by_session(conn: &Connection, session_id: &str) -> Result<Vec<TurnRow>> {
    let mut stmt = conn.prepare(
        "SELECT turn_id, turn, started_at, ended_at, duration_ms, reason,
                usage_input, usage_output, usage_cache_read, usage_cache_write, usage_reasoning,
                user_text, assistant_text
         FROM turns WHERE session_id = ?1 ORDER BY turn",
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(TurnRow {
            turn_id: r.get(0)?,
            turn: r.get(1)?,
            started_at: r.get(2)?,
            ended_at: r.get(3)?,
            duration_ms: r.get(4)?,
            reason: r.get(5)?,
            usage_input: r.get(6)?,
            usage_output: r.get(7)?,
            usage_cache_read: r.get(8)?,
            usage_cache_write: r.get(9)?,
            usage_reasoning: r.get(10)?,
            user_text: r.get(11)?,
            assistant_text: r.get(12)?,
        })
    })?;
    rows.collect()
}

/// 某会话的全部工具调用。
pub fn tool_calls_by_session(conn: &Connection, session_id: &str) -> Result<Vec<ToolCallRow>> {
    let mut stmt = conn.prepare(
        "SELECT call_id, name, arguments, result_text, is_error, duration_ms, meta
         FROM tool_calls WHERE session_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(ToolCallRow {
            call_id: r.get(0)?,
            name: r.get(1)?,
            arguments: r.get(2)?,
            result_text: r.get(3)?,
            is_error: r.get::<_, i64>(4)? != 0,
            duration_ms: r.get(5)?,
            meta: r.get(6)?,
        })
    })?;
    rows.collect()
}

/// 某 runtime 的全部日志（按时间升序）。
pub fn logs_by_runtime(conn: &Connection, runtime_id: &str) -> Result<Vec<LogRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, time, level, message
         FROM runtime_logs WHERE runtime_id = ?1 ORDER BY time",
    )?;
    let rows = stmt.query_map(params![runtime_id], |r| {
        Ok(LogRow {
            id: r.get(0)?,
            time: r.get(1)?,
            level: r.get(2)?,
            message: r.get(3)?,
        })
    })?;
    rows.collect()
}

/// 某会话的 token 合计（监管面板汇总用）。
pub fn usage_summary(conn: &Connection, session_id: &str) -> Result<(i64, i64, i64, i64, i64)> {
    conn.query_row(
        "SELECT COALESCE(SUM(usage_input), 0),
                COALESCE(SUM(usage_output), 0),
                COALESCE(SUM(usage_cache_read), 0),
                COALESCE(SUM(usage_cache_write), 0),
                COALESCE(SUM(usage_reasoning), 0)
         FROM turns WHERE session_id = ?1",
        params![session_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )
}
