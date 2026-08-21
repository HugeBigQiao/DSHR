//! 写入层：只写不删（append-only）。
//!
//! 例外：`runtimes` 表的 name/state 允许 update（用户改名、标记废弃），
//! 其余表均为历史记录，不做 delete/update。
use rusqlite::{Connection, params};

/// 插入一个 runtime 进程。
pub fn insert_runtime(
    conn: &Connection,
    id: &str,
    name: &str,
    state: &str,
    created_at: i64,
    command: &str,
    args: Option<&str>,
    current_dir: Option<&str>,
    env: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO runtimes (id, name, state, created_at, command, args, current_dir, env)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, name, state, created_at, command, args, current_dir, env],
    )?;
    Ok(())
}

/// 改 runtime 名字（runtimes 表唯一的 update 入口之一）。
pub fn update_runtime_name(conn: &Connection, id: &str, new_name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE runtimes SET name = ?2 WHERE id = ?1",
        params![id, new_name],
    )?;
    Ok(())
}

/// 标记废弃（删除 = 改 state 为 archived，数据保留，不物理删除）。
pub fn archive_runtime(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE runtimes SET state = 'archived' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// 插入一个会话。
pub fn insert_session(
    conn: &Connection,
    id: &str,
    runtime_id: &str,
    cwd: &str,
    parent_session_id: Option<&str>,
    created_at: i64,
    status: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, runtime_id, cwd, parent_session_id, created_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, runtime_id, cwd, parent_session_id, created_at, status],
    )?;
    Ok(())
}

/// 更新会话状态（idle/running）。
pub fn update_session_status(conn: &Connection, id: &str, status: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET status = ?2 WHERE id = ?1",
        params![id, status],
    )?;
    Ok(())
}

/// 更新会话最新事件 seq（增量同步书签）。
pub fn update_session_last_seq(conn: &Connection, id: &str, last_seq: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET last_seq = ?2 WHERE id = ?1",
        params![id, last_seq],
    )?;
    Ok(())
}

/// 插入一个请求（initialize / session_prompt / shutdown）。
/// 进程级请求（initialize/shutdown）的 session_id/turn_id 传 None。
pub fn insert_request(
    conn: &Connection,
    runtime_id: &str,
    session_id: Option<&str>,
    turn_id: Option<&str>,
    time: i64,
    method: &str,
    duration_ms: Option<i64>,
    success: bool,
    error_message: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO requests (runtime_id, session_id, turn_id, time, method, duration_ms, success, error_message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            runtime_id,
            session_id,
            turn_id,
            time,
            method,
            duration_ms,
            success as i64,
            error_message
        ],
    )?;
    Ok(())
}

/// 回填请求的 turn_id（session_prompt 发出时轮号未知，等 turn/start 事件来了再回填）。
/// 接收：runtime/session 归属 + 目标 turn_id。
/// 处理：把该会话最近一条未关联的 session_prompt 请求补上 turn_id。
/// 生成：requests 表完成轮级关联（DESIGN §9.5 请求计量）。
pub fn update_request_turn_id(
    conn: &Connection,
    runtime_id: &str,
    session_id: &str,
    turn_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE requests SET turn_id = ?3
         WHERE runtime_id = ?1 AND session_id = ?2
           AND turn_id IS NULL AND method = 'session_prompt'",
        params![runtime_id, session_id, turn_id],
    )?;
    Ok(())
}

/// 插入一轮（turn/start 时开行，turn/end 时回填）。
pub fn insert_turn(
    conn: &Connection,
    turn_id: &str,
    runtime_id: &str,
    session_id: &str,
    turn: i64,
    started_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO turns (turn_id, runtime_id, session_id, turn, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![turn_id, runtime_id, session_id, turn, started_at],
    )?;
    Ok(())
}

/// 轮结束回填：结束时间、耗时、原因、token 合计、用户/助手文本。
/// 接收：turn_id + 可选回填值（None 表示缺失）。
#[allow(clippy::too_many_arguments)]
pub fn finish_turn(
    conn: &Connection,
    turn_id: &str,
    ended_at: Option<i64>,
    duration_ms: Option<i64>,
    reason: Option<&str>,
    usage_input: Option<i64>,
    usage_output: Option<i64>,
    usage_cache_read: Option<i64>,
    usage_cache_write: Option<i64>,
    usage_reasoning: Option<i64>,
    user_text: Option<&str>,
    assistant_text: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE turns SET ended_at = ?2, duration_ms = ?3, reason = ?4,
                usage_input = ?5, usage_output = ?6, usage_cache_read = ?7,
                usage_cache_write = ?8, usage_reasoning = ?9,
                user_text = ?10, assistant_text = ?11
         WHERE turn_id = ?1",
        params![
            turn_id,
            ended_at,
            duration_ms,
            reason,
            usage_input,
            usage_output,
            usage_cache_read,
            usage_cache_write,
            usage_reasoning,
            user_text,
            assistant_text
        ],
    )?;
    Ok(())
}

/// 插入一个事件（lossless：payload 原始 JSON 原样存）。
pub fn insert_event(
    conn: &Connection,
    session_id: &str,
    seq: i64,
    event_type: &str,
    time: i64,
    turn: Option<i64>,
    step: Option<i64>,
    payload: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO events (session_id, seq, type, time, turn, step, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![session_id, seq, event_type, time, turn, step, payload],
    )?;
    Ok(())
}

/// 插入一个工具调用。
/// 接收：runtime/session 归属 + call 信息；meta 是工具私有展示载荷（如 fs 工具的 diff），原样存 JSON。
pub fn insert_tool_call(
    conn: &Connection,
    runtime_id: &str,
    session_id: &str,
    turn_id: Option<&str>,
    call_id: &str,
    name: &str,
    arguments: Option<&str>,
    result_text: Option<&str>,
    is_error: bool,
    duration_ms: Option<i64>,
    meta: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tool_calls (runtime_id, session_id, turn_id, call_id, name, arguments, result_text, is_error, duration_ms, meta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            runtime_id,
            session_id,
            turn_id,
            call_id,
            name,
            arguments,
            result_text,
            is_error as i64,
            duration_ms,
            meta
        ],
    )?;
    Ok(())
}

/// 插入一条 runtime 日志（stderr 等进程级输出）。
/// 接收：runtime 归属 + 本地接收时间（epoch ms）+ 级别 + 原始行。
/// 处理：追加到 runtime_logs（append-only）。
/// 生成：一行审计日志（GUI 无终端时的崩溃排查依据）。
pub fn insert_log(
    conn: &Connection,
    runtime_id: &str,
    time: i64,
    level: &str,
    message: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO runtime_logs (runtime_id, time, level, message)
         VALUES (?1, ?2, ?3, ?4)",
        params![runtime_id, time, level, message],
    )?;
    Ok(())
}
