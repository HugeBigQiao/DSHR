//! 请求元数据事件族。
//!
//! 对应官方 `SessionEventMap` 中 `request/header`、`request/context` 两组。
use serde::{Deserialize, Serialize};

/// `request/header` 的 data：下次请求的完整请求头快照。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['request/header']
/// 用在模型请求头事件（监管面板配置/计费的数据源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestHeaderData {
    pub header: EpochHeader,
    pub reason: RequestHeaderReason,
}

/// 为什么追加一个 request/header 快照。
/// 官方：packages/core/session/src/types.ts 的 RequestHeaderReason
/// 用在 RequestHeaderData.reason（wire 上是纯字符串）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestHeaderReason {
    Initial,
    Resume,
    Change,
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
    pub system: Option<String>,
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
}
