//! 请求元数据事件族。
//!
//! 主要用途：`request/header`（每次模型请求的请求头快照 + 追加原因）与
//! `request/context`（provider/model/上下文窗口路由元数据）的 data 类型。
//! 为什么需要：这两组是「模型在什么配置下被调用」的唯一权威记录——计费、容量、
//! 供应商比对都要它；`RequestHeaderReason` 还是官方 merge-extensible 字符串联合的典型，
//! 其 `Unknown` 兜底与 `series` 回归（严格枚举会整体解析失败）都必须有明确归属。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的统计域（请求级统计）。
//! 下接：无（`EpochHeader` 里官方复杂类型先用 opaque `serde_json::Value` 占位）。
//!
//! 官方对应：`SessionEventMap` 中 `request/header`、`request/context` 两组
//!（`packages/core/session/src/types.ts`）。
use serde::{Deserialize, Serialize};

/// `request/header` 的 data：下次请求的完整请求头快照。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['request/header']
///      （reason 类型 L201-208；startsSeries 字段 L283-287）
/// 用在模型请求头事件（监管面板配置/计费的数据源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeaderData {
    pub header: EpochHeader,
    pub reason: RequestHeaderReason,
    /// 变更头同时开启一条独立模型消息序列（wire 上 `startsSeries: true`，仅 change 时出现）。
    /// 官方：packages/core/session/src/types.ts 的 SessionEventMap['request/header'].startsSeries
    /// 发出点：packages/core/agent-loop/src/agent.ts 的 Agent.buildRequest()（L511-514）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_series: Option<bool>,
}

/// 为什么追加一个 request/header 快照。
/// 官方：packages/core/session/src/types.ts 的 RequestHeaderReason（L201-208，
///       'initial' | 'resume' | 'change' | 'series'）
/// 用在 RequestHeaderData.reason（wire 上是纯字符串）。
/// `series` 的发出点：packages/core/agent-loop/src/agent.ts 的 Agent.buildRequest()（L516-517，
/// 请求头未变但显式开启新消息序列时 append reason: 'series'）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestHeaderReason {
    Initial,
    Resume,
    Change,
    Series,
    /// 未知原因（官方是 merge-extensible 的字符串联合，backends 可能扩展）。
    /// `#[serde(other)]` 兜住任何其他字符串；纯字符串无载荷，不丢数据。
    /// 参照：subagent.rs 的 SubagentStopReason::Unknown 同款模式。
    #[serde(other)]
    Unknown,
}

/// 一次请求的调用配置快照。
/// 官方：packages/core/session/src/types.ts 的 EpochHeader
/// 用在 RequestHeaderData.header。
/// 简化：config 官方是 LlmCallConfig（复杂），先用 opaque Value；
/// system/tools 同理，用到再补全形状（未知字段反序列化自动忽略）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpochHeader {
    pub config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_defaults: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
}

/// `request/context` 的 data：模型路由元数据。
/// 官方：packages/core/session/src/types.ts 的 RequestContext
/// 用在路由或容量变化时的事件（provider/model/contextWindow）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContextData {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_update: Option<String>,
}
