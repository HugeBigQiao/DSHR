//! 工具事件族。
//!
//! 对应官方 `SessionEventMap` 中 `tool/call`、`tool/result` 两组
//! 以及 code-mode 的 `tool/code-dispatch`、`tool/code-dispatch-start`
//! （`tool-workflow/*` 等扩展在工作流文件，由 fallback 兜住）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;
use crate::session_event::message::Message;

/// `tool/call` 的 data：模型请求调用一个工具。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['tool/call']
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
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['tool/result']
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
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['tool/result'].error
/// 用在 ToolResultData.error。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultError {
    pub name: String,
    pub code: String,
}

/// `tool/code-dispatch-start` 的 data：code-mode 子调用开始执行。
/// 官方：packages/core/tools/src/types.ts 的 CodeDispatchStartEventData
/// 用在子派发事件（调度器真正开始执行时追加，非提交时；UI 按 subCallId 配对实时运行态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeDispatchStartData {
    /// 最外层 run_code 的 call id。
    pub root_call_id: String,
    /// 父 run_code 的 call id。
    pub parent_call_id: String,
    /// 确定性 id：`<parent>:code:<n>`，按提交顺序编号。
    pub sub_call_id: String,
    /// 子工具名。
    pub name: String,
    /// 已解析的参数对象（与 tool/call 的原始 JSON 字符串不同，dispatch 前归一化快照）。
    pub arguments: serde_json::Value,
}

/// `tool/code-dispatch` 的 data：code-mode 子调用的结算。
/// 官方：packages/core/tools/src/types.ts 的 CodeDispatchEventData
/// 用在子派发结束事件（与 code-dispatch-start 按 subCallId 配对，abort 也算 isError）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeDispatchData {
    /// 最外层 run_code 的 call id。
    pub root_call_id: String,
    /// 父 run_code 的 call id。
    pub parent_call_id: String,
    /// 与 code-dispatch-start 配对的确定性 id。
    pub sub_call_id: String,
    /// 子工具名。
    pub name: String,
    /// 已解析的参数对象。
    pub arguments: serde_json::Value,
    /// 子调用是否出错（abort 也算）。
    pub is_error: bool,
    /// 完整模型可见结果，与 tool/result 同词汇。
    pub content: Vec<ContentBlock>,
}
