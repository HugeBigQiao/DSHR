//! 数据管道 s2：sqlite 落库（DESIGN §11.1 罗盘 / §11.2 表集 / §11.3 统计域 / §11.4 管道分层）。
//!
//! 只做 s2：s1 的 fold（fold.rs）已把事件流折成内存快照（snapshot.rs 的 SessionSnapshot），
//! 本模块把快照按「会话整体重放」语义持久化到 `<workspace 根>/data/dshr.db`（rusqlite
//! bundled，§11.1）。db 只装 dshr 自己的加工事实，wire-logs JSONL 仍是 lossless 源（§11.2
//! 不建 events 全量表）；跨层聚合 = read 层函数不入库（§11.3）。s3（UI 真 bridge）、
//! s4（监控页）、常驻消费循环（§11.4 fold 与落库同源同巡的事件循环）均不在本模块。
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Transaction, params};

use dsh_sdk_protocol::notifications::SessionStatus;

use crate::snapshot::{MsgKind, SessionSnapshot};

// —— 错误 ——

/// store 错误：sqlite 错误 / 目录文件 I/O / 入参不合法（三源合一，调用方一个类型处理）。
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
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

// —— §11.2 表集 DDL（幂等：全部 IF NOT EXISTS，可反复 init）——
//
// 主键思路（相对 v3 的简化，UI/查询一律按会话展开）：
//   turns / tool_calls 用 (session_id, …) 复合主键，不要全局自增 id；
//   file_ops 无自然唯一键 → 隐式 rowid + (session_id, path) 索引（s4 按 path 聚合）；
//   requests / runtime_logs 尚无写入方，建表留扩展点（s1 未折叠请求层 / 无 stderr 通道）。
//   sessions 的 created_at 由首条消息时间推出；last_seq = 快照最大消息 seq（增量书签）。
const SCHEMA: &str = r#"
-- runtime 实例事实（沿用 v3）：s2 只建表——s1 快照无 runtime 元数据，
-- s3 接 UI 时由 runtime.rs 侧写入（command/args/env 存 JSON 文本）。
CREATE TABLE IF NOT EXISTS runtimes (
    id         TEXT PRIMARY KEY,
    name       TEXT,
    state      TEXT,
    created_at INTEGER,
    command    TEXT,
    args       TEXT,   -- JSON 数组文本
    cwd        TEXT,
    env        TEXT    -- JSON 对象文本
);

-- 会话元数据（title 由 session/title 最后写入者胜；status 来自 session.status 通知）。
CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,                    -- 会话 id（通知 sessionId，wire 字符串）
    runtime_id TEXT REFERENCES runtimes(id),        -- 归属 runtime；s1 无源 → NULL
    cwd        TEXT,                                -- 会话工作目录；s1 无源 → NULL
    parent     TEXT,                                -- 子会话血缘（subagent 通知）；s3 会话树写
    created_at INTEGER NOT NULL,                    -- 首条消息 time（无消息 = 落库时刻），重放不覆盖
    status     TEXT,                                -- 'idle'|'running'（kebab，同 wire）；未收到 = NULL
    state      TEXT,                                -- 预留深层生命周期态；s1 无源 → NULL
    title      TEXT,                                -- 会话标题（session/title）
    updated_at INTEGER NOT NULL,                    -- 最后一条消息 time（无消息 = 落库时刻）
    last_seq   INTEGER NOT NULL DEFAULT 0           -- 快照最大消息 seq（增量书签/目录排序）
);

-- 请求层事实（§11.3）：s1 fold 未折叠 RequestHeader 族 → 本表只建不写（写入留 s3 扩展点）。
CREATE TABLE IF NOT EXISTS requests (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    TEXT REFERENCES sessions(id),
    runtime_id    TEXT REFERENCES runtimes(id),
    turn          INTEGER,
    method        TEXT,          -- session/prompt / shutdown / …
    time          INTEGER,       -- epoch ms
    duration_ms   INTEGER,
    success       INTEGER,       -- 0/1
    error_message TEXT
);

-- 轮事实：主键 (session_id, turn)——比 v3 全局 turn_id 简单，UI/查询都按会话展开。
-- token 六桶列名即六桶语义（DESIGN §11.2）：total = adapter 权威 totalTokens，缺报 = 0。
CREATE TABLE IF NOT EXISTS turns (
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn         INTEGER NOT NULL,                  -- 轮号（turn/start data.turn）
    started      INTEGER,                           -- turn/start time（epoch ms）
    ended        INTEGER,                           -- turn/end time；未结算轮 = NULL
    duration_ms  INTEGER,                           -- ended − started；任一端缺失 = NULL
    reason       TEXT,                              -- 结束原因一行（fold 的 reason_text，'error/…' 前缀记错误）；未结算 = NULL
    input        INTEGER NOT NULL DEFAULT 0,
    output       INTEGER NOT NULL DEFAULT 0,
    cache_read   INTEGER NOT NULL DEFAULT 0,
    cache_write  INTEGER NOT NULL DEFAULT 0,
    reasoning    INTEGER NOT NULL DEFAULT 0,
    total        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, turn)
);

-- 工具调用事实：主键 (session_id, call_id)。arguments/result 存 fold 已截断的摘要
-- （≤300 字符，全文在 wire log）；meta_json 暂不写——s1 ToolItem 只留 diffs 摘要
-- （FileDiff），原样 meta JSON 在 wire log（§11.2 tool_calls.meta 预留原样 JSON）。
CREATE TABLE IF NOT EXISTS tool_calls (
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    call_id      TEXT NOT NULL,                     -- tool/call data.callId
    name         TEXT NOT NULL,                     -- 工具名
    arguments    TEXT NOT NULL,                     -- 参数截断摘要
    result       TEXT,                              -- 结果截断摘要；挂起调用（result 未到）= NULL
    is_error     INTEGER NOT NULL DEFAULT 0,        -- 0/1：tool/result error 或 isError=true
    duration_ms  INTEGER NOT NULL DEFAULT 0,        -- result.time − call.time（saturating，回放不可靠时 0）
    meta_json    TEXT,                              -- 预留：meta.diffs 原样 JSON；s2 暂不写
    PRIMARY KEY (session_id, call_id)
);

-- 文件变更事实（§11.2 新增，自 ToolItem.diffs 展开，一行 = 一个文件变更摘要）。
-- turn 列 s2 恒 NULL：s1 快照的 Tool 行未标注轮号（MsgItem 无 turn 字段），
-- fold 补标注后填；seq = 工具行事件 seq（表内排序/时间线），time = tool/call 时刻。
CREATE TABLE IF NOT EXISTS file_ops (
    session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn          INTEGER,
    seq           INTEGER,                          -- 所属工具行的消息 seq
    time          INTEGER,                          -- tool/call time（epoch ms）
    path          TEXT NOT NULL,                    -- FileDiff.path
    op            TEXT NOT NULL,                    -- edit|write|delete|str_replace|diff（见 infer_op）
    lines_added   INTEGER NOT NULL,                 -- newText 行数
    lines_removed INTEGER NOT NULL                  -- oldText 行数
);
CREATE INDEX IF NOT EXISTS idx_file_ops_session_path ON file_ops(session_id, path);

-- runtime stderr 审计（§11.3 系统层）：尚无 stderr 通道（client 未暴露）→ 只建表，
-- s3 接 client 的 stderr 监控后写。
CREATE TABLE IF NOT EXISTS runtime_logs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    runtime_id TEXT REFERENCES runtimes(id),
    time       INTEGER,
    level      TEXT,
    line       TEXT
);
"#;

// —— Store ——

/// sqlite 加工库（DESIGN §11.1 的 data/dshr.db）。写 = 会话整体重放（幂等），读 = 聚合。
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开（或创建）库：父目录自动创建（运行时 data/ 不存在则一并建出），随后 init_schema。
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?; // data/ 已被 .gitignore 忽略（§11.1 罗盘）。
        }
        let conn = Connection::open(path)?;
        // 级联删除依赖 FK：sessions 删除 → turns/tool_calls/file_ops 随删（s3 目录层清理用）。
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Store { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// 内存库（测试用）：schema 已就绪，免建目录。
    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Store { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// 建表（幂等：IF NOT EXISTS；open 已调用，重复调用无害）。
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// 落库一个会话快照，语义 = 该会话「整体重放/替换」（幂等）：
    /// sessions UPSERT 元数据；turns/tool_calls/file_ops 先 DELETE 该会话再整插——
    /// 快照 = 该会话全量视角（fold 每次从事件流重建全量），同一快照重复 persist 行数不变。
    /// 一个事务包住全部四步（失败整体回滚，不留半截状态）。
    pub fn persist_snapshot(&mut self, snap: &SessionSnapshot) -> Result<()> {
        if snap.session_id.is_empty() {
            return Err(StoreError::InvalidSnapshot(
                "session_id 为空（快照未接任何会话通知）".into(),
            ));
        }
        let now = now_ms();
        // created/updated 由消息 time 推出：离线回放与在线同输入产出同一时刻（确定性强），
        // 无消息的会话（纯 status/title）退回落库时刻。
        let created_at = snap.messages.iter().map(|m| m.time).min().unwrap_or(now);
        let updated_at = snap.messages.iter().map(|m| m.time).max().unwrap_or(now);
        // last_seq = 快照内最大消息 seq：TurnStat/MsgItem 中只有消息流带 seq（TurnStat 无 seq），
        // 故以最大消息 seq 为准；无消息 = 0。s3 增量同步的续传书签。
        let last_seq = snap.messages.iter().map(|m| m.seq).max().unwrap_or(0);
        let status = snap.status.as_ref().map(status_of);
        let tx = self.conn.transaction()?;
        upsert_session(&tx, snap, created_at, updated_at, last_seq, status)?;
        replace_turns(&tx, &snap.session_id, &snap.turns)?;
        replace_tool_calls(&tx, &snap.session_id, &snap.messages)?;
        replace_file_ops(&tx, &snap.session_id, &snap.messages)?;
        tx.commit()?;
        Ok(())
    }

    /// 会话层聚合（§11.3 会话层；s4 监控页雏形/目录的数据源）。左联聚合不入库。
    pub fn session_summaries(&self) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(SQL_SUMMARIES)?;
        let rows = stmt.query_map([], |r| {
            Ok(SessionSummary {
                id: r.get("id")?,
                title: r.get("title")?,
                status: r.get("status")?,
                created_at: sql_u64(r.get("created_at")?),
                updated_at: sql_u64(r.get("updated_at")?),
                last_seq: sql_u64(r.get("last_seq")?),
                turns: sql_u64(r.get("turns")?),
                tokens: sql_u64(r.get("tokens")?),
                tool_calls: sql_u64(r.get("tool_calls")?),
                turn_errors: sql_u64(r.get("turn_errors")?),
                tool_errors: sql_u64(r.get("tool_errors")?),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// rusqlite 整数列是 i64（SQLite INTEGER 无符号型）：u64 边界转换。
/// 值域（时间戳 ms/行数/token 数）远小于 i64::MAX，`as` 截断无实际风险。
#[allow(clippy::cast_possible_wrap)]
fn sql_i64(v: u64) -> i64 {
    v as i64
}

#[allow(clippy::cast_possible_wrap)]
fn sql_i64o(v: Option<u64>) -> Option<i64> {
    v.map(|x| x as i64)
}

fn sql_u64(v: i64) -> u64 {
    v as u64
}

/// §11.3 会话层聚合查询：sessions 左联 turns/tool_calls 子查询（无轮/无工具的会话也出列）。
/// tokens 口径：六桶取五项求和（input/output/cache_read/cache_write/reasoning）——
/// total 是 adapter 权威整值（已含前几项），相加会重复计，故不入合计（明细仍可单独查）。
/// turn_errors 按 reason 'error/…' 前缀计（fold 的 reason_text 对 TurnEndReason::Error 产此形状）。
const SQL_SUMMARIES: &str = r#"
SELECT s.id           AS id,
       s.title        AS title,
       s.status       AS status,
       s.created_at   AS created_at,
       s.updated_at   AS updated_at,
       s.last_seq     AS last_seq,
       COALESCE(t.n_turns,    0) AS turns,
       COALESCE(t.n_tokens,   0) AS tokens,
       COALESCE(t.n_turn_err, 0) AS turn_errors,
       COALESCE(c.n_calls,    0) AS tool_calls,
       COALESCE(c.n_tool_err, 0) AS tool_errors
FROM sessions s
LEFT JOIN (
    SELECT session_id,
           COUNT(*)                                            AS n_turns,
           SUM(input + output + cache_read + cache_write + reasoning) AS n_tokens,
           SUM(CASE WHEN reason LIKE 'error/%' THEN 1 ELSE 0 END)     AS n_turn_err
    FROM turns GROUP BY session_id
) t ON t.session_id = s.id
LEFT JOIN (
    SELECT session_id,
           COUNT(*)        AS n_calls,
           SUM(is_error)   AS n_tool_err
    FROM tool_calls GROUP BY session_id
) c ON c.session_id = s.id
ORDER BY s.updated_at DESC
"#;

/// 一个会话的聚合摘要（§11.3 会话层：起止/轮数/token/工具/错误/标题）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    /// 'idle'|'running'；未收到 session.status 通知 = None。
    pub status: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// 快照最大消息 seq（无消息 0）。
    pub last_seq: u64,
    /// 轮数（turns 行数；含未结算轮——fold 把进行中轮也放进快照）。
    pub turns: u64,
    /// token 合计（六桶中五项之和，total 权威桶单列不入合计，见 SQL_SUMMARIES 注释）。
    pub tokens: u64,
    pub tool_calls: u64,
    /// 轮级错误（reason 以 error/ 开头；fold 的 errors 口径之一）。
    pub turn_errors: u64,
    /// 工具级错误（is_error=1；fold 的 errors 口径之二）。
    pub tool_errors: u64,
}

impl SessionSummary {
    /// 总错误数 = 轮级 + 工具级（与 fold 的 errors 计数同口径：不重复计）。
    pub fn errors(&self) -> u64 {
        self.turn_errors + self.tool_errors
    }
}

/// 默认库路径：`<workspace 根>/data/dshr.db`（§11.1 罗盘）。
/// workspace 根 = 本 crate 的父目录（env! CARGO_MANIFEST_DIR，与 config.rs/main.rs/
/// dshr-ui setting.rs 同款取法）；父目录由 `Store::open` 自动创建。
pub fn default_db_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace 根")
        .join("data")
        .join("dshr.db")
}

// —— persist 内部实现 ——

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// status → 存储文本（kebab-case，与 wire 的 serde rename 一致；监控页直接展示）。
fn status_of(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Running => "running",
    }
}

/// sessions UPSERT：只插不入更新 created_at（重放不覆盖首见时刻）；runtime_id/cwd/
/// parent/state 暂缺源 → 插 NULL，更新不碰（未来由 s3 目录层/请求层写）。
fn upsert_session(
    tx: &Transaction,
    snap: &SessionSnapshot,
    created_at: u64,
    updated_at: u64,
    last_seq: u64,
    status: Option<&'static str>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO sessions (id, runtime_id, cwd, parent, created_at, status, state,
                               title, updated_at, last_seq)
         VALUES (?1, NULL, NULL, NULL, ?2, ?3, NULL, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
             status     = excluded.status,
             title      = excluded.title,
             updated_at = excluded.updated_at,
             last_seq   = excluded.last_seq",
        params![
            snap.session_id,
            sql_i64(created_at),
            status,
            snap.title,
            sql_i64(updated_at),
            sql_i64(last_seq)
        ],
    )?;
    Ok(())
}

/// turns 替换：DELETE 该会话全部轮 → 按快照顺序整插（TurnStat.usage 六桶落六列）。
fn replace_turns(tx: &Transaction, sid: &str, turns: &[crate::snapshot::TurnStat]) -> Result<()> {
    tx.execute("DELETE FROM turns WHERE session_id = ?1", params![sid])?;
    for t in turns {
        // duration = ended − started；未结算轮（end None，截断日志）或 start 缺失 → NULL。
        let duration = match (t.start_time, t.end_time) {
            (Some(s), Some(e)) => Some(e.saturating_sub(s)),
            _ => None,
        };
        tx.execute(
            "INSERT INTO turns (session_id, turn, started, ended, duration_ms, reason,
                                input, output, cache_read, cache_write, reasoning, total)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                sid,
                sql_i64(t.turn),
                sql_i64o(t.start_time),
                sql_i64o(t.end_time),
                duration.map(|d| d as i64),
                t.reason,
                sql_i64(t.usage.input),
                sql_i64(t.usage.output),
                sql_i64(t.usage.cache_read),
                sql_i64(t.usage.cache_write),
                sql_i64(t.usage.reasoning),
                sql_i64(t.usage.total)
            ],
        )?;
    }
    Ok(())
}

/// tool_calls 替换：DELETE 该会话全部 → 从消息流的 Tool 行整插。
/// arguments/result 已在 fold 截断（≤300 字符）；meta_json 暂不写（见 DDL 注释）。
fn replace_tool_calls(
    tx: &Transaction,
    sid: &str,
    msgs: &[crate::snapshot::MsgItem],
) -> Result<()> {
    tx.execute("DELETE FROM tool_calls WHERE session_id = ?1", params![sid])?;
    for m in msgs {
        if m.kind != MsgKind::Tool {
            continue;
        }
        let Some(tool) = m.tool.as_ref() else {
            continue; // Tool 行必带卡片；防御性跳过。
        };
        tx.execute(
            "INSERT INTO tool_calls (session_id, call_id, name, arguments, result,
                                     is_error, duration_ms, meta_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![
                sid,
                tool.call_id,
                tool.name,
                tool.arguments,
                tool.result,
                if tool.is_error { 1 } else { 0 },
                sql_i64(tool.duration_ms)
            ],
        )?;
    }
    Ok(())
}

/// file_ops 替换：DELETE 该会话全部 → 自 ToolItem.diffs（FileDiff）逐条展开成行
/// （一行 = 一个文件变更摘要，§11.2 file_ops）。op 由工具名推断（简单规则，见 infer_op）。
fn replace_file_ops(tx: &Transaction, sid: &str, msgs: &[crate::snapshot::MsgItem]) -> Result<()> {
    tx.execute("DELETE FROM file_ops WHERE session_id = ?1", params![sid])?;
    for m in msgs {
        if m.kind != MsgKind::Tool {
            continue;
        }
        let Some(tool) = m.tool.as_ref() else {
            continue;
        };
        let op = infer_op(&tool.name);
        for d in &tool.diffs {
            tx.execute(
                "INSERT INTO file_ops (session_id, turn, seq, time, path, op,
                                       lines_added, lines_removed)
                 VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    sid,
                    sql_i64(m.seq),
                    sql_i64(m.time),
                    d.path,
                    op,
                    sql_i64(d.added),
                    sql_i64(d.removed)
                ],
            )?;
        }
    }
    Ok(())
}

/// 由工具名推断 file_ops.op（简单规则，DESIGN §11.2 op ∈ edit|write|delete|str_replace）：
/// 名字含 edit → edit；含 write → write；含 delete/rm_ → delete；含 str_replace/replace →
/// str_replace；其余（read/查询类 diff 摘要、未来新工具）统一 "diff"。误判无害——path 与
/// 行数才是事实列，op 只是展示归类；精确 op 语义待 s3 对照官方工具名再校准。
fn infer_op(name: &str) -> &'static str {
    if name.contains("edit") {
        "edit"
    } else if name.contains("write") {
        "write"
    } else if name.contains("delete") || name.contains("rm_") {
        "delete"
    } else if name.contains("str_replace") || name.contains("replace") {
        "str_replace"
    } else {
        "diff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{FileDiff, MsgItem, SessionStats, ToolItem, TurnStat, UsageAgg};

    /// 工具行助手（卡片内容可直接构造，不必过 fold）。
    fn tool_msg(
        seq: u64,
        time: u64,
        call_id: &str,
        name: &str,
        is_error: bool,
        duration_ms: u64,
        diffs: Vec<FileDiff>,
    ) -> MsgItem {
        MsgItem {
            kind: MsgKind::Tool,
            text: String::new(),
            reasoning: None,
            usage: None,
            tool: Some(ToolItem {
                call_id: call_id.into(),
                name: name.into(),
                arguments: "{\"path\":\"/x\"}".into(),
                duration_ms,
                is_error,
                result: Some("ok".into()),
                diffs,
            }),
            time,
            seq,
        }
    }

    /// 样本快照：2 轮（1 完成 + 1 错误）+ 1 工具（edit_file，2 个 diff），
    /// 消息 seq 1..3（last_seq=3）。turn 计数与 fold 同款（UsageAgg 数字取自 fold.rs 测试）。
    fn sample_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            session_id: "s-rt1".into(),
            title: Some("落库测试会话".into()),
            status: Some(SessionStatus::Running),
            messages: vec![
                MsgItem {
                    kind: MsgKind::User,
                    text: "第一问".into(),
                    reasoning: None,
                    usage: None,
                    tool: None,
                    time: 1000,
                    seq: 1,
                },
                MsgItem {
                    kind: MsgKind::Assistant,
                    text: "答".into(),
                    reasoning: None,
                    usage: None,
                    tool: None,
                    time: 2000,
                    seq: 2,
                },
                tool_msg(
                    3,
                    3000,
                    "c1",
                    "edit_file",
                    false,
                    500,
                    vec![
                        FileDiff {
                            path: "a.rs".into(),
                            added: 3,
                            removed: 2,
                        },
                        FileDiff {
                            path: "b.txt".into(),
                            added: 2,
                            removed: 0,
                        },
                    ],
                ),
            ],
            turns: vec![
                TurnStat {
                    turn: 1,
                    start_time: Some(1000),
                    end_time: Some(6000),
                    reason: Some("completed".into()),
                    usage: UsageAgg {
                        input: 100,
                        output: 20,
                        cache_read: 5,
                        cache_write: 3,
                        reasoning: 2,
                        total: 0,
                    },
                },
                // reason 形状 = fold 的 reason_text（error/ 前缀 → summary 记轮级错误 1）。
                TurnStat {
                    turn: 2,
                    start_time: Some(7000),
                    end_time: Some(9000),
                    reason: Some("error/LLM_ERR: 模型崩了".into()),
                    usage: UsageAgg {
                        input: 50,
                        output: 8,
                        cache_read: 0,
                        cache_write: 0,
                        reasoning: 0,
                        total: 58,
                    },
                },
            ],
            stats: SessionStats {
                turns: 2,
                errors: 1,
                ..Default::default()
            },
        }
    }

    /// 各事实表行数（幂等断言用）。
    fn counts(conn: &Connection) -> (i64, i64, i64, i64) {
        let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        (
            one("SELECT COUNT(*) FROM sessions"),
            one("SELECT COUNT(*) FROM turns"),
            one("SELECT COUNT(*) FROM tool_calls"),
            one("SELECT COUNT(*) FROM file_ops"),
        )
    }

    /// init 幂等：重复 init_schema 不报错；磁盘库 open 自动建父目录且可重开（IF NOT EXISTS）。
    #[test]
    fn schema_init_is_idempotent_and_open_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("dshr-s2-{}", std::process::id()));
        let db = dir.join("nested").join("dshr.db");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let store = Store::open(&db).unwrap(); // 父目录尚不存在 → open 内 create_dir_all。
            store.init_schema().unwrap();
            store.init_schema().unwrap(); // 幂等
        }
        {
            let store = Store::open(&db).unwrap(); // 重开（schema 已存在，不报错）
            assert!(store.session_summaries().unwrap().is_empty());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 往返：写 sample（2 轮/1 工具/2 diff/usage）→ 聚合断言轮数/token/工具/错误/标题。
    #[test]
    fn persist_roundtrip_drives_session_summaries() {
        let mut store = Store::open_in_memory().unwrap();
        store.persist_snapshot(&sample_snapshot()).unwrap();
        let sums = store.session_summaries().unwrap();
        assert_eq!(sums.len(), 1);
        let s = &sums[0];
        assert_eq!(s.id, "s-rt1");
        assert_eq!(s.title.as_deref(), Some("落库测试会话"));
        assert_eq!(s.status.as_deref(), Some("running"));
        assert_eq!(s.created_at, 1000); // 首条消息 time
        assert_eq!(s.updated_at, 3000); // 末条消息 time
        assert_eq!(s.last_seq, 3);
        assert_eq!(s.turns, 2);
        // 六桶五项合计：(100+20+5+3+2) + (50+8+0+0+0) = 188；total 桶（0+58）不入合计。
        assert_eq!(s.tokens, 188);
        assert_eq!(s.tool_calls, 1);
        assert_eq!(s.turn_errors, 1); // turn2 reason 'error/…'
        assert_eq!(s.tool_errors, 0);
        assert_eq!(s.errors(), 1);
        // tool_calls 行落库如实（name/arguments/result/is_error/duration_ms）。
        let tool: (String, String, String, i64, i64) = store
            .conn
            .query_row(
                "SELECT name, arguments, result, is_error, duration_ms
                 FROM tool_calls WHERE session_id = 's-rt1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(tool.0, "edit_file");
        assert!(tool.1.contains("\"path\""));
        assert_eq!(tool.2, "ok");
        assert_eq!((tool.3, tool.4), (0, 500));
    }

    /// 替换幂等：同一快照再次 persist → 四表行数不变、聚合不变。
    #[test]
    fn repersist_same_snapshot_is_row_idempotent() {
        let mut store = Store::open_in_memory().unwrap();
        store.persist_snapshot(&sample_snapshot()).unwrap();
        let first = counts(&store.conn);
        let sum1 = store.session_summaries().unwrap();
        store.persist_snapshot(&sample_snapshot()).unwrap();
        assert_eq!(counts(&store.conn), first);
        assert_eq!(store.session_summaries().unwrap(), sum1);
        assert_eq!(first, (1, 2, 1, 2)); // sessions 1 / turns 2 / tool_calls 1 / file_ops 2
    }

    /// file_ops 展开：一行 = 一个 diff；op 推断规则（edit/write/delete/diff）+ 行数正确。
    #[test]
    fn file_ops_expansion_rows_and_op_inference() {
        let mut snap = sample_snapshot();
        snap.messages.push(tool_msg(
            4,
            4000,
            "c2",
            "write_file",
            false,
            10,
            vec![FileDiff {
                path: "new/x.md".into(),
                added: 2,
                removed: 0,
            }],
        ));
        snap.messages.push(tool_msg(
            5,
            5000,
            "c3",
            "delete_file",
            true, // 工具错误如实落库
            7,
            vec![FileDiff {
                path: "old.rs".into(),
                added: 0,
                removed: 3,
            }],
        ));
        let mut store = Store::open_in_memory().unwrap();
        store.persist_snapshot(&snap).unwrap();
        let rows: Vec<(String, String, i64, i64, i64)> = store
            .conn
            .prepare(
                "SELECT path, op, seq, lines_added, lines_removed
                 FROM file_ops WHERE session_id = 's-rt1' ORDER BY seq",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(rows.len(), 4); // sample 2 + write 1 + delete 1
        assert_eq!(rows[0], ("a.rs".into(), "edit".into(), 3, 3, 2));
        assert_eq!(rows[1], ("b.txt".into(), "edit".into(), 3, 2, 0));
        assert_eq!(rows[2], ("new/x.md".into(), "write".into(), 4, 2, 0));
        assert_eq!(rows[3], ("old.rs".into(), "delete".into(), 5, 0, 3));
        // c3 工具错误 is_error=1 落库。
        let err: i64 = store
            .conn
            .query_row(
                "SELECT is_error FROM tool_calls
                 WHERE session_id = 's-rt1' AND call_id = 'c3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(err, 1);
    }

    /// sessions UPSERT：title/status/last_seq 落库；重放更新 title 但 created_at 不覆盖。
    #[test]
    fn sessions_upsert_carries_title_status_and_last_seq() {
        let mut store = Store::open_in_memory().unwrap();
        let mut snap = SessionSnapshot {
            session_id: "s-meta".into(),
            title: Some("旧标题".into()),
            status: Some(SessionStatus::Idle),
            ..Default::default()
        };
        snap.messages.push(MsgItem {
            kind: MsgKind::User,
            text: "hi".into(),
            reasoning: None,
            usage: None,
            tool: None,
            time: 100,
            seq: 5,
        });
        snap.messages.push(MsgItem {
            kind: MsgKind::User,
            text: "hi2".into(),
            reasoning: None,
            usage: None,
            tool: None,
            time: 400,
            seq: 9,
        });
        store.persist_snapshot(&snap).unwrap();
        // last_seq = 最大消息 seq = 9；created/updated 由消息 time 推出。
        let row: (String, Option<String>, Option<String>, i64, i64, i64) = store
            .conn
            .query_row(
                "SELECT id, title, status, created_at, updated_at, last_seq
                 FROM sessions WHERE id = 's-meta'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "s-meta");
        assert_eq!(row.1.as_deref(), Some("旧标题"));
        assert_eq!(row.2.as_deref(), Some("idle"));
        assert_eq!((row.3, row.4, row.5), (100, 400, 9));
        // 标题更新重放：title 变、created_at 仍是首见时刻、行数不增。
        snap.title = Some("新标题".into());
        store.persist_snapshot(&snap).unwrap();
        let row2: (Option<String>, i64) = store
            .conn
            .query_row(
                "SELECT title, created_at FROM sessions WHERE id = 's-meta'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row2.0.as_deref(), Some("新标题"));
        assert_eq!(row2.1, 100);
        assert_eq!(counts(&store.conn).0, 1);
        // 无轮无工具会话也能出聚合行（0 值）。
        assert_eq!(store.session_summaries().unwrap()[0].turns, 0);
        assert_eq!(store.session_summaries().unwrap()[0].tokens, 0);
    }
}
