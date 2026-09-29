//! 会话生命周期/日志类事件族：`session/end-seed`、`session-log-deepseek/delivery-accepted`。
//!
//! 主要用途：`session/end-seed`（构造种子结束标记）与 `delivery-accepted`（会话日志上传送达确认）
//! 两个事件的 data 类型。
//! 为什么需要：`session/end-seed` 是**位置语义**事件——它之前的 seq 都来自种子
//!（resume/fork/replay），载荷是空的，判断含义全靠类型与位置；不解释这点就会被当成无意义事件。
//! 它与 title/request 等「会话内业务事件」不同（一个是边界，一个是内容），故单列文件。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 record（WireLog 回放时区分种子段）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：`session/end-seed` 属核心包 `SessionEventMap`（packages/core/session/src/types.ts）；
//! `delivery-accepted` 由 session-log-deepseek 插件注册（packages/session/
//! session-log-deepseek/src/types.ts 的 declare module，L54-63）。`session/title` 等
//! 会话内其他事件在 title.rs / request.rs 等族文件。
use serde::{Deserialize, Serialize};

/// `session/end-seed` 的 data：空对象，位置和 time 携带含义。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['session/end-seed']
/// 用在构造种子结束的事件（之前的 seq 均来自种子：resume/fork/replay）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEndSeedData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited: Option<bool>,
}

/// `session-log-deepseek/delivery-accepted` 的 data：官方 DeepSeek 会话日志上传送达确认。
/// 官方：packages/session/session-log-deepseek/src/types.ts 的 declare module（L54-63）
/// 用在增量上传被端点接受的事件（throughSeq = 已接受请求里最后一条 canonical event；
/// 继承 fork 标记会保留父会话 id）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAcceptedData {
    /// 已接受投递携带的会话 id。
    pub session_id: String,
    /// 已接受的 Session format generation；缺失表示 version 0。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_format_version: Option<u64>,
    /// 已接受请求包含的最后一条事件序号（官方 branded SessionSeq）。
    pub through_seq: u64,
}
