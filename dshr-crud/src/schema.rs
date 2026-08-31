//! 建表：执行 TABLES.csv 定义的全部 CREATE TABLE + 索引。
//!
//! 表结构以 `dshr/TABLES.csv` 为准（单信息源），本文件是它的落地实现。
use rusqlite::Connection;

/// 建表 + 索引（幂等：IF NOT EXISTS，重复调用安全）。
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        -- ① 进程宿主（用户可改名、可标记废弃）
        CREATE TABLE IF NOT EXISTS runtimes (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL,
            state        TEXT NOT NULL,           -- active / closed / archived
            created_at   INTEGER NOT NULL,
            command      TEXT NOT NULL,
            args         TEXT,
            current_dir  TEXT,
            env          TEXT
        );

        -- ② 会话（整段对话，挂到 runtime 下）
        CREATE TABLE IF NOT EXISTS sessions (
            id                TEXT PRIMARY KEY,
            runtime_id        TEXT NOT NULL REFERENCES runtimes(id),
            cwd               TEXT NOT NULL,
            parent_session_id TEXT,
            created_at        INTEGER NOT NULL,
            status            TEXT,
            state             TEXT NOT NULL DEFAULT 'active',  -- active / archived（归档到历史）
            title             TEXT,                            -- 会话标题（session/title 事件或用户改名）
            last_seq          INTEGER NOT NULL DEFAULT 0
        );

        -- ③ 请求（进程级 initialize/shutdown + 轮请求 session_prompt）
        CREATE TABLE IF NOT EXISTS requests (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            runtime_id    TEXT NOT NULL,
            session_id    TEXT,
            turn_id       TEXT,
            time          INTEGER NOT NULL,
            method        TEXT NOT NULL,
            duration_ms   INTEGER,
            success       INTEGER NOT NULL DEFAULT 0,
            error_message TEXT
        );

        -- ④ 轮（turn/start → turn/end，token 展开成列）
        CREATE TABLE IF NOT EXISTS turns (
            turn_id             TEXT PRIMARY KEY,
            runtime_id          TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            turn                INTEGER NOT NULL,
            started_at          INTEGER NOT NULL,
            ended_at            INTEGER,
            duration_ms         INTEGER,
            reason              TEXT,
            usage_input         INTEGER,
            usage_output        INTEGER,
            usage_cache_read    INTEGER,
            usage_cache_write   INTEGER,
            usage_reasoning     INTEGER,
            user_text           TEXT,
            assistant_text      TEXT
        );

        -- ⑤ 事件（lossless 底线：payload 原始 JSON）
        CREATE TABLE IF NOT EXISTS events (
            session_id TEXT NOT NULL,
            seq        INTEGER NOT NULL,
            type       TEXT NOT NULL,
            time       INTEGER NOT NULL,
            turn       INTEGER,
            step       INTEGER,
            payload    TEXT NOT NULL,
            PRIMARY KEY (session_id, seq)
        );

        -- ⑥ 工具调用（监管命令视图的直查表）
        CREATE TABLE IF NOT EXISTS tool_calls (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            runtime_id   TEXT NOT NULL,
            session_id   TEXT NOT NULL,
            turn_id      TEXT,
            call_id      TEXT NOT NULL,
            name         TEXT NOT NULL,
            arguments    TEXT,
            result_text  TEXT,
            is_error     INTEGER NOT NULL DEFAULT 0,
            duration_ms  INTEGER,
            meta         TEXT
        );

        -- ⑦ runtime 日志（stderr 等进程级输出，审计用）
        CREATE TABLE IF NOT EXISTS runtime_logs (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            runtime_id   TEXT NOT NULL,
            time         INTEGER NOT NULL,
            level        TEXT NOT NULL DEFAULT 'stderr',
            message      TEXT NOT NULL
        );

        -- 索引（查询常用路径）
        CREATE INDEX IF NOT EXISTS idx_sessions_runtime ON sessions(runtime_id);
        CREATE INDEX IF NOT EXISTS idx_turns_session    ON turns(session_id);
        CREATE INDEX IF NOT EXISTS idx_events_type      ON events(session_id, type);
        CREATE INDEX IF NOT EXISTS idx_tool_calls_sess  ON tool_calls(session_id, turn_id);
        CREATE INDEX IF NOT EXISTS idx_requests_rt_meth ON requests(runtime_id, method);
        CREATE INDEX IF NOT EXISTS idx_logs_runtime    ON runtime_logs(runtime_id, time);
        "#,
    )?;

    // 老库迁移（决策 20）：sessions.state/title 列是后加的，已存在则报错忽略（幂等）。
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN state TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", []);
    Ok(())
}
