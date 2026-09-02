//! 数据管道 s1：把会话事件流折叠（fold）成内存快照（DESIGN §11.4）。
//!
//! 纯函数、无 I/O：`Folder` 只持内部折叠态；`push_event`/`push_wire_line`/
//! `push_notification` 喂入事件，`snapshot()` 产出 snapshot.rs 的纯数据。
//! 同源同巡：在线（SDK 通知）与离线（WireLog JSONL 回放）走同一折叠语义；
//! s2 落库消费同一快照。只做 s1——s2 落库 sqlite、s3 接 UI、s4 监控页均不在本模块。
use std::collections::HashMap;

use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::notifications;
use dsh_sdk_protocol::notifications::SessionStatus;
use dsh_sdk_protocol::rpc;
use dsh_sdk_protocol::session_event::SessionEvent;
use dsh_sdk_protocol::session_event::message::{
    AssistantMessageData, Message, MessageRole, MessageSource,
};
use dsh_sdk_protocol::session_event::tool::{ToolCallData, ToolResultData};
use dsh_sdk_protocol::session_event::turn::{TurnEndCancelCause, TurnEndData, TurnEndReason};

use crate::snapshot::{
    FileDiff, MsgItem, MsgKind, SessionSnapshot, SessionStats, ToolItem, TurnStat, UsageAgg,
};

/// 折叠状态机：喂事件 → 内部态推进 → `snapshot()` 出不可变快照。
#[derive(Debug)]
pub struct Folder {
    session_id: String,
    title: Option<String>,
    status: Option<SessionStatus>,
    /// 消息流（Tool 行在 tool/call 时插入、tool/result 时原地补全，保证事件序）。
    msgs: Vec<MsgItem>,
    /// call_id → msgs 下标（result 未到的挂起调用；result 到达即摘除）。
    tool_index: HashMap<String, usize>,
    /// 已结算轮。
    closed_turns: Vec<TurnStat>,
    /// 进行中轮（turn/start 后、turn/end 前）。
    open_turn: Option<OpenTurn>,
    usage: UsageAgg,
    steps: u64,
    messages: u64,
    tool_calls: u64,
    errors: u64,
}

/// 进行中轮的折叠态。
#[derive(Debug, Clone)]
struct OpenTurn {
    turn: u64,
    start_time: u64,
    usage: UsageAgg,
}

impl Folder {
    /// 空折叠器（session_id 由通知/回放行带出）。
    pub fn new() -> Self {
        Self {
            session_id: String::new(),
            title: None,
            status: None,
            msgs: Vec::new(),
            tool_index: HashMap::new(),
            closed_turns: Vec::new(),
            open_turn: None,
            usage: UsageAgg::default(),
            steps: 0,
            messages: 0,
            tool_calls: 0,
            errors: 0,
        }
    }

    /// 折叠一条已解析的会话事件（事件不含 sessionId，如需快照带 session_id 请用
    /// `push_wire_line` / `push_notification`，或事后从通知侧补）。
    pub fn push_event(&mut self, ev: &SessionEvent) {
        use SessionEvent::*;
        match ev {
            // —— 折叠：轮 / 步 ——
            TurnStart { time, data, .. } => {
                if let Some(open) = self.open_turn.replace(OpenTurn {
                    turn: data.turn,
                    start_time: *time,
                    usage: UsageAgg::default(),
                }) {
                    // 前一轮未 end（截断日志）→ 强制结算（end/reason 缺失）。
                    self.closed_turns.push(TurnStat {
                        turn: open.turn,
                        start_time: Some(open.start_time),
                        end_time: None,
                        reason: None,
                        usage: open.usage,
                    });
                }
            }
            TurnEnd { time, data, .. } => self.on_turn_end(*time, data),
            // step 数按 step/start 计（截断日志下比 end 侧稳）。
            StepStart { .. } => self.steps += 1,
            // step/end 与 step/start 对称，无额外折叠内容。
            StepEnd { .. } => {}
            // —— 折叠：消息 ——
            UserMessage { seq, time, data } => self.on_user_message(*seq, *time, data),
            AssistantMessage { seq, time, data } => self.on_assistant_message(*seq, *time, data),
            // 忽略：assistant/chunk 每 token 级、量大且无聚合价值——不入库不聚合，
            // 只在会话流式渲染期做内存态（DESIGN §11.3）；正文一律以 assistant/message 为准。
            AssistantChunk { .. } => {}
            // —— 折叠：工具（call ↔ result 按 call_id 配对）——
            ToolCall { seq, time, data } => self.on_tool_call(*seq, *time, data),
            ToolResult { time, data, .. } => self.on_tool_result(*time, data),
            // —— 折叠：会话属性 ——
            SessionTitle { data, .. } => self.title = Some(data.title.clone()),
            // —— 折叠：Notice 行（compaction 是"上下文替换"类系统动作，UI 尚未消费，
            //     折一行小字占位；真正的 surface 替换语义留给 read 层/UI）——
            CompactionStart { seq, time, data } => {
                self.push_notice(
                    *seq,
                    *time,
                    format!("compaction 开始（{}）", data.compaction_id),
                );
            }
            CompactionEnd { seq, time, data } => match &data.error {
                Some(e) => self.push_notice(
                    *seq,
                    *time,
                    format!(
                        "compaction 失败（{}）：{}",
                        data.compaction_id,
                        truncate_chars(e, 160)
                    ),
                ),
                None => self.push_notice(
                    *seq,
                    *time,
                    format!("compaction 结束（{}）", data.compaction_id),
                ),
            },
            CompactionSummary { seq, time, data } => {
                let first = data.summary.iter().find_map(|b| match b {
                    ContentBlock::Text(t) => Some(t.text.clone()),
                    _ => None,
                });
                self.push_notice(
                    *seq,
                    *time,
                    format!("compaction 摘要：{}", first.unwrap_or_default()),
                );
            }
            // —— 以下事件族 s1 忽略（UI/统计尚未消费，注释即扩展点；s2 落库时按事实表再评估）——
            CompactionPrune { .. } => {} // 剪枝计量（影子价格）是内部成本记账，无折叠价值。
            TodoWrite { .. } => {} // todo 整表快照是"日志 UI 状态"（官方注释），非消息流；s4 任务视图。
            FeedbackRecord { .. } => {} // log-only（官方：永不进模型上下文/历史）。
            GoalChange { .. } => {} // 目标快照+墓碑；s4 目标视图（UI 未消费，别过度建模）。
            RequestHeader { .. } => {} // 下次请求头快照 → 请求层统计（§11.3）；s2 落库用。
            RequestContext { .. } => {} // 模型路由元数据，同上。
            SessionEndSeed { .. } => {} // 种子边界标记；seq 顺序天然保真，折叠不关心。
            AgentPresetSelected { .. } => {} // preset 快照（最后写入者胜）；s4 会话属性。
            AgentInboxSpliced { .. } => {} // inbox 增量（多代理内部）；UI 未消费。
            ApprovalAsked { .. } | ApprovalDecided { .. } => {} // 审批流；s3 交互 UI 再建模。
            ApprovalPolicy { .. } | PermissionPreset { .. } => {} // 策略快照；无折叠值。
            CommandRun { .. } | CommandDone { .. } => {} // 命令轨迹 → 请求层（§11.3）；s2 评估。
            HookInvoked { .. } | HookResult { .. } => {} // hook 调用轨迹；无折叠值。
            LlmRetry { .. } | LlmRetryStarted { .. } => {} // 重试链 → 请求层（重试 = 额外 token）；s2 评估。
            PlanMode { .. } | SandboxMode { .. } => {}     // 模式开关快照；s4 会话属性。
            ScheduleChange { .. } => {}                    // 调度变更；UI 未消费。
            SessionTitleLlmRequest { .. } => {}            // 标题生成内部请求快照；无折叠值。
            SubagentDescriptor { .. } => {} // 静态组成声明（每会话至多一次）；s4 子代理视图。
            TeamMember { .. }
            | TeamMessageQueued { .. }
            | TeamMessageDelivered { .. }
            | TeamTask { .. } => {} // 多代理团队内部；UI 未消费。
            ToolWorkflowRunStart { .. }
            | ToolWorkflowRunEnd { .. }
            | ToolWorkflowAgentStart { .. }
            | ToolWorkflowAgentEnd { .. } => {} // 工具工作流内部编排；s4 工作流视图。
            ToolCodeDispatchStart { .. } | ToolCodeDispatch { .. } => {} // code-mode 子调用：
            // run_code 的 diff 已由 tool/result meta 折叠，
            // 子调用不再计（防重复计数）。
            WebDeepSeekSearchLlmRequest { .. } => {} // 搜索辅助请求快照；无折叠值。
            ModelSelection { .. } => {}              // log-only（官方：后续 prompt 组装记录）。
            SessionLogDeepseekDeliveryAccepted { .. } => {} // log-only（官方送达确认）。
            SubagentModelSelectionPolicy { .. } => {} // 每会话至多一次的策略声明；无折叠值。
            Unknown { .. } => {} // 未知类型（插件/新版）：lossless 原样在 wire log，s1 不折叠。
        }
    }

    /// 折叠一条解析后的 SDK 通知（session.event → 事件 + sessionId；session.status → 状态；
    /// 子代理血缘通知忽略——快照是单会话折叠，s2 会话树/目录层再接）。
    pub fn push_notification(&mut self, kind: &notifications::Kind) {
        match kind {
            notifications::Kind::SessionEvent(n) => {
                self.session_id = n.session_id.clone();
                self.push_event(&n.event);
            }
            notifications::Kind::SessionStatus(n) => {
                self.session_id = n.session_id.clone();
                self.status = Some(n.status.clone());
            }
            notifications::Kind::SubagentStarted(_) | notifications::Kind::SubagentFinished(_) => {}
        }
    }

    /// 折叠一行 WireLog JSONL（record.rs 行格式：`{cat, kind, method, eventType?, raw}`）。
    /// 只消费 `cat=dsh, kind=notification` 的行（raw = 原始 JSON-RPC 通知帧）；app 轨迹、
    /// 请求/响应行跳过；非 JSON / 已知通知但内容畸形 → Err。
    pub fn push_wire_line(&mut self, line: &str) -> Result<(), String> {
        let rec: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("wire 行非 JSON：{e}"))?;
        if rec.get("cat").and_then(serde_json::Value::as_str) != Some("dsh") {
            return Ok(()); // cat=app 的应用轨迹（config/spawn/run 等），非 dsh 事件。
        }
        if rec.get("kind").and_then(serde_json::Value::as_str) != Some("notification") {
            return Ok(()); // request / response / unparseable 行：事件只来自通知。
        }
        let raw = rec
            .get("raw")
            .ok_or_else(|| "dsh notification 行缺 raw".to_string())?;
        let method = raw
            .get("method")
            .and_then(serde_json::Value::as_str)
            .or_else(|| rec.get("method").and_then(serde_json::Value::as_str))
            .ok_or_else(|| "raw 缺 method".to_string())?;
        let params = raw
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let notif = rpc::Notification {
            method: method.to_string(),
            params,
        };
        match notifications::parse(&notif) {
            Ok(Some(kind)) => {
                self.push_notification(&kind);
                Ok(())
            }
            // 未知通知方法（协议演进，跳过；与 notifications.rs 的解析策略一致）。
            Ok(None) => Ok(()),
            Err(e) => Err(format!("通知解析失败：{e}")),
        }
    }

    /// 当前快照（不可变、纯数据；每次调用从内部态重建，调用方自取所需字段）。
    pub fn snapshot(&self) -> SessionSnapshot {
        let mut turns = self.closed_turns.clone();
        if let Some(open) = &self.open_turn {
            turns.push(TurnStat {
                turn: open.turn,
                start_time: Some(open.start_time),
                end_time: None,
                reason: None,
                usage: open.usage.clone(),
            });
        }
        let stats = SessionStats {
            turns: turns.len() as u64,
            steps: self.steps,
            messages: self.messages,
            tool_calls: self.tool_calls,
            usage: self.usage.clone(),
            // 两个耗时桶 s1 恒 0：事件无可靠起止对/时间差不可靠，s2 落库后校准（见 snapshot.rs）。
            llm_ms: 0,
            tool_ms: 0,
            errors: self.errors,
        };
        SessionSnapshot {
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            messages: self.msgs.clone(),
            turns,
            stats,
        }
    }

    // —— 以下为内部折叠逻辑 ——

    fn on_turn_end(&mut self, time: u64, data: &TurnEndData) {
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
    fn on_user_message(&mut self, seq: u64, time: u64, msg: &Message) {
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
            tool: None,
            time,
            seq,
        });
    }

    fn on_assistant_message(&mut self, seq: u64, time: u64, data: &AssistantMessageData) {
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
            tool: None,
            time,
            seq,
        };
        self.push_row(item);
    }

    fn on_tool_call(&mut self, seq: u64, time: u64, data: &ToolCallData) {
        self.tool_calls += 1;
        let idx = self.msgs.len();
        self.msgs.push(MsgItem {
            kind: MsgKind::Tool,
            text: String::new(),
            reasoning: None,
            usage: None,
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

    fn on_tool_result(&mut self, time: u64, data: &ToolResultData) {
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
    fn push_row(&mut self, item: MsgItem) {
        if matches!(item.kind, MsgKind::User | MsgKind::Assistant) {
            self.messages += 1;
        }
        self.msgs.push(item);
    }

    fn push_notice(&mut self, seq: u64, time: u64, text: String) {
        self.push_row(MsgItem {
            kind: MsgKind::Notice,
            text: truncate_chars(&text, 200),
            reasoning: None,
            usage: None,
            tool: None,
            time,
            seq,
        });
    }
}

impl Default for Folder {
    fn default() -> Self {
        Self::new()
    }
}

// —— 纯辅助 ——

/// 截断到 ≤max 个字符（按字符数；超长加省略号）。arguments/result/notice 展示摘要用。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// 消息正文：各 type=text 块按事件序以换行合并（附件等非 text 块不进正文）。
fn text_of(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 思考文本：各 type=reasoning 块合并；无 → None。
fn reasoning_of(content: &[ContentBlock]) -> Option<String> {
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
fn reason_text(r: &TurnEndReason) -> String {
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
    }
}

/// oldText/newText → 行数：空/null = 0；否则按 \n 计数、尾随换行只作行终止符
/// （"a\nb" = 2、"a\nb\n" = 2、"a" = 1）。近似值，精确 diff 以 wire meta.diffs 原样为准。
fn line_count(s: &str) -> u64 {
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
fn fold_diffs(meta: Option<&serde_json::Value>) -> Vec<FileDiff> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// 测试助手：事件 JSON（信封 + data 按官方 wire 形状）→ SessionEvent。
    fn ev(j: serde_json::Value) -> SessionEvent {
        serde_json::from_value(j).expect("官方形状事件应可解析")
    }

    fn push(f: &mut Folder, j: serde_json::Value) {
        f.push_event(&ev(j));
    }

    fn usage_msg(turn: u64, step: u64, usage: serde_json::Value) -> serde_json::Value {
        json!({
            "type": "assistant/message", "seq": step + 10, "time": 150 + step * 10,
            "data": {
                "turn": turn, "step": step,
                "message": {
                    "id": format!("m-a{turn}-{step}"), "role": "assistant",
                    "content": [{"type": "text", "text": format!("答 {turn}.{step}")}],
                    "source": {"kind": "model", "provider": "deepseek", "model": "flash"}
                },
                "usage": usage
            }
        })
    }

    /// 主流程测试：一轮完整会话（user → reasoning+text+usage → turn/end），
    /// 断言消息文本/reasoning/usage 归位 + 轮统计 + 会话汇总。
    #[test]
    fn full_turn_folds_messages_turn_and_stats() {
        let mut f = Folder::new();
        let events = [
            json!({"type":"turn/start","seq":1,"time":100,"data":{"turn":1}}),
            json!({"type":"user/message","seq":2,"time":120,"data":{
                "id":"m-u1","role":"user",
                "content":[{"type":"text","text":"第一问"}],
                "source":{"kind":"user"}}}),
            json!({"type":"step/start","seq":3,"time":130,"data":{"turn":1,"step":1}}),
            json!({"type":"assistant/message","seq":4,"time":180,"data":{
                "turn":1,"step":1,
                "message":{
                    "id":"m-a1","role":"assistant",
                    "content":[
                        {"type":"reasoning","text":"先想想"},
                        {"type":"text","text":"答案是好"}
                    ],
                    "source":{"kind":"model","provider":"deepseek","model":"flash"}},
                "usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":2,
                         "cacheWriteTokens":1,"reasoningTokens":3,"totalTokens":21}}}),
            json!({"type":"turn/end","seq":5,"time":200,"data":{"turn":1,"reason":{"kind":"completed"}}}),
        ];
        for e in &events {
            f.push_event(&ev(e.clone()));
        }
        let s = f.snapshot();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[0].kind, MsgKind::User);
        assert_eq!(s.messages[0].text, "第一问");
        assert_eq!(s.messages[1].kind, MsgKind::Assistant);
        assert_eq!(s.messages[1].text, "答案是好");
        assert_eq!(s.messages[1].reasoning.as_deref(), Some("先想想"));
        let usage = s.messages[1]
            .usage
            .as_ref()
            .expect("assistant 行应带 usage");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.total_tokens, Some(21));

        assert_eq!(s.turns.len(), 1);
        let t = &s.turns[0];
        assert_eq!(t.turn, 1);
        assert_eq!(t.start_time, Some(100));
        assert_eq!(t.end_time, Some(200));
        assert_eq!(t.reason.as_deref(), Some("completed"));
        assert_eq!(
            t.usage,
            UsageAgg {
                input: 10,
                output: 5,
                cache_read: 2,
                cache_write: 1,
                reasoning: 3,
                total: 21
            }
        );
        assert_eq!(
            s.stats,
            SessionStats {
                turns: 1,
                steps: 1,
                messages: 2,
                tool_calls: 0,
                usage: UsageAgg {
                    input: 10,
                    output: 5,
                    cache_read: 2,
                    cache_write: 1,
                    reasoning: 3,
                    total: 21
                },
                llm_ms: 0,
                tool_ms: 0,
                errors: 0,
            }
        );
    }

    /// 只有思考没有正文 → Reasoning 行（不占 messages 统计），usage 照常入桶。
    #[test]
    fn reasoning_only_message_becomes_reasoning_row() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"assistant/message","seq":1,"time":100,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-r1","role":"assistant",
                "content":[{"type":"reasoning","text":"纯思考"}],
                "source":{"kind":"model","provider":"deepseek","model":"flash"}},
            "usage":{"inputTokens":7,"outputTokens":0}}}),
        );
        let s = f.snapshot();
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].kind, MsgKind::Reasoning);
        assert_eq!(s.messages[0].reasoning.as_deref(), Some("纯思考"));
        assert!(s.messages[0].text.is_empty());
        assert_eq!(s.messages[0].usage.as_ref().unwrap().input_tokens, 7);
        // Reasoning 行不占消息数；usage 已入桶。
        assert_eq!(s.stats.messages, 0);
        assert_eq!(s.stats.usage.input, 7);
    }

    /// tool/call ↔ tool/result 配对：结果文本 + duration + meta.diffs 行数折叠。
    #[test]
    fn tool_call_result_pairing_folds_diff_line_counts() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"turn/start","seq":1,"time":100,"data":{"turn":1}}),
        );
        push(
            &mut f,
            json!({"type":"user/message","seq":2,"time":110,"data":{
            "id":"m-u1","role":"user",
            "content":[{"type":"text","text":"查一下 a.rs"}],
            "source":{"kind":"user"}}}),
        );
        push(
            &mut f,
            json!({"type":"assistant/message","seq":3,"time":120,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-a1","role":"assistant",
                "content":[{"type":"text","text":"让我查"}],
                "source":{"kind":"model","provider":"deepseek","model":"flash"}},
            "usage":null}}),
        );
        push(
            &mut f,
            json!({"type":"tool/call","seq":4,"time":300,"data":{
            "turn":1,"step":1,"callId":"c1","name":"read_file",
            "arguments":"{\"path\":\"a.rs\"}"}}),
        );
        push(
            &mut f,
            json!({"type":"tool/result","seq":5,"time":350,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-t1","role":"user",
                "content":[{"type":"tool-result","toolCallId":"c1","content":[
                    {"type":"text","text":"文件内容：第1行\n第2行"}],"isError":false}],
                "source":{"kind":"tool","callId":"c1"}},
            "meta":{"diffs":[
                {"path":"a.rs","oldText":"ab\ncd","newText":"ab\ncd\nef"},
                {"path":"new.txt","oldText":null,"newText":"x\ny"},
                {"path":"empty.txt"}]}}}),
        );
        let s = f.snapshot();
        assert_eq!(s.messages.len(), 3); // user + assistant + tool 行（Tool 不占 messages 数）
        assert_eq!(s.messages[2].kind, MsgKind::Tool);
        let tool = s.messages[2].tool.as_ref().expect("tool 行应带卡片");
        assert_eq!(tool.call_id, "c1");
        assert_eq!(tool.name, "read_file");
        assert!(tool.arguments.contains("a.rs"));
        assert_eq!(tool.duration_ms, 50);
        assert!(!tool.is_error);
        assert_eq!(tool.result.as_deref(), Some("文件内容：第1行\n第2行"));
        assert_eq!(tool.diffs.len(), 3);
        assert_eq!(tool.diffs[0].path, "a.rs");
        assert_eq!(tool.diffs[0].removed, 2); // "ab\ncd"
        assert_eq!(tool.diffs[0].added, 3); // "ab\ncd\nef"
        assert_eq!(tool.diffs[1].path, "new.txt");
        assert_eq!(tool.diffs[1].removed, 0); // oldText null → 0
        assert_eq!(tool.diffs[1].added, 2);
        assert_eq!(tool.diffs[2].path, "empty.txt");
        assert_eq!((tool.diffs[2].removed, tool.diffs[2].added), (0, 0));
        assert_eq!(s.stats.tool_calls, 1);
        assert_eq!(s.stats.errors, 0);
        // 配对后挂起表已摘除（快照不可见，靠 tool.diffs 补全证明在 msgs 原地生效）。
    }

    /// 工具错误 + turn/end 错误都计入 errors；reason 带 code。
    #[test]
    fn tool_and_turn_errors_are_counted() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"turn/start","seq":1,"time":100,"data":{"turn":1}}),
        );
        push(
            &mut f,
            json!({"type":"tool/call","seq":2,"time":110,"data":{
            "turn":1,"step":1,"callId":"c1","name":"read_file","arguments":"{}"}}),
        );
        push(
            &mut f,
            json!({"type":"tool/result","seq":3,"time":120,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-t1","role":"user",
                "content":[{"type":"tool-result","toolCallId":"c1","content":[
                    {"type":"text","text":"读不了"}],"isError":true}],
                "source":{"kind":"tool","callId":"c1"}},
            "error":{"name":"ReadError","code":"READ_ERR"}}}),
        );
        push(
            &mut f,
            json!({"type":"turn/end","seq":4,"time":130,"data":{
            "turn":1,"reason":{"kind":"error",
                               "error":{"message":"模型崩了","code":"LLM_ERR"}}}}),
        );
        let s = f.snapshot();
        let tool = s.messages[0].tool.as_ref().expect("tool 行");
        assert!(tool.is_error);
        assert_eq!(s.stats.errors, 2); // tool is_error + turn/end error
        assert_eq!(
            s.turns[0].reason.as_deref(),
            Some("error/LLM_ERR: 模型崩了")
        );
    }

    /// usage 六桶跨两轮累加；adapter 未报的可选桶按 0。
    #[test]
    fn usage_buckets_aggregate_across_turns() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"turn/start","seq":1,"time":100,"data":{"turn":1}}),
        );
        push(
            &mut f,
            usage_msg(
                1,
                1,
                json!({"inputTokens":100,"outputTokens":20,
            "cacheReadTokens":5,"cacheWriteTokens":3,"reasoningTokens":2}),
            ),
        ); // 无 totalTokens
        push(
            &mut f,
            json!({"type":"turn/end","seq":3,"time":200,"data":{"turn":1,"reason":{"kind":"completed"}}}),
        );
        push(
            &mut f,
            json!({"type":"turn/start","seq":4,"time":300,"data":{"turn":2}}),
        );
        push(
            &mut f,
            usage_msg(
                2,
                1,
                json!({"inputTokens":50,"outputTokens":8,"totalTokens":58}),
            ),
        ); // 无缓存桶
        push(
            &mut f,
            json!({"type":"turn/end","seq":6,"time":400,"data":{"turn":2,"reason":{"kind":"completed"}}}),
        );
        let s = f.snapshot();
        assert_eq!(s.turns.len(), 2);
        // 第一轮：totalTokens 未报 → total 桶 0；第二轮：缓存桶未报 → 0。
        assert_eq!(
            s.turns[0].usage,
            UsageAgg {
                input: 100,
                output: 20,
                cache_read: 5,
                cache_write: 3,
                reasoning: 2,
                total: 0
            }
        );
        assert_eq!(
            s.turns[1].usage,
            UsageAgg {
                input: 50,
                output: 8,
                cache_read: 0,
                cache_write: 0,
                reasoning: 0,
                total: 58
            }
        );
        assert_eq!(
            s.stats.usage,
            UsageAgg {
                input: 150,
                output: 28,
                cache_read: 5,
                cache_write: 3,
                reasoning: 2,
                total: 58
            }
        );
        assert_eq!(s.stats.turns, 2);
        assert_eq!(s.stats.messages, 2);
    }

    /// assistant/chunk 不入聚合：消息文本以 assistant/message 为准，行数与统计不受 chunk 影响。
    #[test]
    fn chunks_do_not_touch_text_or_stats() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"user/message","seq":1,"time":100,"data":{
            "id":"m-u1","role":"user",
            "content":[{"type":"text","text":"流式"}],
            "source":{"kind":"user"}}}),
        );
        for (i, part) in ["一点点", "又一点"].iter().enumerate() {
            push(
                &mut f,
                json!({"type":"assistant/chunk","seq":(2 + i) as u64,"time":110,"data":{
                "turn":1,"step":1,
                "chunk":{"type":"text-delta","index":0,"text":part}}}),
            );
        }
        push(
            &mut f,
            json!({"type":"assistant/message","seq":4,"time":150,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-a1","role":"assistant",
                "content":[{"type":"text","text":"最终完整文本"}],
                "source":{"kind":"model","provider":"deepseek","model":"flash"}},
            "usage":null}}),
        );
        let s = f.snapshot();
        assert_eq!(s.messages.len(), 2);
        assert_eq!(s.messages[1].text, "最终完整文本"); // 不拼 chunk 文本
        assert_eq!(s.messages[1].reasoning, None);
        assert_eq!(s.stats.messages, 2);
        assert_eq!(s.stats.turns, 0); // 无 turn 事件也容忍
    }

    /// 未结算轮：turns 含进行中轮（end/reason None）；挂起 tool 保留挂起态且行序 = call 序。
    #[test]
    fn pending_tool_and_open_turn_survive_partial_log() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"turn/start","seq":1,"time":100,"data":{"turn":1}}),
        );
        push(
            &mut f,
            json!({"type":"tool/call","seq":2,"time":110,"data":{
            "turn":1,"step":1,"callId":"c1","name":"read_file","arguments":"{\"p\":1}"}}),
        );
        push(
            &mut f,
            json!({"type":"tool/call","seq":3,"time":120,"data":{
            "turn":1,"step":1,"callId":"c2","name":"list_dir","arguments":"{}"}}),
        );
        push(
            &mut f,
            json!({"type":"tool/result","seq":4,"time":140,"data":{
            "turn":1,"step":1,
            "message":{
                "id":"m-t2","role":"user",
                "content":[{"type":"tool-result","toolCallId":"c2","content":[
                    {"type":"text","text":"目录清单"}],"isError":false}],
                "source":{"kind":"tool","callId":"c2"}}}}),
        );
        let s = f.snapshot();
        assert_eq!(s.messages.len(), 2);
        // 行序 = call 序（result 晚到也原地补全，不重排）。
        let t1 = s.messages[0].tool.as_ref().unwrap();
        assert_eq!(t1.call_id, "c1");
        assert_eq!(t1.result, None); // c1 挂起
        assert_eq!(t1.duration_ms, 0);
        assert!(!t1.is_error);
        let t2 = s.messages[1].tool.as_ref().unwrap();
        assert_eq!(t2.call_id, "c2");
        assert_eq!(t2.result.as_deref(), Some("目录清单"));
        assert_eq!(t2.duration_ms, 20);
        assert_eq!(s.stats.tool_calls, 2);
        assert_eq!(s.stats.errors, 0);
        // 进行中轮：end_time/reason None。
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].end_time, None);
        assert_eq!(s.turns[0].reason, None);
        assert_eq!(s.stats.turns, 1);
    }

    /// session/title + session.status 通知 → 快照属性；子代理血缘通知被忽略。
    #[test]
    fn title_status_and_subagent_notifications() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"session/title","seq":1,"time":100,"data":{
            "title":"会话标题","messageSeqs":[],"source":{"kind":"user"}}}),
        );
        f.push_notification(&notifications::Kind::SessionStatus(
            notifications::SessionStatusNotification {
                session_id: "s-9".to_string(),
                status: notifications::SessionStatus::Running,
            },
        ));
        f.push_notification(&notifications::Kind::SubagentStarted(
            notifications::SubagentStartedNotification {
                parent_session_id: "s-9".to_string(),
                child_session_id: "s-10".to_string(),
            },
        ));
        let s = f.snapshot();
        assert_eq!(s.title.as_deref(), Some("会话标题"));
        assert_eq!(s.session_id, "s-9");
        assert_eq!(s.status, Some(notifications::SessionStatus::Running));
        assert!(s.messages.is_empty()); // 子代理通知不产行
    }

    /// compaction 事务 → Notice 行（start/摘要/end）；prune 不产行。
    #[test]
    fn compaction_folds_to_notice_rows() {
        let mut f = Folder::new();
        push(
            &mut f,
            json!({"type":"compaction/start","seq":1,"time":100,"data":{"compactionId":"c-1"}}),
        );
        push(
            &mut f,
            json!({"type":"compaction/summary","seq":2,"time":120,"data":{
            "compactionId":"c-1","summary":[{"type":"text","text":"这是压缩后的总结"}],
            "shadowedRange":{"start":3,"end":5},"shadowedSeqs":[3,4,5],
            "shadowedTokenCount":120,"provider":"deepseek","model":"flash"}}),
        );
        push(
            &mut f,
            json!({"type":"compaction/prune","seq":3,"time":130,"data":{
            "shadowedRange":{"start":3,"end":4},"shadowedSeqs":[],"shadowedTokenCount":5}}),
        );
        push(
            &mut f,
            json!({"type":"compaction/end","seq":4,"time":140,"data":{"compactionId":"c-1"}}),
        );
        let s = f.snapshot();
        let notices: Vec<&str> = s
            .messages
            .iter()
            .filter(|m| m.kind == MsgKind::Notice)
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(notices.len(), 3); // prune 不产行
        assert!(notices[0].contains("compaction 开始"));
        assert!(notices[1].contains("这是压缩后的总结"));
        assert!(notices[2].contains("compaction 结束"));
        assert_eq!(s.stats.messages, 0); // Notice 不占消息数
        assert_eq!(s.stats.turns, 0);
    }

    /// WireLog JSONL 回放（record.rs 行格式）：session.event / session.status 行折叠，
    /// app/request/未知通知行跳过，坏行 Err。
    #[test]
    fn wire_line_replay_folds_notifications_and_skips_others() {
        let mut f = Folder::new();
        // session.event 通知行（raw = 原始帧，transport.rs 的 record_recv 形状）。
        let ev_line = json!({
        "t": 1, "cat": "dsh", "dir": "recv", "kind": "notification",
        "method": "session.event", "eventType": "user/message",
        "raw": {"jsonrpc": "2.0", "method": "session.event", "params": {
            "sessionId": "s-w1",
            "event": {"type": "user/message", "seq": 10, "time": 1000, "data": {
                "id": "m1", "role": "user",
                "content": [{"type": "text", "text": "回放你好"}],
                "source": {"kind": "user"}}}}
        }})
        .to_string();
        f.push_wire_line(&ev_line).expect("event 行应可折叠");
        // session.status 通知行。
        let st_line = json!({
            "t": 2, "cat": "dsh", "dir": "recv", "kind": "notification",
            "method": "session.status",
            "raw": {"jsonrpc": "2.0", "method": "session.status",
                    "params": {"sessionId": "s-w1", "status": "running"}}})
        .to_string();
        f.push_wire_line(&st_line).expect("status 行应可折叠");
        // app 轨迹行 / 请求行 / 未知通知方法行：跳过。
        f.push_wire_line(&json!({"t":3,"cat":"app","kind":"config.loaded","data":{}}).to_string())
            .expect("app 行应跳过");
        f.push_wire_line(
            &json!({"t":4,"cat":"dsh","dir":"send","kind":"request",
                                 "id":1,"method":"initialize","raw":{"jsonrpc":"2.0","id":1,
                                 "method":"initialize","params":{}}})
            .to_string(),
        )
        .expect("请求行应跳过");
        f.push_wire_line(
            &json!({"t":5,"cat":"dsh","dir":"recv","kind":"notification",
                                 "method":"subagent.started","raw":{"jsonrpc":"2.0",
                                 "method":"subagent.started","params":{"parentSessionId":"s-w1",
                                 "childSessionId":"s-w2"}}})
            .to_string(),
        )
        .expect("未知通知方法应跳过");
        let s = f.snapshot();
        assert_eq!(s.session_id, "s-w1");
        assert_eq!(s.status, Some(notifications::SessionStatus::Running));
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].kind, MsgKind::User);
        assert_eq!(s.messages[0].text, "回放你好");
        assert_eq!(s.messages[0].seq, 10);
        // 坏行 → Err（不 panic、不改变状态）。
        assert!(f.push_wire_line("这不是 JSON").is_err());
        assert!(f.push_wire_line("").is_err());
        assert_eq!(f.snapshot().messages.len(), 1);
    }

    /// 事件信封缺 data/字段畸形 → 降级 Unknown（协议宽容），不 panic、不折叠。
    #[test]
    fn malformed_known_event_degrades_to_unknown_and_is_ignored() {
        let mut f = Folder::new();
        // user/message data 缺 source（Message 必填字段）→ fallback 降级 Unknown。
        push(
            &mut f,
            json!({"type":"user/message","seq":1,"time":100,"data":{
            "id":"m-bad","role":"user","content":[{"type":"text","text":"hi"}]}}),
        );
        let s = f.snapshot();
        assert!(s.messages.is_empty());
        assert_eq!(s.stats.messages, 0);
    }
}
