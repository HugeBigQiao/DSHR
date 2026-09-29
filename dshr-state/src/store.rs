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

use crate::snapshot::SessionSnapshot;

pub mod convert;
pub mod error;
pub mod schema;
pub mod write;

pub use error::{Result, StoreError};

use convert::{now_ms, sql_i64, sql_u64, status_of};
use schema::{SCHEMA, SQL_SUMMARIES};
use write::{replace_file_ops, replace_tool_calls, replace_turns, upsert_session};

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

    /// 建表（幂等：IF NOT EXISTS；open 已调用，重复调用无害）。
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// 落库一个会话快照，语义 = 该会话「整体重放/替换」（幂等）：
    /// sessions UPSERT 元数据；turns/tool_calls/file_ops 先 DELETE 该会话再整插——
    /// 快照 = 该会话全量视角（fold 每次从事件流重建全量），同一快照重复 persist 行数不变。
    /// 一个事务包住全部四步（失败整体回滚，不留半截状态）。
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
        let tx = self.conn.unchecked_transaction()?;
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
