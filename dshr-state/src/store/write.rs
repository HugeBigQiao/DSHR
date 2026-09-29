//! store 的写入实现：`persist_snapshot` 的四步落库。
//!
//! 主要用途：被 `store.rs` 的 `Store::persist_snapshot` 在**一个事务内**依次调用
//! （upsert 会话元数据 → 替换 turns → 替换 tool_calls → 替换 file_ops）。
//! 为什么需要：这四个函数的共同点是「**先删该会话的旧行，再按快照整插**」——
//! 因为 fold 的快照是**全量视角**（每次从事件流重建），所以落库语义是「整体重放/替换」
//! 而不是增量 upsert。把这条语义集中在这里，`store.rs` 就只需负责事务边界与字段推导。
//! 上接：`store.rs`（`Store::persist_snapshot`）。
//! 下接：`store/schema.rs`（表结构）、`store/convert.rs`（值转换）、`store/error.rs`（错误）、
//!       `snapshot`（输入是 `SessionSnapshot` / `TurnStat` / `MsgItem`）。
//! 官方对应：无（dshr 自己的持久化细节）。
// —— persist 内部实现 ——
use rusqlite::{Transaction, params};

use crate::snapshot::{MsgKind, SessionSnapshot};

use super::convert::{sql_i64, sql_i64o};
use super::error::Result;

/// sessions UPSERT：只插不入更新 created_at（重放不覆盖首见时刻）；runtime_id/cwd/
/// parent/state 暂缺源 → 插 NULL，更新不碰（未来由多 runtime 管理与请求层写）。
///
/// 为什么需要：`sessions` 是其余表的父行（FK 级联删除依赖它），且它的 `title`/`status`/`last_seq`
/// 会随后续快照变化——所以必须 UPSERT 而非纯 INSERT。
/// 参数 `tx`：调用方开启的事务（本函数不 commit）。
/// 参数 `snap`：会话快照（用 `session_id` 与 `title`）。
/// 参数 `created_at`/`updated_at`：由快照消息时间推出的起止（无消息时由调用方回落库时刻）。
/// 参数 `last_seq`：快照内最大消息 seq，作为增量续传书签。
/// 参数 `status`：已转成存储文本的状态（见 [`super::convert::status_of`]）。
/// 返回：sqlite 错误包装为 [`Result`]；成功为 `()`。
pub(crate) fn upsert_session(
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

/// turns 替换：DELETE 该会话全部轮 → 按快照顺序整插（`TurnStat.usage` 六桶落六列）。
///
/// 为什么需要：轮的统计会随快照重放而变化（例如截断日志被后续事件补齐），
/// 逐行 upsert 需要主键比对且容易留下已消失的行，整表替换更简单且天然幂等。
/// 参数 `tx`：调用方事务。参数 `sid`：会话 id。参数 `turns`：快照里的轮统计（顺序即插入顺序）。
/// 返回：sqlite 错误包装为 [`Result`]。
pub(crate) fn replace_turns(
    tx: &Transaction,
    sid: &str,
    turns: &[crate::snapshot::TurnStat],
) -> Result<()> {
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
/// `arguments`/`result` 已在 fold 截断（≤300 字符）；`meta_json` 暂不写（见 DDL 注释）。
///
/// 为什么需要：工具事实是「每工具名多少次、成功失败、耗时」统计的唯一来源；
/// 行从 `MsgItem` 的 Tool 行来而不是另建集合——fold 已经把 call↔result 配对完成。
/// 参数 `tx`：调用方事务。参数 `sid`：会话 id。参数 `msgs`：快照消息流（只取 Tool 行）。
/// 返回：sqlite 错误包装为 [`Result`]；非 Tool 行与缺卡片的行防御性跳过（不报错）。
pub(crate) fn replace_tool_calls(
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

/// file_ops 替换：DELETE 该会话全部 → 自 `ToolItem.diffs`（`FileDiff`）逐条展开成行
/// （一行 = 一个文件变更摘要）。`op` 由工具名推断（简单规则，见 [`infer_op`]）。
///
/// 为什么需要：把「工具调用里的 diff 载荷」摊平成**按文件**的事实行，
/// 才能回答「这个仓库哪些文件被改得最多、累计 +n/−m」——这是原始事件流无法直接查询的。
/// 参数 `tx`：调用方事务。参数 `sid`：会话 id。参数 `msgs`：快照消息流（只取带 diffs 的 Tool 行）。
/// 返回：sqlite 错误包装为 [`Result`]。`turn` 列暂写 NULL（快照的 Tool 行未携带轮号）。
pub(crate) fn replace_file_ops(
    tx: &Transaction,
    sid: &str,
    msgs: &[crate::snapshot::MsgItem],
) -> Result<()> {
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

/// 由工具名推断 `file_ops.op`（取值域 `edit` | `write` | `delete` | `str_replace` | `diff`）。
///
/// 为什么需要：事件流里没有「这是什么操作」的显式字段，只有工具名与 diff 载荷。
/// 按工具名归类足够回答「改了哪些文件、增删多少行」；**误判无害**——`path` 与行数才是事实列，
/// `op` 只是展示归类。精确语义待对照官方工具名再校准。
/// 入参 `name`：工具名（如 `edit_file` / `write_file` / `str_replace_editor`）。
/// 返回：归类串；无法归类时返回 `"diff"`（含查询类工具的 diff 摘要与未来的新工具）。
pub(crate) fn infer_op(name: &str) -> &'static str {
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
