//! `deliverables/presented`：present 工具成功交付文件后的持久化声明。
//!
//! 主要用途：记录某 turn 内 present 工具交付了哪些文件（`DeliverablesPresentedData` /
//! `PresentedFile`）。
//! 为什么需要：这是「本次任务产出了什么」的唯一结构化事实（不是从 tool/result 的模型可见
//! 文本里猜），桌面端的文件页/交付物列表直接依赖它；单列文件是因为它的官方出处与内容块/
//! 会话事件都不同（fs 工具包）。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（交付物列表）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/fs/tool-present/src/types.ts 的 `SessionEventMap['deliverables/presented']`。
use serde::{Deserialize, Serialize};

/// `deliverables/presented` 的 data：本轮交付的文件列表。
///
/// 为什么需要：落库/UI 只关心「这个 turn 交付了什么」，`call_id` 保留是为了能反查
/// 是哪个 present 工具调用产生的（与 tool/call 配对）。
/// 官方：packages/fs/tool-present/src/types.ts 的 `SessionEventMap['deliverables/presented']`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverablesPresentedData {
    pub turn: u64,
    pub call_id: String,
    pub files: Vec<PresentedFile>,
}

/// 一个被声明交付的文件。
///
/// 为什么需要：`path` 可能是绝对路径或相对 Session 工作目录——这条歧义必须保留原样
///（不能在这里规范化，否则丢掉了交付时的原始意图），由消费方按工作目录解析。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentedFile {
    /// 原始绝对路径或相对 Session 工作目录的路径。
    pub path: String,
    /// 模型给出的可选说明。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
