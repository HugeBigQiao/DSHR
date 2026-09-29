//! 通知侧 wire 类型（dsh → dshr）。
//!
//! 主要用途：承接 4 个**通知**方法的载荷类型，并提供唯一的解析入口 `parse`
//!（method → `Kind`），供消费方 match 分流。
//! 为什么需要：通知是「dsh 主动发的」，解析策略必须与请求相反——未知 method 要跳过
//!（协议演进），已知 method 但内容畸形要报错（不静默）。把这条策略固定在 `parse` 一处、
//! 并把 4 个载荷类型收在一个文件，才能让「通知面只有 4 种」这件事可被一眼核对
//!（改这里等于改协议面，需同步 DESIGN §4.1 与 59 事件全集的维护纪律）。
//! 上接：`dsh-sdk-client` 的 transport.rs（把 `rpc::Notification` 广播出去）与
//!       subscription.rs / api.rs；`dshr-state` 的 raw 层与 fold（按 Kind 分流）。
//! 下接：`session_event::SessionEvent`（session.event 的通知载荷）、
//!       `content_block::ContentBlock`（subagent.finished 的最后助手消息）、
//!       `subagent::SubagentStopReason`、`rpc::ParseError`（解析错误类型）。
//!
//! 官方对应：packages/sdk/protocol/src/types.ts 的 HarnessSdkNotificationMap（4 个通知：
//! `SessionEventNotification` / `SessionStatusNotification` / `SubagentStartedNotification` /
//! `SubagentFinishedNotification`）。
//! 方向：这些是"dsh 主动发的"，和 requests.rs（你发的）相对；
//! `Kind` 是解析后的分发入口，state 用它 match 分流。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;
use crate::rpc::ParseError;
use crate::session_event::SessionEvent;
use crate::subagent::SubagentStopReason;

/// 部署映射的运行结果（官方 SdkRunStatus：'ok' | 'error'）。
/// 用在 SubagentFinishedNotification.status。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SdkRunStatus {
    Ok,
    Error,
}

/// 会话状态（官方 SessionStatusNotification.status：'idle' | 'running'）。
/// 用在 SessionStatusNotification.status。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Idle,
    Running,
}

/// `session.event` 通知：一条会话日志事件。
/// 官方：packages/sdk/protocol/src/types.ts 的 SessionEventNotification
/// 用在会话事件流（event 是 session_event.rs 的 SessionEvent）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEventNotification {
    pub session_id: String,
    pub event: SessionEvent,
}

/// `session.status` 通知：整代理生命周期状态。
/// 官方：packages/sdk/protocol/src/types.ts 的 SessionStatusNotification
/// 用在状态切换（idle/running）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusNotification {
    pub session_id: String,
    pub status: SessionStatus,
}

/// `subagent.started` 通知：runtime 内创建了子会话。
/// 官方：packages/sdk/protocol/src/types.ts 的 SubagentStartedNotification
/// 用在会话树血缘（parent → child）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStartedNotification {
    pub parent_session_id: String,
    pub child_session_id: String,
}

/// `subagent.finished` 通知：子代理运行结束。
/// 官方：packages/sdk/protocol/src/types.ts 的 SubagentFinishedNotification
/// 用在会话树完成标记（本地运行的子代理才上报）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentFinishedNotification {
    pub provider: String,
    pub agent_id: String,
    pub parent_session_id: String,
    pub child_session_id: String,
    pub status: SdkRunStatus,
    pub stop_reason: SubagentStopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<Vec<ContentBlock>>,
}

/// 解析后的通知（4 种之一）。
/// 官方：HarnessSdkNotificationMap 的 4 个成员
/// 用在 state 的分发入口（match 后按种类处理）。
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    SessionEvent(SessionEventNotification),
    SessionStatus(SessionStatusNotification),
    SubagentStarted(SubagentStartedNotification),
    SubagentFinished(SubagentFinishedNotification),
}

/// 按 method 解析帧通知。
///
/// # 参数
/// - `notification`：`rpc::classify` 产出的原始通知帧（method + params）。
///
/// # 返回
/// - `Ok(Some(kind))`：已知方法解析成功
/// - `Ok(None)`：未知方法（协议演进，跳过）
/// - `Err`：已知方法但内容畸形（记日志，不该静默）
///
/// 为什么需要：这是「解析策略」的唯一落点。请求侧对坏数据必须报错（你发错了），
/// 通知侧对未知方法必须静默跳过（官方加了新通知不该炸掉整个消费循环），
/// 两种相反的宽容度分开写在这里，消费方不必各自判断。
pub fn parse(notification: &crate::rpc::Notification) -> Result<Option<Kind>, ParseError> {
    let params = &notification.params;
    let kind = match notification.method.as_str() {
        "session.event" => {
            Kind::SessionEvent(serde_json::from_value(params.clone()).map_err(ParseError::Json)?)
        }
        "session.status" => {
            Kind::SessionStatus(serde_json::from_value(params.clone()).map_err(ParseError::Json)?)
        }
        "subagent.started" => {
            Kind::SubagentStarted(serde_json::from_value(params.clone()).map_err(ParseError::Json)?)
        }
        "subagent.finished" => Kind::SubagentFinished(
            serde_json::from_value(params.clone()).map_err(ParseError::Json)?,
        ),
        _ => return Ok(None),
    };
    Ok(Some(kind))
}
