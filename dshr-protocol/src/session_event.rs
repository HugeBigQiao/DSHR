//! `SessionEvent`：会话日志事件（`type` 字段打标的判别联合）。
//! 信封 + 判别枚举放本文件，事件 data 按事件族拆到子模块：
//! 每个子模块对应官方 `packages/core/session/src/types.ts` 的 `SessionEventMap` 一组事件。
pub mod approval;
pub mod compaction;
pub mod fallback;
pub mod message;
pub mod misc;
pub mod request;
pub mod session;
pub mod tool;
pub mod turn;

use serde::Serialize;

/// 事件信封：`type` 判别 + 公共字段 `seq/time` + 各自 `data`。
/// 官方：core/session/src/types.ts 的 SessionEvent（404 行起）
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
