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
    AssistantAttemptData, AssistantMessageData, Message, MessageRole, MessageSource,
};
use dsh_sdk_protocol::session_event::tool::{ToolCallData, ToolResultData};
use dsh_sdk_protocol::session_event::turn::{TurnEndData, TurnEndReason};

use super::render::{fold_diffs, reason_text, reasoning_of, stream_summary, text_of};
use super::{Folder, TurnStat, UsageAgg, truncate_chars};
use crate::snapshot::{MsgItem, MsgKind, ToolItem};

impl Folder {
    pub(super) fn on_turn_end(&mut self, time: u64, data: &TurnEndData) {
        // 结算 = 把「进行中轮」（`open_turn`，它累积了本轮 usage）搬进 `closed_turns`。
        // 三种情况都要能收尾（**顺序**就是下面的分支顺序）：
        //   ① 轮号一致 → 正常结算：起点与 usage 都取自 `open_turn`；
        //   ② 轮号不一致（截断日志/跨段拼接里冒出一个新的 end：旧轮还没收到 end 就先来了别的轮的）
        //      → 旧轮按「未结算」先收尾，再给这个 end 造一条只有结束侧的记录。宁可多一行，
        //      也不要把两个轮的 usage 错配到一起（那会让成本统计整体偏掉）；
        //   ③ 压根没有 `open_turn`（日志从中间开始，或只截到 end）→ 也只造结束侧记录。
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
        // 错误轮单独计数：这是 `errors` 的两个口径之一（另一个是工具失败，见 `on_tool_result`）。
        // 单独计而不是事后扫字符串，是因为「一轮结束得不好」与「某个工具失败」在语义上不同，
        // 而下游（监控页/导出）要能分开看（§8.3）。
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

    /// 折一条 `user/message`：**所有来源都折成行**。
    ///
    /// 人类输入（`source.kind = user`）折成 `User` 行；程序化注入（goal 续跑 / webhook /
    /// runtime-context / plan-mode / 技能指令……官方把它们**都写成 role=user 的程序化输入**）
    /// 折成 `Injected` 行并带上来源文本。
    ///
    /// 为什么不再直接丢（2026-09-29 用户要求「能记录多少就记多少」）：它们是「模型当时看到
    /// 了什么」的事实，此前 `return` 掉之后库里与导出里都没有，只能回 wire log 里翻。
    /// **显示策略归 UI**（默认不渲染 Injected 行），记录与显示从此分开。
    pub(super) fn on_user_message(&mut self, seq: u64, time: u64, msg: &Message) {
        // 两道闸门，顺序不能换：
        // ① 角色：`role != user` 的帧（官方偶尔把别的角色也走这个通道）不从这条路入流；
        // ② 来源：`source.kind = user` 才是人类输入，其余都是程序化注入。
        // 两者**都入流**，只是 kind 不同（User / Injected）——「记录全部、显示靠 UI 过滤」的分工
        // 就落在这两行上（2026-09-29 用户要求「能记多少记多少」）。
        if msg.role != MessageRole::User {
            return;
        }
        if matches!(msg.source, MessageSource::User { .. }) {
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
                turn: self.open_turn.as_ref().map(|t| t.turn),
                step: self.cur_step,
                source: "user".to_string(),
                error: None,
            });
        } else {
            self.on_injected_message(seq, time, msg, None, None);
        }
    }

    /// 折一条**程序化注入**的消息（`system/message`、`developer/message`、以及非人类来源的
    /// `user/message`）：kind = `Injected`，来源文本进 `source` 列。
    ///
    /// `turn`/`step` 优先用事件自带的值（system/developer 带），没有则沿用当前进行中的轮/步。
    pub(super) fn on_injected_message(
        &mut self,
        seq: u64,
        time: u64,
        msg: &Message,
        turn: Option<u64>,
        step: Option<u64>,
    ) {
        // 注入行也保真：正文与思考照取（如 runtime-context 的 sections 会被拼成文本），
        // 只是 kind 记 `Injected`——它的价值在「模型当时看到了什么」，不在聊天视图里好不好看。
        let text = text_of(&msg.content);
        let reasoning = reasoning_of(&msg.content);
        self.push_row(MsgItem {
            kind: MsgKind::Injected,
            text,
            reasoning,
            usage: None,
            stream: None,
            tool: None,
            time,
            seq,
            turn: turn.or_else(|| self.open_turn.as_ref().map(|t| t.turn)),
            step: step.or(self.cur_step),
            source: source_kind_of(&msg.source),
            error: None,
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
            turn: Some(data.turn),
            step: Some(data.step),
            // 模型产出的消息：source.kind 必为 model（官方 AssistantMessage 的 source 就是 ModelMessageSource）。
            source: source_kind_of(&data.message.source),
            error: None,
        };
        self.push_row(item);
    }

    /// 折一条 `assistant/attempt`：一次**没有提交 surface 消息**的模型尝试。
    ///
    /// 为什么值得一行：中断/失败/重试场景下常常只剩这个可查——它带 turn/step 与流摘要，
    /// 是「模型确实跑过、token 确实花过」的证据（官方注释：attempt 不提交 surface 消息）。
    pub(super) fn on_assistant_attempt(
        &mut self,
        seq: u64,
        time: u64,
        data: &AssistantAttemptData,
    ) {
        // 只落「轮/步 + 流摘要」，不产正文：这次尝试**没有**要展示的消息（官方语义就是如此），
        // 所以正文留空、思考留空，靠 `stream` 里的 chunk 数与首 token 时间说明「确实跑过」。
        self.push_row(MsgItem {
            kind: MsgKind::Attempt,
            text: String::new(),
            reasoning: None,
            usage: None,
            stream: data.stream.as_deref().map(stream_summary),
            tool: None,
            time,
            seq,
            turn: Some(data.turn),
            step: Some(data.step),
            source: "model".to_string(),
            error: None,
        });
    }

    pub(super) fn on_tool_call(&mut self, seq: u64, time: u64, data: &ToolCallData) {
        // 方法：**先挂起一行**（只有参数，结果字段留空），并把行号记进 `tool_index`，
        // 等 `tool/result` 回来时按 callId 回填。
        // 为什么这样：call 与 result 是**两条独立事件**（中间可能隔很久，甚至永远不来——
        // 工具被中断时），先立行才能让「调用了但没结果」在界面上有个位置；
        // 也正因为要等回填，工具行只能变成两条事件合并的结果，不能等 result 才一次建行。
        // 目的：行序 = 事件序（时间线可靠），且统计（工具数/失败数/耗时）能逐条累计。
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
                // 不截断：库/导出要保真（用户 2026-09-29 要求），截断交给渲染层。
                arguments: data.arguments.clone(),
                // 挂起态：result 未到（时长/错误/结果在 tool/result 时补全）。
                duration_ms: 0,
                is_error: false,
                error: None,
                result: None,
                diffs: Vec::new(),
                meta: None,
            }),
            time,
            seq,
            turn: Some(data.turn),
            step: Some(data.step),
            source: "model".to_string(),
            error: None,
        });
        self.tool_index.insert(data.call_id.clone(), idx);
    }

    pub(super) fn on_tool_result(&mut self, time: u64, data: &ToolResultData) {
        // 方法（三步）：① 从内容块里抠出 `callId` 当配对键 → ② 用索引找到挂起的那一行 →
        // ③ 回填结果/时长/失败原因/原样 meta。
        // 为什么键要抠：`tool/result` 的信封里**没有** callId，它藏在消息内容唯一的 tool-result
        // 块里（协议如此），所以配对必须读内容而没法只看元数据。
        // 目的：工具事实（成功/失败/耗时/改了哪些文件）只有在配对后才是完整的，
        // 而它们正是 `tool_calls` / `file_ops` 两张表的唯一来源。
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
        // 失败判据取两个来源的**或**：信封上的 error（结构化失败）与块上的 isError（工具自己
        // 报的失败）。两者都可能单独出现——只取一个就会漏掉另一类失败。
        let is_error = data.error.is_some() || block.is_error == Some(true);
        // 结果正文 = tool-result 块内首文本块（**不截断**）；兼容旧形状：退而取消息顶层文本块。
        let result_text = block
            .content
            .iter()
            .find_map(|b| match b {
                ContentBlock::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .or_else(|| {
                data.message.content.iter().find_map(|b| match b {
                    ContentBlock::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
            });
        // 失败原因：优先官方给的用户可见 reason，退而用 `code: name`（用户要「失败原因」）。
        let error_text = data.error.as_ref().map(|e| {
            e.reason
                .clone()
                .unwrap_or_else(|| format!("{}: {}", e.code, e.name))
        });
        let diffs = fold_diffs(data.meta.as_ref());
        let start_time = self.msgs[idx].time;
        // 行级补全：失败原因与轮/步（call 侧已经写了，这里只是兜底补缺）。
        self.msgs[idx].error = error_text.clone();
        if self.msgs[idx].turn.is_none() {
            self.msgs[idx].turn = Some(data.turn);
        }
        if self.msgs[idx].step.is_none() {
            self.msgs[idx].step = Some(data.step);
        }
        if let Some(tool) = self.msgs[idx].tool.as_mut() {
            // 时长 = result.time − call.time；回放/时钟错乱时 saturating 归 0（不可靠）。
            tool.duration_ms = time.saturating_sub(start_time);
            tool.is_error = is_error;
            tool.error = error_text;
            tool.result = result_text;
            tool.diffs = diffs;
            // 原样 meta（含 fs 工具的完整 diff 正文）：库里要能回答「到底改了什么」。
            tool.meta = data.meta.clone();
        }
        if is_error {
            self.errors += 1;
        }
    }

    /// 追加一行并维护"消息数"统计（User/Assistant 行才占；其余种类不占——它们不是对话消息）。
    pub(super) fn push_row(&mut self, item: MsgItem) {
        // 「消息数」在此处唯一地累加：口径是「只有 User/Assistant 行算对话消息」。
        // 为什么集中在这里而不是各 `on_*` 里各自加：口径要能被一处读出来，否则加一种行
        //（Injected/Attempt/Notice）就可能有人顺手把计数也加了，统计从此不可比。
        if matches!(item.kind, MsgKind::User | MsgKind::Assistant) {
            self.messages += 1;
        }
        self.msgs.push(item);
    }

    /// 折一行系统小字（compaction / 重试等；text 是**一行简述**，故仍截断）。
    pub(super) fn push_notice(&mut self, seq: u64, time: u64, text: String) {
        self.push_notice_with(seq, time, text, None);
    }

    /// 同上，但带**失败原因**（重试/本地失败等：`error` 列单独存，导出与监控页可直接筛）。
    pub(super) fn push_notice_with(
        &mut self,
        seq: u64,
        time: u64,
        text: String,
        error: Option<String>,
    ) {
        self.push_row(MsgItem {
            kind: MsgKind::Notice,
            text: truncate_chars(&text, 500),
            reasoning: None,
            usage: None,
            stream: None,
            tool: None,
            time,
            seq,
            turn: self.open_turn.as_ref().map(|t| t.turn),
            step: self.cur_step,
            source: String::new(),
            error,
        });
    }
}

/// `MessageSource` → 它的 wire kind 文本（如 `user` / `model` / `runtime-context`）。
///
/// 为什么用 serde 序列化而不是穷尽 match：枚举用 `#[serde(tag = "kind", rename_all = "kebab-case")]`
/// 定义，序列化结果就是 wire 上的文本——**单一真源**，将来加变体不用在这里补一行
///（这类「第四处维护点」正是本项目反复吃亏的地方）。
fn source_kind_of(source: &MessageSource) -> String {
    serde_json::to_value(source)
        .ok()
        .and_then(|v| v.get("kind")?.as_str().map(str::to_string))
        .unwrap_or_default()
}
