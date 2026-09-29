//! 工具工作流扩展事件族：`tool-workflow/run-start`、`tool-workflow/run-end`、
//! `tool-workflow/agent-start`、`tool-workflow/agent-end`。
//!
//! 主要用途：工作流 run 的开合与 run 内 `agent()` 子调用开合四组事件的 data 类型。
//! 为什么需要：run 与 agent 是两层嵌套的配对结构（runId 分组、seq 配对），
//! 且 `agent-start.childId` 是「工作流内 spawn 出的子会话」与主会话树之间的桥梁——
//! 这套嵌套配对只有整体看才成立，故单列成族。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（工作流进度视图）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/workflow/tool-workflow/src/types.ts 的 `tool-workflow/*` 四组
//!（outcome/reason 在 packages/workflow/workflow/src/types.ts）。
use serde::{Deserialize, Serialize};

/// `tool-workflow/run-start` 的 data：打开一条持久化 run 记录。
/// 官方：packages/workflow/tool-workflow/src/types.ts 的 SessionEventMap['tool-workflow/run-start']
/// 用在工作流 run 开始事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolWorkflowRunStartData {
    /// 引擎铸造的 UUID。
    pub run_id: String,
    /// 来自 run.meta.name（workflow 脚本 meta 块）。
    pub name: String,
}

/// `tool-workflow/run-end` 的 data：关闭 run 记录。
/// 官方：packages/workflow/tool-workflow/src/types.ts 的 SessionEventMap['tool-workflow/run-end']
/// 用在工作流 run 结束事件（活跃资源安静后发出）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolWorkflowRunEndData {
    pub run_id: String,
    pub stop_reason: WorkflowStopReason,
}

/// 工作流停止原因（官方 WorkflowStopReason，引擎拥有的封闭联合）。
/// 用在 ToolWorkflowRunEndData.stop_reason。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowStopReason {
    Completed,
    Cancelled,
    Error,
}

/// `tool-workflow/agent-start` 的 data：子 agent 调用开始（子 Session 发布后记录）。
/// 官方：packages/workflow/tool-workflow/src/types.ts 的 SessionEventMap['tool-workflow/agent-start']
/// 用在工作流内 agent() 调用开始事件（seq 与该 run 的 agent-end 配对）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolWorkflowAgentStartData {
    pub run_id: String,
    /// 该 run 内 agent() 调用的 1-based 序号。
    pub seq: u32,
    /// label 选项或 prompt 片段。
    pub label: String,
    /// 可选的 phase 标题（agent 有 phase 时才写入）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// 已发布的子 agent Session id。
    pub child_id: String,
}

/// `tool-workflow/agent-end` 的 data：子 agent 调用结算。
/// 官方：packages/workflow/tool-workflow/src/types.ts 的 SessionEventMap['tool-workflow/agent-end']
/// 用在工作流内 agent() 调用结束事件（seq 是唯一配对键）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolWorkflowAgentEndData {
    pub run_id: String,
    /// 与 agent-start 的 seq 配对。
    pub seq: u32,
    pub outcome: WorkflowAgentOutcome,
}

/// 工作流 agent 结果（官方 WorkflowAgentOutcome）。
/// 用在 ToolWorkflowAgentEndData.outcome。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowAgentOutcome {
    Completed,
    Failed,
    Cancelled,
}
