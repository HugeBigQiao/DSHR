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
//   requests / runtime_logs 尚无写入方，建表留扩展点（请求层折叠未做 / stderr 通道未接）。
//   sessions 的 created_at 由首条消息时间推出；last_seq = 快照最大消息 seq（增量书签）。
pub(crate) const SCHEMA: &str = r#"
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
