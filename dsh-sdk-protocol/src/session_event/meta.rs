//! `SessionEvent` 的元数据访问器（与枚举本体分开，控制父文件行数 ≤350）。
//!
//! 主要用途：提供五个只读访问器——`event_type` / `time` / `seq` / `turn_step` / `degraded_event`，
//! 让消费方不必 match 59 个变体就能拿到排序与落库所需的公共列，以及识别「疑似协议漂移」。
//! 为什么需要：枚举本体是「协议面」（跟官方走），访问器是「消费面」（跟本地库表走）；
//! 分开后官方加事件只改枚举+fallback，而落库列变化只改这里。另外这四个方法都是**穷尽匹配**，
//! 官方新增变体时编译器会强制在这里补一行——这是同步纪律的第三处抓手（另两处：
//! 枚举变体、fallback 分发）。
//! 上接：`dshr-state` 的 record / store（events 表的 type/time/seq/turn/step 列）、
//!       `dsh-sdk-client` 的 api.rs（判断事件种类）。
//! 下接：`session_event.rs` 的枚举本体、`retry`（`LlmRetryData` 的 turn/step 藏在联合变体里）。
//!
//! 枚举本体在父模块 `session_event.rs`；本文件只有 `impl SessionEvent` 的只读访问器：
//! `event_type` / `time` / `seq` / `turn_step`。事件 data 的官方引用见各变体注释。
//!
//! 官方对应：packages/core/session/src/types.ts 的 SessionEvent（信封上的 type/seq/time
//! 字段本体）；`turn_step` 无官方对应——是 dshr 为落 events 表的 turn/step 列而加的便利方法。
use super::SessionEvent;
use super::retry;

impl SessionEvent {
    /// wire 类型字符串（events 表的 type 列）。
    ///
    /// # 返回
    /// 与 wire 上 `type` 逐字一致的静态字符串（如 `"turn/start"`）；
    /// `Unknown` 变体返回稳定占位 `"unknown"`（原始串可用事件的 data/上游帧取，见实现注释）。
    ///
    /// 为什么需要：落库与对账都以 wire 字符串为键，返回 `&'static str` 可免分配、
    /// 也避免调用方手写字面量写错。
    pub fn event_type(&self) -> &'static str {
        use SessionEvent::*;
        match self {
            TurnStart { .. } => "turn/start",
            TurnEnd { .. } => "turn/end",
            StepStart { .. } => "step/start",
            StepEnd { .. } => "step/end",
            UserMessage { .. } => "user/message",
            SystemMessage { .. } => "system/message",
            AssistantMessage { .. } => "assistant/message",
            AssistantAttempt { .. } => "assistant/attempt",
            ToolCall { .. } => "tool/call",
            ToolResult { .. } => "tool/result",
            TodoWrite { .. } => "todo/write",
            RequestHeader { .. } => "request/header",
            RequestContext { .. } => "request/context",
            SessionEndSeed { .. } => "session/end-seed",
            DeliverablesPresented { .. } => "deliverables/presented",
            DeveloperMessage { .. } => "developer/message",
            ImageOffload { .. } => "image/offload",
            WorkspaceChanges { .. } => "workspace/changes",
            AgentPresetSelected { .. } => "agent-preset/selected",
            AgentInboxSpliced { .. } => "agent/inbox/spliced",
            ApprovalAsked { .. } => "approval/asked",
            ApprovalDecided { .. } => "approval/decided",
            ApprovalPolicy { .. } => "approval/policy",
            PermissionPreset { .. } => "permission/preset",
            CommandRun { .. } => "command/run",
            CommandDone { .. } => "command/done",
            CompactionStart { .. } => "compaction/start",
            CompactionEnd { .. } => "compaction/end",
            CompactionPrune { .. } => "compaction/prune",
            CompactionSummary { .. } => "compaction/summary",
            FeedbackRecord { .. } => "feedback/record",
            FeedbackMessagePut { .. } => "feedback/message-put",
            FeedbackMessageDelete { .. } => "feedback/message-delete",
            GoalChange { .. } => "goal/change",
            HookInvoked { .. } => "hook/invoked",
            HookResult { .. } => "hook/result",
            LlmRetry { .. } => "llm/retry",
            LlmRetryStarted { .. } => "llm/retry-started",
            PlanMode { .. } => "plan/mode",
            SandboxMode { .. } => "sandbox/mode",
            ScheduleChange { .. } => "schedule/change",
            SessionTitle { .. } => "session/title",
            SessionTitleLlmRequest { .. } => "session/title-llm-request",
            SubagentDescriptor { .. } => "subagent/descriptor",
            SubagentCatalog { .. } => "subagent/catalog",
            TeamMember { .. } => "team/member",
            TeamMessageQueued { .. } => "team/message/queued",
            TeamMessageDelivered { .. } => "team/message/delivered",
            TeamTask { .. } => "team/task",
            ToolWorkflowRunStart { .. } => "tool-workflow/run-start",
            ToolWorkflowRunEnd { .. } => "tool-workflow/run-end",
            ToolWorkflowAgentStart { .. } => "tool-workflow/agent-start",
            ToolWorkflowAgentEnd { .. } => "tool-workflow/agent-end",
            ToolPtcDispatchStart { .. } => "tool/ptc-dispatch-start",
            ToolPtcDispatch { .. } => "tool/ptc-dispatch",
            WebDeepSeekSearchLlmRequest { .. } => "web/deepseek-search-llm-request",
            ModelSelection { .. } => "model/selection",
            SessionLogDeepseekDeliveryAccepted { .. } => "session-log-deepseek/delivery-accepted",
            SubagentModelSelectionPolicy { .. } => "subagent/model-selection-policy",
            Unknown { .. } => {
                // 未知类型：返回稳定占位（调用方如需原始串可用 events.payload）。
                "unknown"
            }
        }
    }

    /// 事件时间（dsh 侧 epoch ms）。
    ///
    /// # 返回
    /// 信封上的 `time`（毫秒；所有变体都必有，故无需 Option）。
    ///
    /// 为什么需要：时长统计/时间线排序都靠它；由访问器统一取出，
    /// 消费方不必为 59 个变体各写一次 match。
    pub fn time(&self) -> u64 {
        use SessionEvent::*;
        match self {
            TurnStart { time, .. }
            | TurnEnd { time, .. }
            | StepStart { time, .. }
            | StepEnd { time, .. }
            | UserMessage { time, .. }
            | SystemMessage { time, .. }
            | AssistantMessage { time, .. }
            | AssistantAttempt { time, .. }
            | ToolCall { time, .. }
            | ToolResult { time, .. }
            | TodoWrite { time, .. }
            | RequestHeader { time, .. }
            | RequestContext { time, .. }
            | SessionEndSeed { time, .. }
            | DeliverablesPresented { time, .. }
            | DeveloperMessage { time, .. }
            | ImageOffload { time, .. }
            | WorkspaceChanges { time, .. }
            | AgentPresetSelected { time, .. }
            | AgentInboxSpliced { time, .. }
            | ApprovalAsked { time, .. }
            | ApprovalDecided { time, .. }
            | ApprovalPolicy { time, .. }
            | PermissionPreset { time, .. }
            | CommandRun { time, .. }
            | CommandDone { time, .. }
            | CompactionStart { time, .. }
            | CompactionEnd { time, .. }
            | CompactionPrune { time, .. }
            | CompactionSummary { time, .. }
            | FeedbackRecord { time, .. }
            | FeedbackMessagePut { time, .. }
            | FeedbackMessageDelete { time, .. }
            | GoalChange { time, .. }
            | HookInvoked { time, .. }
            | HookResult { time, .. }
            | LlmRetry { time, .. }
            | LlmRetryStarted { time, .. }
            | PlanMode { time, .. }
            | SandboxMode { time, .. }
            | ScheduleChange { time, .. }
            | SessionTitle { time, .. }
            | SessionTitleLlmRequest { time, .. }
            | SubagentDescriptor { time, .. }
            | SubagentCatalog { time, .. }
            | TeamMember { time, .. }
            | TeamMessageQueued { time, .. }
            | TeamMessageDelivered { time, .. }
            | TeamTask { time, .. }
            | ToolWorkflowRunStart { time, .. }
            | ToolWorkflowRunEnd { time, .. }
            | ToolWorkflowAgentStart { time, .. }
            | ToolWorkflowAgentEnd { time, .. }
            | ToolPtcDispatchStart { time, .. }
            | ToolPtcDispatch { time, .. }
            | WebDeepSeekSearchLlmRequest { time, .. }
            | ModelSelection { time, .. }
            | SessionLogDeepseekDeliveryAccepted { time, .. }
            | SubagentModelSelectionPolicy { time, .. }
            | Unknown { time, .. } => *time,
        }
    }

    /// 事件在会话内的序号（排序/增量书签用）。
    ///
    /// # 返回
    /// 信封上的 `seq`（会话内单调递增；同一会话的排序基准）。
    ///
    /// 为什么需要：WireLog 回放、增量读取（throughSeq 书签）与事件去重都以 seq 为键，
    /// 集中在此以免各处自行从 JSON 里取。
    pub fn seq(&self) -> u64 {
        use SessionEvent::*;
        match self {
            TurnStart { seq, .. }
            | TurnEnd { seq, .. }
            | StepStart { seq, .. }
            | StepEnd { seq, .. }
            | UserMessage { seq, .. }
            | SystemMessage { seq, .. }
            | AssistantMessage { seq, .. }
            | AssistantAttempt { seq, .. }
            | ToolCall { seq, .. }
            | ToolResult { seq, .. }
            | TodoWrite { seq, .. }
            | RequestHeader { seq, .. }
            | RequestContext { seq, .. }
            | SessionEndSeed { seq, .. }
            | DeliverablesPresented { seq, .. }
            | DeveloperMessage { seq, .. }
            | ImageOffload { seq, .. }
            | WorkspaceChanges { seq, .. }
            | AgentPresetSelected { seq, .. }
            | AgentInboxSpliced { seq, .. }
            | ApprovalAsked { seq, .. }
            | ApprovalDecided { seq, .. }
            | ApprovalPolicy { seq, .. }
            | PermissionPreset { seq, .. }
            | CommandRun { seq, .. }
            | CommandDone { seq, .. }
            | CompactionStart { seq, .. }
            | CompactionEnd { seq, .. }
            | CompactionPrune { seq, .. }
            | CompactionSummary { seq, .. }
            | FeedbackRecord { seq, .. }
            | FeedbackMessagePut { seq, .. }
            | FeedbackMessageDelete { seq, .. }
            | GoalChange { seq, .. }
            | HookInvoked { seq, .. }
            | HookResult { seq, .. }
            | LlmRetry { seq, .. }
            | LlmRetryStarted { seq, .. }
            | PlanMode { seq, .. }
            | SandboxMode { seq, .. }
            | ScheduleChange { seq, .. }
            | SessionTitle { seq, .. }
            | SessionTitleLlmRequest { seq, .. }
            | SubagentDescriptor { seq, .. }
            | SubagentCatalog { seq, .. }
            | TeamMember { seq, .. }
            | TeamMessageQueued { seq, .. }
            | TeamMessageDelivered { seq, .. }
            | TeamTask { seq, .. }
            | ToolWorkflowRunStart { seq, .. }
            | ToolWorkflowRunEnd { seq, .. }
            | ToolWorkflowAgentStart { seq, .. }
            | ToolWorkflowAgentEnd { seq, .. }
            | ToolPtcDispatchStart { seq, .. }
            | ToolPtcDispatch { seq, .. }
            | WebDeepSeekSearchLlmRequest { seq, .. }
            | ModelSelection { seq, .. }
            | SessionLogDeepseekDeliveryAccepted { seq, .. }
            | SubagentModelSelectionPolicy { seq, .. }
            | Unknown { seq, .. } => *seq,
        }
    }

    /// 提取事件 data 里的 (turn, step)。
    /// 接收：任意 SessionEvent。
    /// 处理：按变体取出 data.turn / data.step（无 turn/step 的事件返回 None,None）。
    /// 生成：(Option<turn>, Option<step>)，消费方落库 events 表的 turn/step 列用。
    /// 为什么需要：turn/step 在 wire 上嵌在各族 data 里（还有 `llm/retry` 那种藏在
    /// 联合变体内部的），落库时却要拍平成两列；此方法把「哪个变体有哪个字段」的知识
    /// 收敛在一处，并借穷尽匹配强迫官方新增事件时补登记。
    pub fn turn_step(&self) -> (Option<u64>, Option<u64>) {
        use SessionEvent::*;
        match self {
            TurnStart { data, .. } => (Some(data.turn), None),
            TurnEnd { data, .. } => (Some(data.turn), None),
            StepStart { data, .. } => (Some(data.turn), Some(data.step)),
            StepEnd { data, .. } => (Some(data.turn), Some(data.step)),
            SystemMessage { data, .. } => (Some(data.turn), Some(data.step)),
            AssistantMessage { data, .. } => (Some(data.turn), Some(data.step)),
            AssistantAttempt { data, .. } => (Some(data.turn), Some(data.step)),
            ToolCall { data, .. } => (Some(data.turn), Some(data.step)),
            ToolResult { data, .. } => (Some(data.turn), Some(data.step)),
            HookInvoked { data, .. } => (Some(data.turn), None),
            HookResult { data, .. } => (Some(data.turn), None),
            LlmRetry { data, .. } => match data {
                retry::LlmRetryData::Normal { turn, step, .. } => (Some(*turn), Some(*step)),
                retry::LlmRetryData::Always { turn, step, .. } => (Some(*turn), Some(*step)),
            },
            LlmRetryStarted { data, .. } => (Some(data.turn), Some(data.step)),
            DeliverablesPresented { data, .. } => (Some(data.turn), None),
            DeveloperMessage { data, .. } => (Some(data.turn), Some(data.step)),
            WorkspaceChanges { data, .. } => (Some(data.turn), None),
            CompactionStart { data, .. } => (data.turn, None),
            CompactionEnd { data, .. } => (data.turn, None),
            _ => (None, None),
        }
    }

    /// 若本条是「**已知类型、data 没解析出来**」的降级事件 → 返回 (原始 type, seq)。
    ///
    /// # 返回
    /// - `Some((type, seq))` = 协议漂移信号（官方改了字段形状，或 dshr 的变体与官方不一致）；
    /// - `None` = 解析成功，**或**类型本身就未知（插件自注册事件——merge-extensible 的预期内行为）。
    ///
    /// 为什么需要：`fallback::known()` 的容错（解析失败 → 降级 `Unknown`）是刻意的，代价是
    /// **静默**——2026-09-29 就是靠事后人工扫日志才发现 `system/message` 全量降级
    ///（漏了 `system-prompt` 这个 kind）。engine 用本方法把漂移写成一条 app 轨迹
    ///（`event.degraded`），此后不必等人去翻 wire log。
    /// 边界：只报「已知类型解析失败」，不报「未知类型」——否则正常演进会被淹没在告警里。
    pub fn degraded_event(&self) -> Option<(&str, u64)> {
        match self {
            SessionEvent::Unknown {
                event_type,
                seq,
                degraded: true,
                ..
            } => Some((event_type.as_str(), *seq)),
            _ => None,
        }
    }
}
