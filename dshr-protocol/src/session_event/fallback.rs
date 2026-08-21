//! `SessionEvent` 的 fallback：手写 `Deserialize`。
//!
//! 官方协议是 merge-extensible（插件可注册新事件、版本会继续涨），Rust 枚举
//! 是封闭集合，所以反序列化走"通用信封 → 按 type 分发"：已知 48 种 →
//! 类型化变体；未知（如插件自注册事件）→ `Unknown`（全字段 lossless 保留）。
use serde::Deserialize;
use serde::de::{self, Deserializer};

use super::SessionEvent;

/// 通用信封：不判别，先接住一切（type/seq/time/data + 可选扩展字段）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEvent {
    #[serde(rename = "type")]
    event_type: String,
    seq: u64,
    time: u64,
    // 先不解析，当原始 JSON 存着；分发到已知类型时再解。
    data: serde_json::Value,
    #[serde(default)]
    ignorable: Option<bool>,
    #[serde(default)]
    source_event_seqs: Option<Vec<u64>>,
    #[serde(default)]
    surface_op: Option<serde_json::Value>,
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // 手写反序列化的核心分发：
        // 接收：任意事件 JSON。
        // 处理：先解通用信封（type/seq/time/data + 可选扩展字段），再按 type 字符串分发——
        //       已知 48 种 → 把 data 解成对应事件结构体；
        //       未知 → 全字段原样保留进 Unknown（lossless，插件/新版扩展事件靠它兜住）。
        // 生成：类型化的 SessionEvent。
        let raw = RawEvent::deserialize(d)?;
        let seq = raw.seq;
        let time = raw.time;
        let data = raw.data;
        Ok(match raw.event_type.as_str() {
            "turn/start" => SessionEvent::TurnStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "turn/end" => SessionEvent::TurnEnd {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "step/start" => SessionEvent::StepStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "step/end" => SessionEvent::StepEnd {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "user/message" => SessionEvent::UserMessage {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "assistant/chunk" => SessionEvent::AssistantChunk {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "assistant/message" => SessionEvent::AssistantMessage {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool/call" => SessionEvent::ToolCall {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool/result" => SessionEvent::ToolResult {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "todo/write" => SessionEvent::TodoWrite {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "request/header" => SessionEvent::RequestHeader {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "request/context" => SessionEvent::RequestContext {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "session/end-seed" => SessionEvent::SessionEndSeed {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "agent-preset/selected" => SessionEvent::AgentPresetSelected {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "agent/inbox/spliced" => SessionEvent::AgentInboxSpliced {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "approval/asked" => SessionEvent::ApprovalAsked {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "approval/decided" => SessionEvent::ApprovalDecided {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "approval/policy" => SessionEvent::ApprovalPolicy {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "permission/preset" => SessionEvent::PermissionPreset {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "command/run" => SessionEvent::CommandRun {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "command/done" => SessionEvent::CommandDone {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "compaction/start" => SessionEvent::CompactionStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "compaction/end" => SessionEvent::CompactionEnd {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "compaction/prune" => SessionEvent::CompactionPrune {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "compaction/summary" => SessionEvent::CompactionSummary {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "feedback/record" => SessionEvent::FeedbackRecord {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "goal/change" => SessionEvent::GoalChange {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "hook/invoked" => SessionEvent::HookInvoked {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "hook/result" => SessionEvent::HookResult {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "llm/retry" => SessionEvent::LlmRetry {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "llm/retry-started" => SessionEvent::LlmRetryStarted {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "plan/mode" => SessionEvent::PlanMode {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "sandbox/mode" => SessionEvent::SandboxMode {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "schedule/change" => SessionEvent::ScheduleChange {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "session/title" => SessionEvent::SessionTitle {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "session/title-llm-request" => SessionEvent::SessionTitleLlmRequest {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "subagent/descriptor" => SessionEvent::SubagentDescriptor {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "team/member" => SessionEvent::TeamMember {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "team/message/queued" => SessionEvent::TeamMessageQueued {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "team/message/delivered" => SessionEvent::TeamMessageDelivered {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "team/task" => SessionEvent::TeamTask {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool-workflow/run-start" => SessionEvent::ToolWorkflowRunStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool-workflow/run-end" => SessionEvent::ToolWorkflowRunEnd {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool-workflow/agent-start" => SessionEvent::ToolWorkflowAgentStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool-workflow/agent-end" => SessionEvent::ToolWorkflowAgentEnd {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool/code-dispatch-start" => SessionEvent::ToolCodeDispatchStart {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "tool/code-dispatch" => SessionEvent::ToolCodeDispatch {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            "web/deepseek-search-llm-request" => SessionEvent::WebDeepSeekSearchLlmRequest {
                seq,
                time,
                data: serde_json::from_value(data).map_err(de::Error::custom)?,
            },
            other => SessionEvent::Unknown {
                event_type: other.to_string(),
                seq,
                time,
                data,
                ignorable: raw.ignorable,
                source_event_seqs: raw.source_event_seqs,
                surface_op: raw.surface_op,
            },
        })
    }
}
