//! `Folder` 的事件处理：把单条事件的 `data` 推进折叠态。
//!
//! 主要用途：`fold.rs` 的 `push_event` 按事件类型分发后，调用这里的 `on_*` 方法完成实际折叠
//! （消息入流、工具 call↔result 配对、轮结算、统计累加）。
//! 为什么需要：`fold.rs` 原本 1193 行，其中近半是「事件 → 状态」的映射逻辑；它与
//! 「状态定义 + 编排」是两类改动理由不同的代码（前者随官方事件集变化，后者随 UI 需求变化）。
//! 上接：`fold.rs`（`Folder::push_event` 分发进来）、`tests/fold_projection.rs`（用例间接覆盖）。
//! 下接：`fold/render.rs`（文本提取与统计）、`fold.rs`（`truncate_chars`）、
//!       `snapshot`（产出 `MsgItem` / `TurnStat` / `ToolItem`）、`dsh_sdk_protocol::session_event`。
//! 官方对应：无单点对应（语义对应官方会话日志到投影的那一层，见 `fold.rs` 文件级说明）。
//!
//! 可见性说明：这些方法读写 `Folder` 的私有字段；因定义在子模块，需 `pub(super)` 才能被
//! 父模块的 `push_event` 调用（Rust 的「私有」只对自身及后代模块可见，父模块看不见）。
use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::session_event::message::{
    AssistantMessageData, Message, MessageRole, MessageSource,
};
use dsh_sdk_protocol::session_event::tool::{ToolCallData, ToolResultData};
use dsh_sdk_protocol::session_event::turn::{TurnEndData, TurnEndReason};

use super::render::{fold_diffs, reason_text, reasoning_of, stream_summary, text_of};
use super::{Folder, TurnStat, UsageAgg, truncate_chars};
use crate::snapshot::{MsgItem, MsgKind, ToolItem};

impl Folder {
    pub(super) fn on_turn_end(&mut self, time: u64, data: &TurnEndData) {
        let mut usage = UsageAgg::default();
        let mut start_time = None;
        if let Some(open) = &self.open_turn {
            if open.turn == data.turn {
                usage = open.usage.clone();
                start_time = Some(open.start_time);
            } else {
                // 轮号不匹配（截断/跨段日志）：旧轮按未结算关闭，新轮号只造 end 侧记录。
                self.closed_turns.push(TurnStat {
                    turn: open.turn,
                    start_time: Some(open.start_time),
                    end_time: None,
                    reason: None,
                    usage: open.usage.clone(),
                });
            }
        }
        self.open_turn = None;
        if matches!(data.reason, TurnEndReason::Error { .. }) {
            self.errors += 1;
        }
        self.closed_turns.push(TurnStat {
            turn: data.turn,
            start_time,
            end_time: Some(time),
            reason: Some(reason_text(&data.reason)),
            usage,
        });
    }

    /// 只认 role=user 且 source.kind=user 的人类消息为 User 行；其余来源（goal 续跑/
    /// webhook/技能/指令/上下文注入……官方把它们都写成 role=user 的程序化输入）s1 不占
    /// 消息流——折叠语义留 s3（按 source.kind 分类渲染），wire 原样保真在 JSONL。
    pub(super) fn on_user_message(&mut self, seq: u64, time: u64, msg: &Message) {
        if msg.role != MessageRole::User {
            return;
        }
        if !matches!(msg.source, MessageSource::User { .. }) {
            return;
        }
        self.push_row(MsgItem {
            kind: MsgKind::User,
            // 只拼 text 块；图片等附件消息文本为空（附件渲染 s3），行仍保留以保事件序。
            text: text_of(&msg.content),
            reasoning: None,
            usage: None,
            stream: None,
            tool: None,
            time,
            seq,
        });
    }

    pub(super) fn on_assistant_message(
        &mut self,
        seq: u64,
        time: u64,
        data: &AssistantMessageData,
    ) {
        // usage 六桶无论是否产行都入账（纯 tool-call 消息也计 token）。
        if let Some(u) = &data.usage {
            self.usage.add(u);
            if let Some(open) = &mut self.open_turn {
                open.usage.add(u);
            }
        }
        let text = text_of(&data.message.content);
        let reasoning = reasoning_of(&data.message.content);
        let usage = data.usage.clone();
        let stream = data.stream.as_deref().map(stream_summary);
        if text.is_empty() && reasoning.is_none() {
            // content 只有 tool-call/image/未知块：不产行——工具行由 tool/call+result
            // 配对产生，附件渲染 s3；usage 已在上方入账。
            return;
        }
        // 正文与思考合并进同一行（思考折叠行由 UI 在 Assistant 行内展开）；
        // 只有思考没有正文（中断前缀/纯思考步）→ 灰字 Reasoning 行。
        let item = MsgItem {
            kind: if text.is_empty() {
                MsgKind::Reasoning
            } else {
                MsgKind::Assistant
            },
            text,
            reasoning,
            usage,
            stream,
            tool: None,
            time,
            seq,
        };
        self.push_row(item);
    }

    pub(super) fn on_tool_call(&mut self, seq: u64, time: u64, data: &ToolCallData) {
        self.tool_calls += 1;
        let idx = self.msgs.len();
        self.msgs.push(MsgItem {
            kind: MsgKind::Tool,
            text: String::new(),
            reasoning: None,
            usage: None,
            stream: None,
            tool: Some(ToolItem {
                call_id: data.call_id.clone(),
                name: data.name.clone(),
                arguments: truncate_chars(&data.arguments, 300),
                // 挂起态：result 未到（时长/错误/结果在 tool/result 时补全）。
                duration_ms: 0,
                is_error: false,
                result: None,
                diffs: Vec::new(),
            }),
            time,
            seq,
        });
        self.tool_index.insert(data.call_id.clone(), idx);
    }

    pub(super) fn on_tool_result(&mut self, time: u64, data: &ToolResultData) {
        // 配对键：tool/result 不带 callId，call_id 在消息内容唯一的 tool-result 块里。
        let Some(block) = data.message.content.iter().find_map(|b| match b {
            ContentBlock::ToolResult(t) => Some(t),
            _ => None,
        }) else {
            // 内容无 tool-result 块（协议漂移/异常）→ 无法配对，忽略；wire 原样保真在 JSONL。
            return;
        };
        let Some(idx) = self.tool_index.remove(&block.tool_call_id) else {
            // result 先于 call / 跨日志片段（孤 result）：没有挂起行可补，忽略。
            return;
        };
        let is_error = data.error.is_some() || block.is_error == Some(true);
        // 结果摘要 = tool-result 块内首文本块（截断 300）；兼容旧形状：退而取消息顶层文本块。
        let result_text = block
            .content
            .iter()
            .find_map(|b| match b {
                ContentBlock::Text(t) => Some(truncate_chars(&t.text, 300)),
                _ => None,
            })
            .or_else(|| {
                data.message.content.iter().find_map(|b| match b {
                    ContentBlock::Text(t) => Some(truncate_chars(&t.text, 300)),
                    _ => None,
                })
            });
        let diffs = fold_diffs(data.meta.as_ref());
        let start_time = self.msgs[idx].time;
        if let Some(tool) = self.msgs[idx].tool.as_mut() {
            // 时长 = result.time − call.time；回放/时钟错乱时 saturating 归 0（不可靠）。
            tool.duration_ms = time.saturating_sub(start_time);
            tool.is_error = is_error;
            tool.result = result_text;
            tool.diffs = diffs;
        }
        if is_error {
            self.errors += 1;
        }
    }

    /// 追加一行并维护"消息数"统计（User/Assistant 行才占；Reasoning/Tool/Notice 不占）。
    pub(super) fn push_row(&mut self, item: MsgItem) {
        if matches!(item.kind, MsgKind::User | MsgKind::Assistant) {
            self.messages += 1;
        }
        self.msgs.push(item);
    }

    pub(super) fn push_notice(&mut self, seq: u64, time: u64, text: String) {
        self.push_row(MsgItem {
            kind: MsgKind::Notice,
            text: truncate_chars(&text, 200),
            reasoning: None,
            usage: None,
            stream: None,
            tool: None,
            time,
            seq,
        });
    }
}
