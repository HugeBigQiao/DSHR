//! 数据投影层：把会话事件流折叠（fold）成内存快照。
//!
//! 主要用途：`engine`（将来直接持有 `Folder`；目前由 `raw::Runtime` 持有）把收到的每条通知喂进
//! [`Folder`]，`Folder::snapshot()` 产出 [`crate::snapshot::SessionSnapshot`]，UI 只消费它。
//! 为什么需要：官方 wire 事件是**面向持久化的追加流**（59 种事件、字段随版本漂移），
//! 不是面向渲染的数据结构。这一层把它折成 UI 能直接用的形状，并**兜住协议漂移**——
//! 字段改名/新增事件不会炸 UI，只是少一个展示项。
//! 关键性质：**纯函数、无 I/O、无时钟**——所以可以用「事件 JSON → 快照相等」直接断言。
//! 同源同巡：在线（SDK 通知）与离线（WireLog JSONL 回放）走**同一套**折叠语义，
//! 因此离线回放可以作为 UI 开发与回归的手段（不 spawn runtime、不烧 token）。
//! 上接：`raw.rs`（`Runtime::Bridge::feed` / `set_status` / `push_local_user_message`）、
//!       `snapshot.rs`（消费者，快照类型定义在那里）。
//! 下接：`dsh_sdk_protocol::session_event`（事件类型）、`rpc`（信封判读）、
//!       `notifications`（已解析的通知）、`fold/event.rs` 与 `fold/render.rs`（本文件的子模块）。
//! 官方对应：无单点对应——语义上对应官方 `packages/core/session` 的会话日志到
//! 「派生历史 / 投影」的那一层（如 `session-projection`、`session-stats`），
//! 但官方按插件分包，dshr 合并在这一处。
//!
//! 模块划分（原单文件 1193 行过大，已拆分；Rust 2018+ 风格：`fold.rs` + `fold/`）：
//! - `fold.rs`（本文件）：折叠态定义、构造、编排方法、Default、共享小工具
//! - `fold/event.rs`：单条事件 → 折叠态推进（`on_*` / `push_row` / `push_notice`）
//! - `fold/render.rs`：纯渲染与解析函数（文本提取、流统计、reason 文本、diff 折叠）
//! - 契约测试在 `tests/fold_projection.rs`（事件 JSON → 快照相等，含在线/离线同源同巡）
use std::collections::HashMap;

use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::notifications;
use dsh_sdk_protocol::notifications::SessionStatus;
use dsh_sdk_protocol::rpc;
use dsh_sdk_protocol::session_event::SessionEvent;

use crate::snapshot::{MsgItem, SessionSnapshot, SessionStats, TurnStat, UsageAgg};

/// 折叠状态机：喂事件 → 内部态推进 → `snapshot()` 出不可变快照。
///
/// 为什么需要：把所有「跨事件才能算出来」的状态（进行中轮、call↔result 配对、token 六桶累加、
/// 流统计）集中在一个结构里，使每次 `snapshot()` 都是一次**纯读取**——调用方无需关心事件顺序。
/// 生命周期：`raw::Runtime` 在 `Start`/`ResetSession` 时 `Folder::new()` 重建（会话 id 换新），
/// `Stop` 时**保留**（让 UI 还能看见已收消息）。
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
            // system/message 是派生 surface 的系统提示，s1 暂不折入消息流。
            SystemMessage { .. } => {}
            AssistantMessage { seq, time, data } => self.on_assistant_message(*seq, *time, data),
            // assistant/attempt 没有提交 surface 消息，不入库不聚合。
            AssistantAttempt { .. } => {}
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
            FeedbackMessagePut { .. } | FeedbackMessageDelete { .. } => {} // 消息反馈 log-only。
            GoalChange { .. } => {} // 目标快照+墓碑；s4 目标视图（UI 未消费，别过度建模）。
            RequestHeader { .. } => {} // 下次请求头快照 → 请求层统计（§11.3）；s2 落库用。
            RequestContext { .. } => {} // 模型路由元数据，同上。
            SessionEndSeed { .. } => {} // 种子边界标记；seq 顺序天然保真，折叠不关心。
            DeliverablesPresented { seq, time, data } => {
                // 先折成一行 Notice；后续再做交付文件卡片/侧栏。
                let paths = data
                    .files
                    .iter()
                    .map(|file| file.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.push_notice(
                    *seq,
                    *time,
                    format!(
                        "交付文件（{}）：{}",
                        data.files.len(),
                        truncate_chars(&paths, 200)
                    ),
                );
            }
            AgentPresetSelected { .. } => {} // preset 快照（最后写入者胜）；s4 会话属性。
            // 增量工具声明变更（官方 0.1.7-rc.2）：surface 事件（tool-addition /
            // tool-removal 块），但 s1 不重建请求历史，故无折叠值；s2 落库时按
            // 事实表评估（请求侧工具历史）。
            DeveloperMessage { .. } => {}
            // 图片卸载决策（官方 0.1.7-rc.2，标 @messageProjection）：只影响后续
            // 模型请求的图片投影，不改消息身份/内容；s1 折叠不消费。
            ImageOffload { .. } => {}
            // 工作区变更时间线标记（官方 0.1.7-rc.2）：摘要本体在 Host（按 seq 取），
            // 日志只有 { turn }；s4 交付/变更视图再消费。
            WorkspaceChanges { .. } => {}
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
            SubagentCatalog { .. } => {}    // 父会话子代理目录；s4 子代理树。
            TeamMember { .. }
            | TeamMessageQueued { .. }
            | TeamMessageDelivered { .. }
            | TeamTask { .. } => {} // 多代理团队内部；UI 未消费。
            ToolWorkflowRunStart { .. }
            | ToolWorkflowRunEnd { .. }
            | ToolWorkflowAgentStart { .. }
            | ToolWorkflowAgentEnd { .. } => {} // 工具工作流内部编排；s4 工作流视图。
            ToolPtcDispatchStart { .. } | ToolPtcDispatch { .. } => {} // PTC mode 子调用：
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

    // on_* 事件处理方法在同一 crate 的 `event.rs`（同类型多 impl 块）。
}

pub mod event;
pub mod render;

// —— 纯辅助（被 event.rs / render.rs 跨模块使用，故 pub(crate)）——

impl Default for Folder {
    fn default() -> Self {
        Self::new()
    }
}

// —— 纯辅助 ——

/// 截断到 ≤max 个字符（按字符数；超长加省略号）。arguments/result/notice 展示摘要用。
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// 消息正文：各 type=text 块按事件序以换行合并（附件等非 text 块不进正文）。
pub(crate) fn text_of(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
