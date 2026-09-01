//! `SessionEvent`：会话日志事件（`type` 字段打标的判别联合）。
//! 信封 + 判别枚举放本文件，事件 data 按事件族拆到子模块：
//! 每个子模块对应官方 `packages/core/session/src/types.ts` 的 `SessionEventMap` 一组事件
//! （核心 13 种在核心包；扩展事件由各插件包 `declare module` 注册，见各文件头注释）。
pub mod agent;
pub mod approval;
pub mod command;
pub mod compaction;
pub mod descriptor;
pub mod fallback;
pub mod goal;
pub mod hook;
pub mod message;
pub mod misc;
pub mod mode;
pub mod request;
pub mod retry;
pub mod schedule;
pub mod session;
pub mod team;
pub mod title;
pub mod tool;
pub mod turn;
pub mod web;
pub mod workflow;
mod meta;

use serde::Serialize;

/// 事件信封：`type` 判别 + 公共字段 `seq/time` + 各自 `data`。
/// 官方：packages/core/session/src/types.ts 的 SessionEvent（404 行起）
/// 用在 session.event 通知的 params.event。
/// 注意：wire 类型带斜杠（turn/start），不能用 kebab-case，逐个显式 rename；
/// 反序列化由 fallback.rs 手工实现（未知类型 → Unknown），`Deserialize` 从 derive 移除。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// 打开一轮 turn。
    #[serde(rename = "turn/start")]
    TurnStart {
        seq: u64,
        time: u64,
        data: turn::TurnStartData,
    },
    /// 关闭一轮 turn。
    #[serde(rename = "turn/end")]
    TurnEnd {
        seq: u64,
        time: u64,
        data: turn::TurnEndData,
    },
    /// 打开一步（一次模型调用 + 其工具执行）。
    #[serde(rename = "step/start")]
    StepStart {
        seq: u64,
        time: u64,
        data: turn::StepStartData,
    },
    /// 关闭一步。
    #[serde(rename = "step/end")]
    StepEnd {
        seq: u64,
        time: u64,
        data: turn::StepEndData,
    },
    /// 一条用户消息进入会话。
    #[serde(rename = "user/message")]
    UserMessage {
        seq: u64,
        time: u64,
        data: message::UserMessageData,
    },
    /// 助手流式输出的一块。
    #[serde(rename = "assistant/chunk")]
    AssistantChunk {
        seq: u64,
        time: u64,
        data: message::AssistantChunkData,
    },
    /// 组装好的助手消息。
    #[serde(rename = "assistant/message")]
    AssistantMessage {
        seq: u64,
        time: u64,
        data: message::AssistantMessageData,
    },
    /// 模型发起一次工具调用。
    #[serde(rename = "tool/call")]
    ToolCall {
        seq: u64,
        time: u64,
        data: tool::ToolCallData,
    },
    /// 工具执行结果。
    #[serde(rename = "tool/result")]
    ToolResult {
        seq: u64,
        time: u64,
        data: tool::ToolResultData,
    },
    /// todo 列表快照。
    #[serde(rename = "todo/write")]
    TodoWrite {
        seq: u64,
        time: u64,
        data: misc::TodoWriteData,
    },
    /// 下次请求的请求头快照。
    #[serde(rename = "request/header")]
    RequestHeader {
        seq: u64,
        time: u64,
        data: request::RequestHeaderData,
    },
    /// 模型路由元数据。
    #[serde(rename = "request/context")]
    RequestContext {
        seq: u64,
        time: u64,
        data: request::RequestContextData,
    },
    /// 构造种子结束。
    #[serde(rename = "session/end-seed")]
    SessionEndSeed {
        seq: u64,
        time: u64,
        data: session::SessionEndSeedData,
    },
    /// 选中的 agent preset id（最后写入者胜）。
    #[serde(rename = "agent-preset/selected")]
    AgentPresetSelected {
        seq: u64,
        time: u64,
        data: agent::AgentPresetSelectedData,
    },
    /// 消息 inbox 的 splice 增量。
    #[serde(rename = "agent/inbox/spliced")]
    AgentInboxSpliced {
        seq: u64,
        time: u64,
        data: agent::InboxSplicedData,
    },
    /// 一次审批询问。
    #[serde(rename = "approval/asked")]
    ApprovalAsked {
        seq: u64,
        time: u64,
        data: approval::ApprovalAskedData,
    },
    /// 审批结果。
    #[serde(rename = "approval/decided")]
    ApprovalDecided {
        seq: u64,
        time: u64,
        data: approval::ApprovalDecidedData,
    },
    /// 会话审批策略开关（最后写入者胜）。
    #[serde(rename = "approval/policy")]
    ApprovalPolicy {
        seq: u64,
        time: u64,
        data: approval::ApprovalPolicyData,
    },
    /// 用户选中的权限预设（最后写入者胜）。
    #[serde(rename = "permission/preset")]
    PermissionPreset {
        seq: u64,
        time: u64,
        data: approval::PermissionPresetData,
    },
    /// 一次命令开始。
    #[serde(rename = "command/run")]
    CommandRun {
        seq: u64,
        time: u64,
        data: command::CommandRunData,
    },
    /// 命令结算。
    #[serde(rename = "command/done")]
    CommandDone {
        seq: u64,
        time: u64,
        data: command::CommandDoneData,
    },
    /// 压缩事务开始。
    #[serde(rename = "compaction/start")]
    CompactionStart {
        seq: u64,
        time: u64,
        data: compaction::CompactionStartData,
    },
    /// 压缩事务结束。
    #[serde(rename = "compaction/end")]
    CompactionEnd {
        seq: u64,
        time: u64,
        data: compaction::CompactionEndData,
    },
    /// 压缩剪枝的计量事件。
    #[serde(rename = "compaction/prune")]
    CompactionPrune {
        seq: u64,
        time: u64,
        data: compaction::CompactionPruneData,
    },
    /// 压缩总结。
    #[serde(rename = "compaction/summary")]
    CompactionSummary {
        seq: u64,
        time: u64,
        data: compaction::CompactionSummaryData,
    },
    /// 用户反馈文本。
    #[serde(rename = "feedback/record")]
    FeedbackRecord {
        seq: u64,
        time: u64,
        data: misc::FeedbackRecordData,
    },
    /// 目标变更（快照 + 墓碑）。
    #[serde(rename = "goal/change")]
    GoalChange {
        seq: u64,
        time: u64,
        data: goal::GoalChangeData,
    },
    /// hook 调用开始。
    #[serde(rename = "hook/invoked")]
    HookInvoked {
        seq: u64,
        time: u64,
        data: hook::HookInvokedData,
    },
    /// hook 调用结算。
    #[serde(rename = "hook/result")]
    HookResult {
        seq: u64,
        time: u64,
        data: hook::HookResultData,
    },
    /// 一次已排程的 LLM 重试。
    #[serde(rename = "llm/retry")]
    LlmRetry {
        seq: u64,
        time: u64,
        data: retry::LlmRetryData,
    },
    /// 重试真正开始。
    #[serde(rename = "llm/retry-started")]
    LlmRetryStarted {
        seq: u64,
        time: u64,
        data: retry::LlmRetryStartedData,
    },
    /// plan 模式开关。
    #[serde(rename = "plan/mode")]
    PlanMode {
        seq: u64,
        time: u64,
        data: mode::PlanModeData,
    },
    /// 沙箱模式。
    #[serde(rename = "sandbox/mode")]
    SandboxMode {
        seq: u64,
        time: u64,
        data: mode::SandboxModeData,
    },
    /// 调度变更。
    #[serde(rename = "schedule/change")]
    ScheduleChange {
        seq: u64,
        time: u64,
        data: schedule::ScheduleChangeData,
    },
    /// 会话标题快照。
    #[serde(rename = "session/title")]
    SessionTitle {
        seq: u64,
        time: u64,
        data: title::SessionTitleData,
    },
    /// 标题生成的 LLM 请求快照。
    #[serde(rename = "session/title-llm-request")]
    SessionTitleLlmRequest {
        seq: u64,
        time: u64,
        data: title::TitleLlmRequestData,
    },
    /// 子代理组成声明。
    #[serde(rename = "subagent/descriptor")]
    SubagentDescriptor {
        seq: u64,
        time: u64,
        data: descriptor::SubagentDescriptorData,
    },
    /// 团队成员快照。
    #[serde(rename = "team/member")]
    TeamMember {
        seq: u64,
        time: u64,
        data: team::TeamMemberData,
    },
    /// 团队消息入队。
    #[serde(rename = "team/message/queued")]
    TeamMessageQueued {
        seq: u64,
        time: u64,
        data: team::TeamMessageQueuedData,
    },
    /// 团队消息送达确认。
    #[serde(rename = "team/message/delivered")]
    TeamMessageDelivered {
        seq: u64,
        time: u64,
        data: team::TeamMessageDeliveredData,
    },
    /// 团队任务快照。
    #[serde(rename = "team/task")]
    TeamTask {
        seq: u64,
        time: u64,
        data: team::TeamTaskData,
    },
    /// 工具工作流 run 开始。
    #[serde(rename = "tool-workflow/run-start")]
    ToolWorkflowRunStart {
        seq: u64,
        time: u64,
        data: workflow::ToolWorkflowRunStartData,
    },
    /// 工具工作流 run 结束。
    #[serde(rename = "tool-workflow/run-end")]
    ToolWorkflowRunEnd {
        seq: u64,
        time: u64,
        data: workflow::ToolWorkflowRunEndData,
    },
    /// 工具工作流 agent 调用开始。
    #[serde(rename = "tool-workflow/agent-start")]
    ToolWorkflowAgentStart {
        seq: u64,
        time: u64,
        data: workflow::ToolWorkflowAgentStartData,
    },
    /// 工具工作流 agent 调用结算。
    #[serde(rename = "tool-workflow/agent-end")]
    ToolWorkflowAgentEnd {
        seq: u64,
        time: u64,
        data: workflow::ToolWorkflowAgentEndData,
    },
    /// code-mode 子调用开始执行。
    #[serde(rename = "tool/code-dispatch-start")]
    ToolCodeDispatchStart {
        seq: u64,
        time: u64,
        data: tool::CodeDispatchStartData,
    },
    /// code-mode 子调用结算。
    #[serde(rename = "tool/code-dispatch")]
    ToolCodeDispatch {
        seq: u64,
        time: u64,
        data: tool::CodeDispatchData,
    },
    /// 辅助 web 搜索请求快照。
    #[serde(rename = "web/deepseek-search-llm-request")]
    WebDeepSeekSearchLlmRequest {
        seq: u64,
        time: u64,
        data: web::DeepSeekSearchLlmRequestData,
    },
    /// 未知事件类型（merge-extensible 的逃生门，lossless 保留全字段）。
    /// 官方语义：未知 + ignorable=true 可安全跳过；未知且无标记应拒绝重建。
    /// dshr 作为观察者先宽松保留，供打印/转发。
    /// 序列化暂用 derive（会输出 type="unknown"），需要转发时 v2 手写 Serialize。
    Unknown {
        // 原始 type 字符串。
        event_type: String,
        seq: u64,
        time: u64,
        // 原始 data JSON 原样保留。
        data: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        ignorable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_event_seqs: Option<Vec<u64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        surface_op: Option<serde_json::Value>,
    },
}

