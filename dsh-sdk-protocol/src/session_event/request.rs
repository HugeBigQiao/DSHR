//! 请求元数据事件族。
//!
//! 对应官方 `SessionEventMap` 中 `request/header`、`request/context` 两组。
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::session_event::SessionEvent;

    /// 回归：0.1.2-alpha.x 起官方发出 `reason: 'series'`（Agent.buildRequest()），
    /// 严格枚举会让整个 request/header 事件解析失败——必须能解析。
    #[test]
    fn request_header_reason_series_parses() {
        let event: SessionEvent = serde_json::from_value(json!({
            "type": "request/header",
            "seq": 1,
            "time": 100,
            "data": {
                "header": {"config": {}},
                "reason": "series",
                "startsSeries": true
            }
        }))
        .expect("series 应可解析");
        match event {
            SessionEvent::RequestHeader { data, .. } => {
                assert_eq!(data.reason, RequestHeaderReason::Series);
                assert_eq!(data.starts_series, Some(true));
            }
            other => panic!("应解析为 RequestHeader，实际 {other:?}"),
        }
    }

    /// 宽容性：官方 merge-extensible，未来的新 reason 不应让已知事件解析失败。
    #[test]
    fn request_header_reason_unknown_is_tolerated() {
        let event: SessionEvent = serde_json::from_value(json!({
            "type": "request/header",
            "seq": 2,
            "time": 200,
            "data": {"header": {"config": {}}, "reason": "future-reason"}
        }))
        .expect("未知 reason 应落入 Unknown");
        match event {
            SessionEvent::RequestHeader { data, .. } => {
                assert_eq!(data.reason, RequestHeaderReason::Unknown);
            }
            other => panic!("应解析为 RequestHeader，实际 {other:?}"),
        }
    }

    /// 宽容性：已知类型 data 字段漂移（官方发版改字段/结构）时降级 Unknown（lossless），
    /// 不整体报错、不丢事件。fallback.rs 的 known() 是通用解法，此处用缺必需字段模拟漂移。
    #[test]
    fn known_type_parse_failure_degrades_to_unknown() {
        // request/header 缺必需字段 header → 类型化解析失败 → Unknown
        let event: SessionEvent = serde_json::from_value(json!({
            "type": "request/header",
            "seq": 3,
            "time": 300,
            "data": {"reason": "initial"}
        }))
        .expect("漂移的已知事件应降级 Unknown 而非报错");
        match event {
            SessionEvent::Unknown {
                event_type,
                data,
                seq,
                ..
            } => {
                assert_eq!(event_type, "request/header");
                assert_eq!(seq, 3);
                assert_eq!(data, json!({"reason": "initial"}));
            }
            other => panic!("应降级为 Unknown，实际 {other:?}"),
        }
    }
}
