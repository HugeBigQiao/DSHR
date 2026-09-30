//! store 的写入实现：`persist_snapshot` 的落库步骤。
//!
//! 主要用途：被 `store.rs` 的 `Store::persist_snapshot` 在**一个事务内**依次调用
//! （upsert 会话元数据 → 替换 messages → 替换 turns → 替换 tool_calls → 替换 file_ops）。
//! 为什么需要：这些函数的共同点是「**先删该会话的旧行，再按快照整插**」——
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
    meta_json: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO sessions (id, runtime_id, cwd, parent, created_at, status, state,
                               title, updated_at, last_seq, meta_json)
         VALUES (?1, NULL, NULL, NULL, ?2, ?3, NULL, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             status     = excluded.status,
             title      = excluded.title,
             updated_at = excluded.updated_at,
             last_seq   = excluded.last_seq,
             meta_json  = excluded.meta_json",
        params![
            snap.session_id,
            sql_i64(created_at),
            status,
            snap.title,
            sql_i64(updated_at),
            sql_i64(last_seq),
            meta_json
        ],
    )?;
    Ok(())
}

/// messages 替换：DELETE 该会话全部 → 按快照消息流整插（**逐条对话事实**）。
///
/// 为什么需要（2026-09-29 用户要求）：库要能回答「这个会话每轮聊了什么、工具有没有失败、
/// 失败原因是什么」，所以除逐 chunk 之外的全部内容都落库；这是「关掉再打开还能复原」的数据基础。
/// 参数 `tx`：调用方事务。参数 `sid`：会话 id。参数 `msgs`：快照消息流（顺序即插入顺序）。
/// 返回：sqlite 错误包装为 [`Result`]。
pub(crate) fn replace_messages(
    tx: &Transaction,
    sid: &str,
    msgs: &[crate::snapshot::MsgItem],
) -> Result<()> {
    tx.execute("DELETE FROM messages WHERE session_id = ?1", params![sid])?;
    // 先删后逐行插 = **整表替换**语义（快照本来就是全量视角，fold 每次从事件流重建），
    // 而不是逐行 UPSERT：后者要主键比对，还会留下「重放后已不存在」的旧行（比如轮被截断）。
    for m in msgs {
        // 三个可选组先各取一次引用：下面 30 个参数绝大多数来自它们，
        // 逐处 `m.tool.as_ref()` 会让「这列属于哪一组」变模糊。
        let tool = m.tool.as_ref();
        let usage = m.usage.as_ref();
        let stream = m.stream.as_ref();
        tx.execute(
            "INSERT INTO messages (session_id, seq, time, turn, step, kind, source, text, reasoning,
                                   error, input, output, cache_read, cache_write, reasoning_tokens,
                                   total, stream_chunks, stream_first_time, stream_last_time,
                                   stream_first_token, stream_text_chars, stream_reasoning_chars,
                                   stream_tool_args_chars, tool_call_id, tool_name, tool_arguments,
                                   tool_result, tool_is_error, tool_duration_ms, tool_meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                     ?10, ?11, ?12, ?13, ?14, ?15,
                     ?16, ?17, ?18, ?19,
                     ?20, ?21, ?22,
                     ?23, ?24, ?25, ?26,
                     ?27, ?28, ?29, ?30)",
            params![
                sid,
                sql_i64(m.seq),
                sql_i64(m.time),
                sql_i64o(m.turn),
                sql_i64o(m.step),
                kind_text(m.kind),
                m.source,
                m.text,
                m.reasoning,
                m.error,
                // 六桶逐列：**未知保持 NULL**（不写 0）——复原要能区分「报 0」与「没报」。
                usage.map(|u| u.input_tokens as i64),
                usage.map(|u| u.output_tokens as i64),
                usage.and_then(|u| u.cache_read_tokens).map(|v| v as i64),
                usage.and_then(|u| u.cache_write_tokens).map(|v| v as i64),
                usage.and_then(|u| u.reasoning_tokens).map(|v| v as i64),
                usage.and_then(|u| u.total_tokens).map(|v| v as i64),
                sql_i64o(stream.map(|s| s.chunks)),
                sql_i64o(stream.and_then(|s| s.first_time)),
                sql_i64o(stream.and_then(|s| s.last_time)),
                sql_i64o(stream.and_then(|s| s.first_token_time)),
                sql_i64o(stream.map(|s| s.text_chars)),
                sql_i64o(stream.map(|s| s.reasoning_chars)),
                sql_i64o(stream.map(|s| s.tool_args_chars)),
                // 工具有关的列：非工具行整段为 NULL。例外是 `tool_is_error` 与 `tool_duration_ms`，
                // 它们用 0 而不是 NULL——对「不是工具行」而言，这两个值没有「未知」的含义可表达。
                tool.map(|t| t.call_id.clone()),
                tool.map(|t| t.name.clone()),
                tool.map(|t| t.arguments.clone()),
                tool.and_then(|t| t.result.clone()),
                tool.map_or(0, |t| if t.is_error { 1 } else { 0 }),
                sql_i64(tool.map_or(0, |t| t.duration_ms)),
                // `meta` 以原样 JSON 字符串存（含 diffs 与官方后来加的字段）：`diffs` 是它的投影，
                // 复原时从 meta 重折一次即可（见 `Store::load_snapshot`），不另存一份以免两者漂开。
                tool.and_then(|t| t.meta.as_ref())
                    .map(|m| m.to_string()),
            ],
        )?;
    }
    Ok(())
}

/// 消息行种类 → 库里的稳定文本（与 `crate::export::kind_name` 同口径；**不要用 Debug 格式**）。
pub(crate) fn kind_text(kind: MsgKind) -> &'static str {
    match kind {
        MsgKind::User => "user",
        MsgKind::Assistant => "assistant",
        MsgKind::Reasoning => "reasoning",
        MsgKind::Tool => "tool",
        MsgKind::Notice => "notice",
        MsgKind::Injected => "injected",
        MsgKind::Attempt => "attempt",
    }
}

/// 上面那个的逆（复原用）。未知文本 → `Notice`（宁可显示成一行小字，也不丢这一步的记录）。
///
/// 与 `kind_text` 成对放在一起：它是**库里的判别值**，改一处必须改另一处，靠得近才不容易漏。
pub(crate) fn kind_from_text(text: &str) -> MsgKind {
    match text {
        "user" => MsgKind::User,
        "assistant" => MsgKind::Assistant,
        "reasoning" => MsgKind::Reasoning,
        "tool" => MsgKind::Tool,
        "injected" => MsgKind::Injected,
        "attempt" => MsgKind::Attempt,
        _ => MsgKind::Notice,
    }
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
/// `arguments`/`result` **全文落库**（用户 2026-09-29 要求「能记多少记多少」；fold 曾截断到
/// 300 字符，该截断已取消），`meta_json` 存工具的原样 meta（含 `diffs`）。
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
    // 两重过滤：`kind == Tool` 挑出工具行；`tool` 为 `None` 的行跳过（`Tool` 行**应当**带卡片，
    // 这里是防御——宁可少一行工具事实，也不要为一条畸形数据让整个落库事务回滚）。
    for m in msgs {
        if m.kind != MsgKind::Tool {
            continue;
        }
        let Some(tool) = m.tool.as_ref() else {
            continue; // Tool 行必带卡片；防御性跳过。
        };
        tx.execute(
            "INSERT INTO tool_calls (session_id, call_id, name, arguments, result,
                                     is_error, duration_ms, error, meta_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                sid,
                tool.call_id,
                tool.name,
                tool.arguments,
                tool.result,
                if tool.is_error { 1 } else { 0 },
                sql_i64(tool.duration_ms),
                // 失败原因（用户要求）：单独一列，便于「哪些工具失败了、为什么」直接 SQL 查。
                tool.error,
                tool.meta.as_ref().map(serde_json::Value::to_string),
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
    // 一行 = 一个文件变更：把工具卡片里的 `diffs` 摊平。所以「同一个工具改了 3 个文件」在这里是
    // 3 行（工具名与轮号重复出现），这是有意的——按文件聚合（哪些文件被改得最多）才是查询目标。
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
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    sid,
                    // 轮号：fold 给消息行补了 turn 标注（2026-09-29），这里如实落库
                    //（此前恒 NULL，是「快照没有轮号」的历史遗留）。
                    sql_i64o(m.turn),
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
    // 分支顺序**有意如此**（先说清楚，免得后来者当成 bug）：`str_replace_editor` 会先命中
    // `contains("edit")`——因为名字里的 `editor` 含 `edit`——于是它归类成 `edit` 而永远走不到
    // `str_replace` 那条分支。`op` 只是展示归类，粗判无害；要精确得有一张「工具名 → 操作」的
    // 映射表，那要等对照官方工具名清单之后再做（现在没有权威清单，猜表更糟）。
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
