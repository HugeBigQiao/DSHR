//! store 的 SQL 常量：表集 DDL 与聚合查询。
//!
//! 主要用途：`store.rs` 的 `init_schema()` 用 [`SCHEMA`] 建表；
//! `session_summaries()` 用 [`SQL_SUMMARIES`] 做会话层聚合。
//! 为什么需要：SQL 与 Rust 逻辑分开——DDL 是**幂等的可重跑定义**（全部 `IF NOT EXISTS`），
//! 改动它等于改库结构，应当与读写逻辑的改动分开审视与 review。
//! 上接：`store.rs`（`init_schema` / `session_summaries`）。
//! 下接：无（纯字符串常量）。
//! 官方对应：无（dshr 自己的加工库；官方把会话持久化在 `dsh-home/`，与本库职责不同）。
//!
//! 详细设计见 `DESIGN.md` §8.1（数据罗盘）、§8.2（表集）、§8.3（统计域）。

// —— 表集 DDL（幂等：全部 IF NOT EXISTS，可反复 init）——
//
// 主键思路（UI/查询一律按会话展开）：
//   turns / tool_calls 用 (session_id, …) 复合主键，不要全局自增 id；
//   file_ops 无自然唯一键 → 隐式 rowid + (session_id, path) 索引（按 path 聚合）；
//   requests / runtime_logs 用自增 id（追加式日志：同一次请求可能重试、同一条 stderr 可重复）。
//   sessions 的 created_at 由首条消息时间推出；last_seq = 快照最大消息 seq（增量书签）。
//   八张表现在**都有写入方**（2026-09-29）：requests ← engine 记每次 prompt 的成败/耗时；
//   runtime_logs ← engine 记 stderr 与进程退出；messages ← 逐条对话（除逐 chunk）。
pub(crate) const SCHEMA: &str = r#"
-- runtime 实例事实：写入方 = engine（`upsert_runtime`，写 id/name/state 与生命周期）。
-- `command` / `args` / `env` 仍为空（JSON 文本待写）：它们要在真正 spawn 的地方取，
-- 而那里目前只把命令行拼好就用了——等要做「复现一次 runtime 启动」时再补。
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
    last_seq   INTEGER NOT NULL DEFAULT 0,          -- 快照最大消息 seq（增量书签/目录排序）
    -- 复原用：会话级聚合与最近请求原样 JSON（`{stats, last_request, plan_mode, sandbox_mode}`）。
    -- 为什么用 JSON 而不是十几列：这些字段只被「整份读回」使用（查询走 §8.3 的聚合 SQL），
    -- 而 JSON + serde 默认值让**将来加字段不会读不回老库**。
    meta_json  TEXT NOT NULL DEFAULT ''
);

-- 请求层事实（§11.3）：宿主发出的**协议请求**（method/session/prompt…）及结果。
-- 写入方 = engine（它才知道每次请求的耗时与成败）；失败原因存 error_message
-- （用户 2026-09-29 明确要求：哪怕发送失败，失败原因也要记）。
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

-- 消息事实（2026-09-29 用户要求「细致到每个会话内的每轮对话，除了逐 chunk 其余都记」）：
-- 一行 = 消息流里的一行（**含程序化注入与未提交的尝试**；只存流摘要，不存逐 chunk）。
-- 主键 (session_id, seq)：seq 是事件序号；本地合成行（Fake 回显 / 发送失败记录）用
-- `max_seq + 1`，见 `fold::Folder::push_local_notice`——本地行必须不与 wire seq 撞主键。
-- 与 wire-log 的关系：本表是**加工后的关系视图**（可 SQL 聚合、可 CSV 导出），
-- wire-log 仍是 lossless 原始源；两者都由同一套 fold 语义产出，不会互相矛盾。
CREATE TABLE IF NOT EXISTS messages (
    session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq               INTEGER NOT NULL,        -- 事件 seq（本地合成行 = max_seq+1）
    time              INTEGER,                 -- 事件 time（epoch ms）
    turn              INTEGER,                 -- 所属轮（data.turn；无 = NULL）
    step              INTEGER,                 -- 所属步
    kind              TEXT NOT NULL,           -- user|assistant|reasoning|tool|notice|injected|attempt
    source            TEXT,                    -- 来源 kind（user/model/tool/system-prompt/runtime-context/…）
    text              TEXT NOT NULL DEFAULT '',
    reasoning         TEXT,                    -- 思考文本（Assistant/Reasoning 行）
    error             TEXT,                    -- 失败原因（工具失败 / 重试 / 本地发送失败）
    -- token 六桶：**可空**——官方 adapter 只报部分桶（缺报 = unknown，不是 0），
    -- 复原时要能区分「报了 0」与「没报」（否则 `load_snapshot` 读回的快照与原始快照不等）。
    input             INTEGER,
    output            INTEGER,
    cache_read        INTEGER,
    cache_write       INTEGER,
    reasoning_tokens  INTEGER,
    total             INTEGER,
    stream_chunks     INTEGER,                 -- 流摘要（不存逐 chunk，见 §8.3）
    stream_first_time INTEGER,
    stream_last_time  INTEGER,
    stream_first_token INTEGER,
    stream_text_chars INTEGER,
    stream_reasoning_chars INTEGER,
    stream_tool_args_chars INTEGER,
    tool_call_id      TEXT,
    tool_name         TEXT,
    tool_arguments    TEXT,
    tool_result       TEXT,
    tool_is_error     INTEGER NOT NULL DEFAULT 0,
    tool_duration_ms  INTEGER NOT NULL DEFAULT 0,
    tool_meta         TEXT,                    -- 工具私有载荷原样 JSON（如完整 diff）
    PRIMARY KEY (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_messages_session_turn ON messages(session_id, turn);

-- 轮事实：主键 (session_id, turn)——比 v3 全局 turn_id 简单，UI/查询都按会话展开。
-- token 六桶列名即六桶语义（DESIGN.md §8.2）：total = adapter 权威 totalTokens，缺报 = 0。
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

-- 工具调用事实：主键 (session_id, call_id)。**全文落库**（用户 2026-09-29 要求「能记多少记多少」，
-- fold 曾经把 arguments/result 截断到 300 字符，该截断已取消）；meta_json 存原样 meta（如完整 diff）。
CREATE TABLE IF NOT EXISTS tool_calls (
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    call_id      TEXT NOT NULL,                     -- tool/call data.callId
    name         TEXT NOT NULL,                     -- 工具名
    arguments    TEXT NOT NULL,                     -- 参数全文
    result       TEXT,                              -- 结果全文；挂起调用（result 未到）= NULL
    is_error     INTEGER NOT NULL DEFAULT 0,        -- 0/1：tool/result error 或 isError=true
    duration_ms  INTEGER NOT NULL DEFAULT 0,        -- result.time − call.time（saturating，回放不可靠时 0）
    error        TEXT,                              -- 失败原因（官方 error.reason，退而 `code: name`）
    meta_json    TEXT,                              -- meta 原样 JSON（如 fs 工具的完整 diff）
    PRIMARY KEY (session_id, call_id)
);

-- 文件变更事实（§11.2 新增，自 ToolItem.diffs 展开，一行 = 一个文件变更摘要）。
-- turn 由 fold 给消息行补的轮号填（2026-09-29 起有值，此前恒 NULL）；
-- seq = 工具行事件 seq（表内排序/时间线），time = tool/call 时刻。
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

-- runtime stderr 审计（§11.3 系统层）：写入方 = engine（M3 接通）——它把子进程 stderr 的每一行
-- 与进程退出原因都记在这里；「界面上一片安静」时有据可查（这正是当初建表留扩展点的理由）。
CREATE TABLE IF NOT EXISTS runtime_logs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    runtime_id TEXT REFERENCES runtimes(id),
    time       INTEGER,
    level      TEXT,
    line       TEXT
);
"#;

// —— schema 版本与迁移 ——

/// schema 版本（`PRAGMA user_version`）。**改动表结构时必须 +1，并在下面的 `MIGRATIONS` 补一条**。
///
/// 为什么需要（而不是让用户删库重建）：库是「可删除重建」的加工结果，但**用户的会话历史在库里**——
/// 版本升级要能平滑加列，不能逼人清数据（DESIGN §8.1：data/ 可整体删除，但那是兜底不是流程）。
/// v1 → v2（2026-09-29）：新增 `messages` 表（CREATE IF NOT EXISTS 已覆盖）、
/// `sessions.meta_json`、`tool_calls.error`（两列走 ALTER，见 `MIGRATIONS[0]`）。
pub(crate) const SCHEMA_VERSION: i64 = 2;

/// v2 的列级迁移：`(表, 列, 列定义)`。老库缺列时补上；已存在则跳过（幂等）。
///
/// 只放**加列**这类无副作用变更；改类型/删列属于破坏性变更——那时宁可让用户删库重建
/// （数据可从 wire-log 回放补齐，见 `export::replay_sessions`）。
pub(crate) const MIGRATIONS_V2: [(&str, &str, &str); 2] = [
    ("sessions", "meta_json", "TEXT NOT NULL DEFAULT ''"),
    ("tool_calls", "error", "TEXT"),
];

// —— 导出（CSV / 监控页历史导出）——

/// 可导出的库表（§8.2 的事实表；`messages` 是逐条对话，其余是聚合与审计）。
///
/// 为什么用枚举而不是「传表名字符串」：表名最终要拼进 SQL，枚举把**可接受的表**钉在类型上
///（调用方无法传任意字符串去拼 SQL）；列序也固定，导出的文件在不同版本/不同机器间可比、可 diff。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportTable {
    Runtimes,
    Sessions,
    Messages,
    Turns,
    ToolCalls,
    FileOps,
    Requests,
    RuntimeLogs,
}

impl ExportTable {
    /// 全部表，导出顺序固定（先元数据、后事实、最后审计）。
    pub const ALL: [ExportTable; 8] = [
        ExportTable::Runtimes,
        ExportTable::Sessions,
        ExportTable::Messages,
        ExportTable::Turns,
        ExportTable::ToolCalls,
        ExportTable::FileOps,
        ExportTable::Requests,
        ExportTable::RuntimeLogs,
    ];

    /// 文件名（不含扩展名；导出模块拼 `.csv`）。
    pub fn name(self) -> &'static str {
        match self {
            ExportTable::Runtimes => "runtimes",
            ExportTable::Sessions => "sessions",
            ExportTable::Messages => "messages",
            ExportTable::Turns => "turns",
            ExportTable::ToolCalls => "tool_calls",
            ExportTable::FileOps => "file_ops",
            ExportTable::Requests => "requests",
            ExportTable::RuntimeLogs => "runtime_logs",
        }
    }

    /// 导出 SQL：显式列序 + **稳定排序**。
    ///
    /// 为什么不用 `SELECT *`：列序会随 DDL 演进漂移，而导出的 CSV 是给人看/给脚本比对的，
    /// 列序稳定才算得上「可比」。排序同理——同一份数据的两次导出应当逐字节相同。
    pub(crate) fn sql(self) -> &'static str {
        match self {
            ExportTable::Runtimes => {
                "SELECT id, name, state, created_at, command, args, cwd, env FROM runtimes ORDER BY id"
            }
            ExportTable::Sessions => {
                "SELECT id, runtime_id, cwd, parent, created_at, status, state, title, updated_at, last_seq
                 FROM sessions ORDER BY id"
            }
            ExportTable::Messages => {
                "SELECT session_id, seq, time, turn, step, kind, source, text, reasoning, error,
                        input, output, cache_read, cache_write, reasoning_tokens, total,
                        stream_chunks, stream_first_time, stream_last_time, stream_first_token,
                        stream_text_chars, stream_reasoning_chars, stream_tool_args_chars,
                        tool_call_id, tool_name, tool_arguments, tool_result,
                        tool_is_error, tool_duration_ms, tool_meta
                 FROM messages ORDER BY session_id, seq"
            }
            ExportTable::Turns => {
                "SELECT session_id, turn, started, ended, duration_ms, reason,
                        input, output, cache_read, cache_write, reasoning, total
                 FROM turns ORDER BY session_id, turn"
            }
            ExportTable::ToolCalls => {
                "SELECT session_id, call_id, name, arguments, result, is_error, duration_ms, meta_json
                 FROM tool_calls ORDER BY session_id, rowid"
            }
            ExportTable::FileOps => {
                "SELECT session_id, turn, seq, time, path, op, lines_added, lines_removed
                 FROM file_ops ORDER BY session_id, rowid"
            }
            ExportTable::Requests => {
                "SELECT id, session_id, runtime_id, turn, method, time, duration_ms, success,
                        error_message
                 FROM requests ORDER BY id"
            }
            ExportTable::RuntimeLogs => {
                "SELECT id, runtime_id, time, level, line FROM runtime_logs ORDER BY id"
            }
        }
    }
}

/// §11.3 会话层聚合查询：sessions 左联 turns/tool_calls 子查询（无轮/无工具的会话也出列）。
/// tokens 口径：六桶取五项求和（input/output/cache_read/cache_write/reasoning）——
/// total 是 adapter 权威整值（已含前几项），相加会重复计，故不入合计（明细仍可单独查）。
/// turn_errors 按 reason 'error/…' 前缀计（fold 的 reason_text 对 TurnEndReason::Error 产此形状）。
pub(crate) const SQL_SUMMARIES: &str = r#"
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
