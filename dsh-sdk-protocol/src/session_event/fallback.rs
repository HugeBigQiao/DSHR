//! `SessionEvent` 的 fallback：手写 `Deserialize`。
//!
//! 官方协议是 merge-extensible（插件可注册新事件、版本会继续涨），Rust 枚举
//! 是封闭集合，所以反序列化走"通用信封 → 按 type 分发"：已知 51 种 →
//! 类型化变体；未知（如插件自注册事件）→ `Unknown`（全字段 lossless 保留）。
//! v3 起：已知类型 data 解析失败也降级 `Unknown`（lossless），不整体报错——
//! 官方发版漂移（字段改名/枚举新增）时类型化视图失效但不丢事件、不中断解析。
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

/// 已知类型 data 的"尽力解析"：成功 → 类型化变体；失败 → Unknown（lossless 保留原始 data）。
/// 官方对未知事件要求宽容（merge-extensible）；这里是"已知但字段已漂移"的同款宽容。
fn known<T>(
    raw: &RawEvent,
    data: &serde_json::Value,
    seq: u64,
    time: u64,
    make: impl FnOnce(T) -> SessionEvent,
) -> SessionEvent
where
    T: de::DeserializeOwned,
{
    match serde_json::from_value::<T>(data.clone()) {
        Ok(data) => make(data),
        Err(_) => SessionEvent::Unknown {
            event_type: raw.event_type.clone(),
            seq,
            time,
            data: data.clone(),
            ignorable: raw.ignorable,
            source_event_seqs: raw.source_event_seqs.clone(),
            surface_op: raw.surface_op.clone(),
        },
    }
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // 手写反序列化的核心分发：
        // 接收：任意事件 JSON。
        // 处理：先解通用信封（type/seq/time/data + 可选扩展字段），再按 type 字符串分发——
        //       已知 51 种 → known() 尽力解析（失败降级 Unknown）；
        //       未知 → 全字段原样保留进 Unknown（lossless，插件/新版扩展事件靠它兜住）。
        // 生成：类型化的 SessionEvent。
        let raw = RawEvent::deserialize(d)?;
        let seq = raw.seq;
        let time = raw.time;
        Ok(match raw.event_type.as_str() {
            "turn/start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TurnStart { seq, time, data }),
            "turn/end" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TurnEnd { seq, time, data }),
            "step/start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::StepStart { seq, time, data }),
            "step/end" => known(&raw, &raw.data, seq, time, |data| SessionEvent::StepEnd { seq, time, data }),
            "user/message" => known(&raw, &raw.data, seq, time, |data| SessionEvent::UserMessage { seq, time, data }),
            "assistant/chunk" => known(&raw, &raw.data, seq, time, |data| SessionEvent::AssistantChunk { seq, time, data }),
            "assistant/message" => known(&raw, &raw.data, seq, time, |data| SessionEvent::AssistantMessage { seq, time, data }),
            "tool/call" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolCall { seq, time, data }),
            "tool/result" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolResult { seq, time, data }),
            "todo/write" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TodoWrite { seq, time, data }),
            "request/header" => known(&raw, &raw.data, seq, time, |data| SessionEvent::RequestHeader { seq, time, data }),
            "request/context" => known(&raw, &raw.data, seq, time, |data| SessionEvent::RequestContext { seq, time, data }),
            "session/end-seed" => known(&raw, &raw.data, seq, time, |data| SessionEvent::SessionEndSeed { seq, time, data }),
            "agent-preset/selected" => known(&raw, &raw.data, seq, time, |data| SessionEvent::AgentPresetSelected { seq, time, data }),
            "agent/inbox/spliced" => known(&raw, &raw.data, seq, time, |data| SessionEvent::AgentInboxSpliced { seq, time, data }),
            "approval/asked" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ApprovalAsked { seq, time, data }),
            "approval/decided" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ApprovalDecided { seq, time, data }),
            "approval/policy" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ApprovalPolicy { seq, time, data }),
            "permission/preset" => known(&raw, &raw.data, seq, time, |data| SessionEvent::PermissionPreset { seq, time, data }),
            "command/run" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CommandRun { seq, time, data }),
            "command/done" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CommandDone { seq, time, data }),
            "compaction/start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CompactionStart { seq, time, data }),
            "compaction/end" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CompactionEnd { seq, time, data }),
            "compaction/prune" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CompactionPrune { seq, time, data }),
            "compaction/summary" => known(&raw, &raw.data, seq, time, |data| SessionEvent::CompactionSummary { seq, time, data }),
            "feedback/record" => known(&raw, &raw.data, seq, time, |data| SessionEvent::FeedbackRecord { seq, time, data }),
            "goal/change" => known(&raw, &raw.data, seq, time, |data| SessionEvent::GoalChange { seq, time, data }),
            "hook/invoked" => known(&raw, &raw.data, seq, time, |data| SessionEvent::HookInvoked { seq, time, data }),
            "hook/result" => known(&raw, &raw.data, seq, time, |data| SessionEvent::HookResult { seq, time, data }),
            "llm/retry" => known(&raw, &raw.data, seq, time, |data| SessionEvent::LlmRetry { seq, time, data }),
            "llm/retry-started" => known(&raw, &raw.data, seq, time, |data| SessionEvent::LlmRetryStarted { seq, time, data }),
            "plan/mode" => known(&raw, &raw.data, seq, time, |data| SessionEvent::PlanMode { seq, time, data }),
            "sandbox/mode" => known(&raw, &raw.data, seq, time, |data| SessionEvent::SandboxMode { seq, time, data }),
            "schedule/change" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ScheduleChange { seq, time, data }),
            "session/title" => known(&raw, &raw.data, seq, time, |data| SessionEvent::SessionTitle { seq, time, data }),
            "session/title-llm-request" => known(&raw, &raw.data, seq, time, |data| SessionEvent::SessionTitleLlmRequest { seq, time, data }),
            "subagent/descriptor" => known(&raw, &raw.data, seq, time, |data| SessionEvent::SubagentDescriptor { seq, time, data }),
            "team/member" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TeamMember { seq, time, data }),
            "team/message/queued" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TeamMessageQueued { seq, time, data }),
            "team/message/delivered" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TeamMessageDelivered { seq, time, data }),
            "team/task" => known(&raw, &raw.data, seq, time, |data| SessionEvent::TeamTask { seq, time, data }),
            "tool-workflow/run-start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolWorkflowRunStart { seq, time, data }),
            "tool-workflow/run-end" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolWorkflowRunEnd { seq, time, data }),
            "tool-workflow/agent-start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolWorkflowAgentStart { seq, time, data }),
            "tool-workflow/agent-end" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolWorkflowAgentEnd { seq, time, data }),
            "tool/code-dispatch-start" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolCodeDispatchStart { seq, time, data }),
            "tool/code-dispatch" => known(&raw, &raw.data, seq, time, |data| SessionEvent::ToolCodeDispatch { seq, time, data }),
            "web/deepseek-search-llm-request" => known(&raw, &raw.data, seq, time, |data| SessionEvent::WebDeepSeekSearchLlmRequest { seq, time, data }),
            other => SessionEvent::Unknown {
                event_type: other.to_string(),
                seq,
                time,
                data: raw.data,
                ignorable: raw.ignorable,
                source_event_seqs: raw.source_event_seqs,
                surface_op: raw.surface_op,
            },
        })
    }
}
