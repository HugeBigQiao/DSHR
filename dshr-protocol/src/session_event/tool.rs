//! 工具事件族。
//!
//! 对应官方 `SessionEventMap` 中 `tool/call`、`tool/result` 两组
//! （`tool-workflow/*` 等扩展在 3a 不 port，由 3b fallback 兜住）。
use serde::{Deserialize, Serialize};

use crate::session_event::message::Message;

/// `tool/call` 的 data：模型请求调用一个工具。
/// 官方：core/session/src/types.ts 的 SessionEventMap['tool/call']
/// 用在模型发起工具调用的事件（监管面板命令视图的数据源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallData {
    pub turn: u64,
    pub step: u64,
    // 官方 branded CallId，先用 String；与 tool/result 配对。
    pub call_id: String,
    pub name: String,
    // 模型产出的原始 JSON 字符串，保持不解析。
    pub arguments: String,
}

/// `tool/result` 的 data：工具执行结果。
/// 官方：core/session/src/types.ts 的 SessionEventMap['tool/result']
/// 用在工具结果事件（结果消息 + 可选内部失败 + 工具私有 meta）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultData {
    pub turn: u64,
    pub step: u64,
    // 完整消息（role=user、content 为单个 tool-result 块）。
    pub message: Message,
    // 可选内部失败标识（缺省时字段不出现，不是 null）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolResultError>,
    // 工具私有展示载荷（如 fs 工具的 diff），对核心 opaque。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// `tool/result` 的可选内部失败标识。
/// 官方：core/session/src/types.ts 的 SessionEventMap['tool/result'].error
/// 用在 ToolResultData.error。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultError {
    pub name: String,
    pub code: String,
}
