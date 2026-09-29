//! 工具事件族。
//!
//! 主要用途：`tool/call`、`tool/result` 两组核心工具事件的 data 类型，以及 PTC mode 的
//! `tool/ptc-dispatch-start` / `tool/ptc-dispatch`。
//! 为什么需要：工具调用与结果是「模型做了什么」的事实来源（call↔result 按 callId 配对），
//! 落库时对应 tool_calls 表，与消息/生命周期事件的消费方式不同；PTC 子调用虽属另一插件，
//! 但同属「工具执行」语义，放一起可让配对规则只有一处出处。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold / store（工具统计与 file_ops 折叠）。
//! 下接：`session_event/message.rs::Message`（结果消息）、
//!       `crate::content_block::ContentBlock`（PTC 结果的模型可见内容）。
//!
//! 官方对应： `SessionEventMap` 中 `tool/call`、`tool/result` 两组
//! 以及 PTC mode 的 `tool/ptc-dispatch`、`tool/ptc-dispatch-start`
//! （`tool-workflow/*` 等扩展在工作流文件，由 fallback 兜住）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;
use crate::session_event::message::Message;

/// `tool/call` 的 data：模型请求调用一个工具。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['tool/call']
/// 用在模型发起工具调用的事件（监督面板命令视图的数据源）。
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
/// 用在 ToolResultData.error。`reason`（原始面向用户的失败原因，位于模型内容之外）
/// 为官方 0.1.7-rc.2 新增。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultError {
    pub name: String,
    pub code: String,
    /// 原始的用户可见失败原因；仅当消息 `isError: true` 时允许出现。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `tool/ptc-dispatch-start` 的 data：PTC mode 子调用开始执行。
/// 官方：packages/core/tools/src/types.ts 的 PtcDispatchStartEventData
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtcDispatchStartData {
    /// 最外层 run_code 的 call id。
    pub root_call_id: String,
    /// 父 run_code 的 call id。
    pub parent_call_id: String,
    /// 确定性 id：`<parent>:ptc:<n>`，按提交顺序编号。
    pub sub_call_id: String,
    /// 子工具名。
    pub name: String,
    /// 已解析的参数对象（与 tool/call 的原始 JSON 字符串不同，dispatch 前归一化快照）。
    pub arguments: serde_json::Value,
}

/// `tool/ptc-dispatch` 的 data：PTC mode 子调用的结算。
/// 官方：packages/core/tools/src/types.ts 的 PtcDispatchEventData
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtcDispatchData {
    /// 最外层 run_code 的 call id。
    pub root_call_id: String,
    /// 父 run_code 的 call id。
    pub parent_call_id: String,
    /// 与 ptc-dispatch-start 配对的确定性 id。
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
