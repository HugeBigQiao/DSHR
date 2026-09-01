//! `SessionEvent` 的元数据访问器（与枚举本体分开，控制父文件行数 ≤350）。
//!
//! 枚举本体在父模块 `session_event.rs`；本文件只有 `impl SessionEvent` 的只读访问器：
//! `event_type` / `time` / `seq` / `turn_step`。事件 data 的官方引用见各变体注释。
use super::retry;
use super::SessionEvent;

impl SessionEvent {
    /// wire 类型字符串（events 表的 type 列）。
    pub fn event_type(&self) -> &'static str {
        use SessionEvent::*;
        match self {
            TurnStart { .. } => "turn/start",
            TurnEnd { .. } => "turn/end",
            StepStart { .. } => "step/start",
            StepEnd { .. } => "step/end",
            UserMessage { .. } => "user/message",
            AssistantChunk { .. } => "assistant/chunk",
            AssistantMessage { .. } => "assistant/message",
            ToolCall { .. } => "tool/call",
            ToolResult { .. } => "tool/result",
            TodoWrite { .. } => "todo/write",
            RequestHeader { .. } => "request/header",
            RequestContext { .. } => "request/context",
            SessionEndSeed { .. } => "session/end-seed",
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
            TeamMember { .. } => "team/member",
            TeamMessageQueued { .. } => "team/message/queued",
            TeamMessageDelivered { .. } => "team/message/delivered",
            TeamTask { .. } => "team/task",
            ToolWorkflowRunStart { .. } => "tool-workflow/run-start",
            ToolWorkflowRunEnd { .. } => "tool-workflow/run-end",
            ToolWorkflowAgentStart { .. } => "tool-workflow/agent-start",
            ToolWorkflowAgentEnd { .. } => "tool-workflow/agent-end",
            ToolCodeDispatchStart { .. } => "tool/code-dispatch-start",
            ToolCodeDispatch { .. } => "tool/code-dispatch",
            WebDeepSeekSearchLlmRequest { .. } => "web/deepseek-search-llm-request",
            Unknown { .. } => {
                // 未知类型：返回稳定占位（调用方如需原始串可用 events.payload）。
                "unknown"
            }
        }
    }

    /// 事件时间（dsh 侧 epoch ms）。
    pub fn time(&self) -> u64 {
        use SessionEvent::*;
        match self {
            TurnStart { time, .. }
            | TurnEnd { time, .. }
            | StepStart { time, .. }
            | StepEnd { time, .. }
            | UserMessage { time, .. }
            | AssistantChunk { time, .. }
            | AssistantMessage { time, .. }
            | ToolCall { time, .. }
            | ToolResult { time, .. }
            | TodoWrite { time, .. }
            | RequestHeader { time, .. }
            | RequestContext { time, .. }
            | SessionEndSeed { time, .. }
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
            | TeamMember { time, .. }
            | TeamMessageQueued { time, .. }
            | TeamMessageDelivered { time, .. }
            | TeamTask { time, .. }
            | ToolWorkflowRunStart { time, .. }
            | ToolWorkflowRunEnd { time, .. }
            | ToolWorkflowAgentStart { time, .. }
            | ToolWorkflowAgentEnd { time, .. }
            | ToolCodeDispatchStart { time, .. }
            | ToolCodeDispatch { time, .. }
            | WebDeepSeekSearchLlmRequest { time, .. }
            | Unknown { time, .. } => *time,
        }
    }

    /// 事件在会话内的序号（排序/增量书签用）。
    pub fn seq(&self) -> u64 {
        use SessionEvent::*;
        match self {
            TurnStart { seq, .. }
            | TurnEnd { seq, .. }
            | StepStart { seq, .. }
            | StepEnd { seq, .. }
            | UserMessage { seq, .. }
            | AssistantChunk { seq, .. }
            | AssistantMessage { seq, .. }
            | ToolCall { seq, .. }
            | ToolResult { seq, .. }
            | TodoWrite { seq, .. }
            | RequestHeader { seq, .. }
            | RequestContext { seq, .. }
            | SessionEndSeed { seq, .. }
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
            | TeamMember { seq, .. }
            | TeamMessageQueued { seq, .. }
            | TeamMessageDelivered { seq, .. }
            | TeamTask { seq, .. }
            | ToolWorkflowRunStart { seq, .. }
            | ToolWorkflowRunEnd { seq, .. }
            | ToolWorkflowAgentStart { seq, .. }
            | ToolWorkflowAgentEnd { seq, .. }
            | ToolCodeDispatchStart { seq, .. }
            | ToolCodeDispatch { seq, .. }
            | WebDeepSeekSearchLlmRequest { seq, .. }
            | Unknown { seq, .. } => *seq,
        }
    }

    /// 提取事件 data 里的 (turn, step)。
    /// 接收：任意 SessionEvent。
    /// 处理：按变体取出 data.turn / data.step（无 turn/step 的事件返回 None,None）。
    /// 生成：(Option<turn>, Option<step>)，消费方落库 events 表的 turn/step 列用。
    pub fn turn_step(&self) -> (Option<u64>, Option<u64>) {
        use SessionEvent::*;
        match self {
            TurnStart { data, .. } => (Some(data.turn), None),
            TurnEnd { data, .. } => (Some(data.turn), None),
            StepStart { data, .. } => (Some(data.turn), Some(data.step)),
            StepEnd { data, .. } => (Some(data.turn), Some(data.step)),
            AssistantChunk { data, .. } => (Some(data.turn), Some(data.step)),
            AssistantMessage { data, .. } => (Some(data.turn), Some(data.step)),
            ToolCall { data, .. } => (Some(data.turn), Some(data.step)),
            ToolResult { data, .. } => (Some(data.turn), Some(data.step)),
            HookInvoked { data, .. } => (Some(data.turn), None),
            HookResult { data, .. } => (Some(data.turn), None),
            LlmRetry { data, .. } => match data {
                retry::LlmRetryData::Normal { turn, step, .. } => (Some(*turn), Some(*step)),
                retry::LlmRetryData::Always { turn, step, .. } => (Some(*turn), Some(*step)),
            },
            LlmRetryStarted { data, .. } => (Some(data.turn), Some(data.step)),
            CompactionStart { data, .. } => (data.turn, None),
            CompactionEnd { data, .. } => (data.turn, None),
            _ => (None, None),
        }
    }
}
