//! sqlite 加工库的门面：把 fold 的内存快照按「会话整体重放」语义落库。
//!
//! 主要用途：`persist_snapshot()` 是 engine 每次「快照有变化」时的落库入口；
//! `session_summaries()` 供会话目录 / 未来的监控页做跨会话聚合读。
//! 为什么需要：`data/wire-logs/*.jsonl` 虽然是 lossless 源，但它是**逐行追加的原始流**，
//! 无法直接回答「这个会话几轮、总共多少 token、改了哪些文件」这类问题；
//! 本库把这些事实折叠成**关系表**以便查询。库只装 dshr 自己的加工数据——
//! dsh 自己的会话与 storages 留在 `data/dsh-home/`，两者不混。
//! 上接：`raw.rs`（`Runtime::ensure_db` / `persist` / `flush_persist`）。
//! 下接：`fold`→`snapshot`（输入是 `SessionSnapshot`）、`store/schema.rs`（DDL 与查询 SQL）、
//!       `store/write.rs`（写实现）、`store/convert.rs`（值转换）、`store/error.rs`（错误）。
//! 官方对应：无（官方把会话持久化在 `dsh-home/sessions/<workspace>/<id>/session.jsonl.zstd`，
//! 与本加工库是不同用途——那份是**会话日志**，本库是**查询用的事实表**）。
//!
//! 设计见 `DESIGN.md` §8.1（数据罗盘）/ §8.2（表集）/ §8.3（统计域）/ §8.4（管道分层）。
//!
//! 模块划分（原单文件 844 行过大，已拆分；Rust 2018+ 风格：`store.rs` + `store/`）：
//! - `store.rs`（本文件）：门面——`Store` 结构、open / init_schema / persist_snapshot / session_summaries
//! - `store/error.rs`：`StoreError` + `Result`
//! - `store/schema.rs`：表集 DDL 与聚合查询 SQL
//! - `store/convert.rs`：rusqlite 整数列 ↔ u64 转换、时间戳与状态串
//! - `store/write.rs`：persist 的内部实现（upsert_session / replace_* / op 推断）
//! - 契约测试在 `tests/store_persistence.rs`（落库幂等 / 替换语义 / 会话隔离）
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use dsh_sdk_protocol::llm::TokenUsage;
use dsh_sdk_protocol::notifications::SessionStatus;

use crate::snapshot::{
    MsgItem, RequestView, SessionSnapshot, SessionStats, StreamSummary, ToolItem, TurnStat,
    UsageAgg,
};

pub mod convert;
pub mod error;
pub mod schema;
pub mod write;

pub use error::{Result, StoreError};
pub use schema::ExportTable;

use convert::{now_ms, sql_i64, sql_u64, status_of};
use schema::{SCHEMA, SQL_SUMMARIES};
use write::{
    kind_from_text, replace_file_ops, replace_messages, replace_tool_calls, replace_turns,
    upsert_session,
};

/// 状态文本 → `SessionStatus`（复原用；旧库写入的未知值 → `None`）。
fn status_from_text(text: &str) -> Option<SessionStatus> {
    match text {
        "idle" => Some(SessionStatus::Idle),
        "running" => Some(SessionStatus::Running),
        _ => None,
    }
}

/// 从一行的 token 六列（9..15）拼出 `TokenUsage`；六列全 NULL → `None`（该行没有 token 账）。
///
/// 注意**不能**把 NULL 当 0：官方 adapter 缺报的桶是 unknown，复原时必须还原成 None，
/// 否则 `load_snapshot` 读回的快照与原始快照不相等（测试 `restore_roundtrip` 会当场抓住）。
fn token_usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<TokenUsage>> {
    let raw = |i: usize| -> rusqlite::Result<Option<u64>> {
        Ok(row.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64))
    };
    let (input, output) = (raw(9)?, raw(10)?);
    let (cache_read, cache_write, reasoning, total) = (raw(11)?, raw(12)?, raw(13)?, raw(14)?);
    if input.is_none()
        && output.is_none()
        && cache_read.is_none()
        && cache_write.is_none()
        && reasoning.is_none()
        && total.is_none()
    {
        return Ok(None);
    }
    Ok(Some(TokenUsage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total,
    }))
}

/// 从一行的流摘要七列（16..22）拼出 `StreamSummary`；全 NULL → `None`（该行没有流记录）。
fn stream_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<StreamSummary>> {
    let chunks: Option<i64> = row.get(15)?;
    let Some(chunks) = chunks else {
        return Ok(None);
    };
    let opt = |i: usize| -> rusqlite::Result<Option<u64>> {
        Ok(row.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64))
    };
    Ok(Some(StreamSummary {
        chunks: chunks.max(0) as u64,
        first_time: opt(16)?,
        last_time: opt(17)?,
        first_token_time: opt(18)?,
        text_chars: opt(19)?.unwrap_or(0),
        reasoning_chars: opt(20)?.unwrap_or(0),
        tool_args_chars: opt(21)?.unwrap_or(0),
    }))
}

/// 把一列的 sqlite 值拍成文本（CSV 导出用；NULL → 空串）。
///
/// 为什么 NULL 用空串而不是 `NULL` 字面量：CSV 是给人和 Excel 看的，
/// 空单元格比字符串 "NULL" 更不容易被误读成真实取值。
fn value_to_text(value: rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match value {
        Value::Null => String::new(),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(t) => t,
        // 库里不该有 blob（schema 无 BLOB 列）；真出现就如实标注长度而不是丢数据。
        Value::Blob(b) => format!("<blob:{} bytes>", b.len()),
    }
}

/// sqlite 加工库（`data/dshr.db`）。写 = 会话整体重放（幂等），读 = 聚合。
///
/// 为什么需要：把「逐行追加的 wire 日志」转成可查询的关系事实表；
/// 同时它是 UI 之外唯一持久的加工结果（重启后会话目录不必重放全部日志）。
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

    /// 建表 + 迁移（幂等：`IF NOT EXISTS` + 列级 ALTER 先探测后加；open 已调用，重复调用无害）。
    ///
    /// 迁移用 `PRAGMA user_version` 判断是否需要跑（见 `schema::SCHEMA_VERSION`）：
    /// 老库（v1）缺 `sessions.meta_json` / `tool_calls.error` 两列，这里补上——
    /// **用户的历史会话在库里**，不能靠删库升级（DESIGN §8.1 的「可整体删除」是兜底不是流程）。
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 2 {
            for (table, column, ddl) in schema::MIGRATIONS_V2 {
                if !self.has_column(table, column)? {
                    self.conn
                        .execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))?;
                }
            }
        }
        if version < schema::SCHEMA_VERSION {
            self.conn
                .execute_batch(&format!("PRAGMA user_version = {}", schema::SCHEMA_VERSION))?;
        }
        Ok(())
    }

    /// 该表有没有这一列（迁移用：`ALTER TABLE ADD COLUMN` 在列已存在时会报错，故先探测）。
    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        // PRAGMA table_info 的列序：cid, name, type, notnull, dflt_value, pk。
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 落库一个会话快照，语义 = 该会话「整体重放/替换」（幂等）：
    /// sessions UPSERT 元数据；messages/turns/tool_calls/file_ops 先 DELETE 该会话再整插——
    /// 快照 = 该会话全量视角（fold 每次从事件流重建全量），同一快照重复 persist 行数不变。
    /// 一个事务包住全部五步（失败整体回滚，不留半截状态）。
    ///
    /// 为什么用 `&self` 而不是 `&mut self`：`rusqlite` 的事务只需 `&Connection`
    ///（`unchecked_transaction`），所以落库在类型上就是只读借用。这让 engine 能在
    /// 「已持有各会话可变借用」的同时落库——否则每个调用点都要为 `&mut Store` 做借用体操。
    pub fn persist_snapshot(&self, snap: &SessionSnapshot) -> Result<()> {
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
        // 会话级聚合与最近的模型请求原样存 JSON：复原（load_snapshot）靠它把快照补全，
        // 而**查询**走 §8.3 的聚合 SQL（session_summaries）——两者分工不重叠。
        let meta_json = serde_json::json!({
            "stats": snap.stats,
            "last_request": snap.last_request,
            "plan_mode": snap.plan_mode,
            "sandbox_mode": snap.sandbox_mode,
        })
        .to_string();
        let tx = self.conn.unchecked_transaction()?;
        upsert_session(
            &tx, snap, created_at, updated_at, last_seq, status, &meta_json,
        )?;
        replace_messages(&tx, &snap.session_id, &snap.messages)?;
        replace_turns(&tx, &snap.session_id, &snap.turns)?;
        replace_tool_calls(&tx, &snap.session_id, &snap.messages)?;
        replace_file_ops(&tx, &snap.session_id, &snap.messages)?;
        tx.commit()?;
        Ok(())
    }

    /// **复原**：从库里读回一个会话的完整快照（消息流 + 轮 + 聚合 + 模式/最近请求）。
    ///
    /// 为什么需要（用户 2026-09-29 要求）：会话跑到一定数量后关掉应用，再打开要能恢复出之前的
    /// 记录——不能指望每次都重放 wire-log（那是 lossless 源，但重建需要扫全部日志）。
    /// 库里的 `messages` + `turns` + `sessions.meta_json` 正是为此准备的。
    ///
    /// 返回：`Ok(None)` = 库里没有这个会话；`Ok(Some(snapshot))` = 复原出的快照
    ///（按 seq 排序；`last_request`/`stats`/模式从 `meta_json` 读，老库缺字段则取默认值）。
    pub fn load_snapshot(&self, session_id: &str) -> Result<Option<SessionSnapshot>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, status, meta_json FROM sessions WHERE id = ?1",
                [session_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, title, status, meta_json)) = row else {
            return Ok(None);
        };

        let mut messages: Vec<MsgItem> = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT seq, time, turn, step, kind, source, text, reasoning, error,
                    input, output, cache_read, cache_write, reasoning_tokens, total,
                    stream_chunks, stream_first_time, stream_last_time, stream_first_token,
                    stream_text_chars, stream_reasoning_chars, stream_tool_args_chars,
                    tool_call_id, tool_name, tool_arguments, tool_result,
                    tool_is_error, tool_duration_ms, tool_meta
             FROM messages WHERE session_id = ?1 ORDER BY seq",
        )?;
        let mut rows = stmt.query([session_id])?;
        while let Some(r) = rows.next()? {
            let tool_call_id: Option<String> = r.get(22)?;
            let tool = tool_call_id.map(|call_id| ToolItem {
                call_id,
                name: r
                    .get::<_, Option<String>>(23)
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
                // Tool 行的失败原因就是行级 `error` 列：fold 的 `on_tool_result` 同时写行与卡片
                //（两者恒等），所以这里从同一列读回，不需要第二份存储。
                error: r.get::<_, Option<String>>(8).ok().flatten(),
                arguments: r
                    .get::<_, Option<String>>(24)
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
                duration_ms: r.get::<_, Option<i64>>(27).ok().flatten().unwrap_or(0) as u64,
                is_error: r.get::<_, Option<i64>>(26).ok().flatten().unwrap_or(0) != 0,
                result: r.get::<_, Option<String>>(25).ok().flatten(),
                // diffs 不单独存（它是 meta 的投影）：从 meta 重新折叠一次，与在线同口径。
                diffs: crate::fold::render_diffs_from_meta(
                    r.get::<_, Option<String>>(28).ok().flatten().as_deref(),
                ),
                meta: r
                    .get::<_, Option<String>>(28)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok()),
            });
            let usage = token_usage_from_row(&r)?;
            let stream = stream_from_row(&r)?;
            messages.push(MsgItem {
                kind: kind_from_text(&r.get::<_, String>(4)?),
                text: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                reasoning: r.get::<_, Option<String>>(7)?,
                usage,
                stream,
                tool,
                time: r.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
                seq: r.get::<_, i64>(0)?.max(0) as u64,
                turn: r.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
                step: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                source: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                error: r.get::<_, Option<String>>(8)?,
            });
        }

        let mut turns: Vec<TurnStat> = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT turn, started, ended, reason, input, output, cache_read, cache_write,
                    reasoning, total
             FROM turns WHERE session_id = ?1 ORDER BY turn",
        )?;
        let mut rows = stmt.query([session_id])?;
        while let Some(r) = rows.next()? {
            turns.push(TurnStat {
                turn: r.get::<_, i64>(0)?.max(0) as u64,
                start_time: r.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
                end_time: r.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
                reason: r.get::<_, Option<String>>(3)?,
                usage: UsageAgg {
                    input: r.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
                    output: r.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64,
                    cache_read: r.get::<_, Option<i64>>(6)?.unwrap_or(0).max(0) as u64,
                    cache_write: r.get::<_, Option<i64>>(7)?.unwrap_or(0).max(0) as u64,
                    reasoning: r.get::<_, Option<i64>>(8)?.unwrap_or(0).max(0) as u64,
                    total: r.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0) as u64,
                },
            });
        }

        // meta_json：老库为空串或字段缺失 → 取默认（serde default），不让复原失败。
        let meta: serde_json::Value = meta_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);
        let stats: SessionStats = meta
            .get("stats")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let last_request: RequestView = meta
            .get("last_request")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let plan_mode = meta
            .get("plan_mode")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let sandbox_mode = meta
            .get("sandbox_mode")
            .and_then(|v| v.as_str().map(str::to_string));

        Ok(Some(SessionSnapshot {
            session_id: id,
            title,
            status: status.as_deref().and_then(status_from_text),
            messages,
            turns,
            stats,
            last_request,
            plan_mode,
            sandbox_mode,
        }))
    }

    /// 库里有哪些会话（复原时的目录：按最后更新倒序，最近用过的在前）。
    pub fn load_session_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM sessions ORDER BY updated_at DESC, id")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(r.get::<_, String>(0)?);
        }
        Ok(out)
    }

    /// 确保会话壳行存在（幂等）：只为满足 `requests`/`turns` 等子表的外键。
    ///
    /// 为什么需要：`requests` 有 `session_id REFERENCES sessions(id)`，而**第一次 prompt 时
    /// 会话还没落过任何快照**（快照要等事件回来才有内容）→ 直接插请求行会
    /// `FOREIGN KEY constraint failed`（本项目在 `runtime_logs` 上踩过同一个坑）。
    /// 这里只建一行空壳（created_at/updated_at = 落库时刻），后续 `persist_snapshot` 会把它
    /// 补成完整行（UPSERT 且不覆盖 created_at）。
    pub fn ensure_session(&self, session_id: &str) -> Result<()> {
        if session_id.is_empty() {
            return Ok(());
        }
        let now = sql_i64(now_ms());
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions (id, created_at, updated_at, last_seq)
             VALUES (?1, ?2, ?2, 0)",
            rusqlite::params![session_id, now],
        )?;
        Ok(())
    }

    /// 记一条**协议请求**事实（method / 耗时 / 成败 / 失败原因）。
    ///
    /// 为什么需要（用户 2026-09-29 要求「哪怕对话发送失败了，失败原因也要记」）：
    /// 这是 `requests` 表的第一个写入方。它回答的是监控页最直接的两个问题——
    /// 「请求成功率」与「请求有多慢」——而这两件事在 wire 事件里都不存在
    ///（发送失败时根本没有 wire 响应；成功时也没有耗时字段）。
    /// 与 `messages` 表的分工：那表记「会话里发生了什么」，本表记「宿主发了什么、结果如何」。
    #[allow(clippy::too_many_arguments)]
    pub fn append_request(
        &self,
        session_id: &str,
        runtime_id: &str,
        turn: Option<u64>,
        method: &str,
        time: u64,
        duration_ms: u64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()> {
        // 请求可能发生在「会话还没落过任何快照」之前（尤其失败的那次）→ 先补壳行满足外键。
        self.ensure_session(session_id)?;
        self.conn.execute(
            "INSERT INTO requests (session_id, runtime_id, turn, method, time, duration_ms,
                                   success, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                session_id,
                runtime_id,
                turn.map(|t| t as i64),
                method,
                sql_i64(time),
                sql_i64(duration_ms),
                if success { 1 } else { 0 },
                error_message,
            ],
        )?;
        Ok(())
    }

    /// 导出某张事实表的全部行：返回 `(列名, 行)`。
    ///
    /// 为什么放在 `Store` 上（而不是导出模块直接开连接）：连接是私有的，且导出必须与落库
    /// 走**同一个连接**（否则读到的是另一份快照/WAL 视图）。`crate::export` 只负责把行变成 CSV。
    ///
    /// 为什么不用 `Store` 的聚合查询代替：聚合回答「多少」，导出回答「具体是哪些」——
    /// 后者是监控页历史导出与人工排查都要的。
    pub fn export_rows(&self, table: ExportTable) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        let mut stmt = self.conn.prepare(table.sql())?;
        let headers: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        let width = headers.len();
        let mut out = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut cells = Vec::with_capacity(width);
            for i in 0..width {
                let value: rusqlite::types::Value = row.get(i)?;
                cells.push(value_to_text(value));
            }
            out.push(cells);
        }
        Ok((headers, out))
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

/// 默认库路径：`<workspace 根>/data/dshr.db`（§8.1 数据罗盘）。
///
/// workspace 根 = 本 crate 的父目录（`env!("CARGO_MANIFEST_DIR")`），这样库始终落在
/// dshr 仓库的 `data/` 下，与 UI 的工作区根（`raw::workspace_root()`）同源；
/// 与 `config.rs` 的 `config.json` resolve 方式一致（都以 crate 目录为锚点）。
pub fn default_db_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace 根")
        .join("data")
        .join("dshr.db")
}

// —— persist 内部实现 ——

impl Store {
    /// 记录一个 runtime 实例（`runtimes` 表；UPSERT 幂等）。
    ///
    /// # 参数
    /// - `id`：runtime 标识（engine 生成；`sessions.runtime_id` 与 `runtime_logs` 都引用它）；
    /// - `name`：展示名（模式说明）；
    /// - `state`：生命周期态（当前用 `"running"` / `"stopped"`）。
    ///
    /// 为什么需要：「哪个 runtime 产出了这些会话/日志」是审计的基本维度。
    /// 此前 `runtimes` 表建了却**没有写入方**，导致会话行的 `runtime_id` 恒为 NULL——
    /// 多 runtime 时无法把数据归位。
    ///
    /// 注：`command`/`args`/`cwd`/`env` 四列暂不写（spawn 配置在 raw 层、未向上暴露；
    /// 建表留了位，需要时把 `SpawnKit` 摘要传下来即可）。
    pub fn upsert_runtime(&self, id: &str, name: &str, state: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO runtimes (id, name, state, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = ?2, state = ?3",
            rusqlite::params![id, name, state, sql_i64(now_ms())],
        )?;
        Ok(())
    }

    /// 追加一行 runtime stderr（`runtime_logs` 表）。
    ///
    /// # 参数
    /// - `runtime_id`：来源 runtime；
    /// - `level`：当前恒 `"stderr"`（官方只有一条 stderr 流；留列以便将来分级）；
    /// - `line`：原始整行，**不截断**——这里就是「全量现场」，截断摘要属于别的表。
    ///
    /// 为什么需要：`runtime_logs` 建成后一直无写入方（stderr 通道此前未接通），
    /// 而 stderr 是「runtime 为什么出错」的唯一现场记录（协议 4 种通知都不带它）。
    /// 与 `WireLog` 的分工：WireLog 是 lossless 线级记录（问题复现用）；
    /// 本表是**可按 runtime 查询的审计索引**（SQL 可聚合、可只取某一个 runtime）。
    pub fn append_runtime_log(&self, runtime_id: &str, level: &str, line: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO runtime_logs (runtime_id, time, level, line) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![runtime_id, sql_i64(now_ms()), level, line],
        )?;
        Ok(())
    }

    /// 某个 runtime 已有的 stderr 行数（测试与将来监控页用）。
    pub fn runtime_log_count(&self, runtime_id: &str) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runtime_logs WHERE runtime_id = ?1",
            [runtime_id],
            |r| r.get(0),
        )?;
        Ok(sql_u64(n))
    }
}
