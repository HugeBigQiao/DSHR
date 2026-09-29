//! fold 的渲染与解析纯函数：事件 `data` / 折叠态 → 展示用文本与统计。
//!
//! 主要用途：`fold/event.rs` 在折叠每条事件时调用这里的函数，把协议字段转成
//! UI 直接可显示的文本（消息正文、思考文本、结束原因文案）与统计（流摘要、文件 diff 行数）。
//! 为什么需要：这些是**无状态**的转换（输入相同必然输出相同），与「状态如何推进」无关；
//! 分成两层后，`event.rs` 只关心状态迁移，本文件可以单独用纯输入输出验证。
//! 上接：`fold/event.rs`（唯一调用方）。
//! 下接：`fold.rs`（共享小工具 `truncate_chars` / `text_of`）、`snapshot`（产出
//!       `StreamSummary` / `FileDiff`）、`dsh_sdk_protocol::llm` 与 `session_event`。
//! 官方对应：无单点对应（官方把等价的展示转换放在各 client UI 包内；dshr 集中在此）。
use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::llm::{AssistantStreamRecord, StreamChunk};
use dsh_sdk_protocol::session_event::turn::{TurnEndCancelCause, TurnEndReason};

use super::truncate_chars;

/// `event.rs` 经本模块取用（保持 `event -> render -> mod` 的单向依赖）。
///
/// 为什么需要：`text_of` 定义在父模块 `fold.rs`，而 `event.rs` 与父模块直接互相
/// 引用会形成双向依赖；经本模块转发一次，依赖方向保持单向。
pub(super) use super::text_of;

// 本段自带的 use（FileDiff / StreamSummary / ContentBlock / StreamChunk）在下方。
use crate::snapshot::{FileDiff, StreamSummary};

/// 把 v3 `AssistantStreamRecord` 展开成统计摘要（不保留逐 chunk，控制快照体积）。
pub(super) fn stream_summary(records: &[AssistantStreamRecord]) -> StreamSummary {
    let mut summary = StreamSummary::default();
    for record in records {
        for timed in record.expand() {
            summary.chunks += 1;
            summary.first_time = Some(
                summary
                    .first_time
                    .map_or(timed.time, |value| value.min(timed.time)),
            );
            summary.last_time = Some(
                summary
                    .last_time
                    .map_or(timed.time, |value| value.max(timed.time)),
            );
            match timed.chunk {
                StreamChunk::TextDelta { text, .. } => {
                    summary.text_chars += text.chars().count() as u64;
                    mark_first_token(&mut summary, timed.time);
                }
                StreamChunk::ReasoningDelta { text, .. } => {
                    summary.reasoning_chars += text.chars().count() as u64;
                    mark_first_token(&mut summary, timed.time);
                }
                StreamChunk::ToolCallDelta {
                    arguments_delta, ..
                } => {
                    summary.tool_args_chars += arguments_delta.chars().count() as u64;
                    mark_first_token(&mut summary, timed.time);
                }
                _ => {}
            }
        }
    }
    summary
}

pub(super) fn mark_first_token(summary: &mut StreamSummary, time: u64) {
    if summary.first_token_time.is_none() {
        summary.first_token_time = Some(time);
    }
}

/// 思考文本：各 type=reasoning 块合并；无 → None。
pub(super) fn reasoning_of(content: &[ContentBlock]) -> Option<String> {
    let joined = content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Reasoning(r) => Some(r.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// 轮结束原因 → 一行文本（快照展示用）。
pub(super) fn reason_text(r: &TurnEndReason) -> String {
    match r {
        TurnEndReason::Completed => "completed".to_string(),
        TurnEndReason::Aborted { reason } => match reason {
            TurnEndCancelCause::User => "aborted/user".to_string(),
            TurnEndCancelCause::Parent => "aborted/parent".to_string(),
            TurnEndCancelCause::Hook { .. } => "aborted/hook".to_string(),
            TurnEndCancelCause::Disposed => "aborted/disposed".to_string(),
            TurnEndCancelCause::Legacy => "aborted/legacy".to_string(),
        },
        TurnEndReason::Blocked => "blocked".to_string(),
        TurnEndReason::Error { error } => {
            format!(
                "error/{}: {}",
                error.code,
                truncate_chars(&error.message, 160)
            )
        }
        TurnEndReason::MaxTokens => "max-tokens".to_string(),
        TurnEndReason::Interrupted => "interrupted".to_string(),
        // fork 种子构造关闭的未结束 turn（官方 0.1.7-rc.2 新增；主循环不发）。
        TurnEndReason::Forked => "forked".to_string(),
        // merge-extensible 兜底：未来新增 kind 不整体丢失，只退化展示。
        TurnEndReason::Other => "other".to_string(),
    }
}

/// oldText/newText → 行数：空/null = 0；否则按 \n 计数、尾随换行只作行终止符
/// （"a\nb" = 2、"a\nb\n" = 2、"a" = 1）。近似值，精确 diff 以 wire meta.diffs 原样为准。
pub(super) fn line_count(s: &str) -> u64 {
    if s.is_empty() {
        return 0;
    }
    let body = s.strip_suffix('\n').unwrap_or(s);
    if body.is_empty() {
        return 0;
    }
    body.matches('\n').count() as u64 + 1
}

/// 自 tool/result 的 `meta.diffs`（[{path, oldText, newText}]）折叠逐文件行数：
/// removed = oldText 行数、added = newText 行数；oldText/newText 为 null/缺失 → 0；
/// 无 path 的条目跳过（无法归属文件）。
pub(super) fn fold_diffs(meta: Option<&serde_json::Value>) -> Vec<FileDiff> {
    let Some(arr) = meta.and_then(|m| m.get("diffs")).and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let removed = entry
            .get("oldText")
            .and_then(serde_json::Value::as_str)
            .map_or(0, line_count);
        let added = entry
            .get("newText")
            .and_then(serde_json::Value::as_str)
            .map_or(0, line_count);
        out.push(FileDiff {
            path: path.to_string(),
            added,
            removed,
        });
    }
    out
}
