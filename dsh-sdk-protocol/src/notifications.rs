//! 通知侧 wire 类型（dsh → dshr）。
//!
//! 官方：packages/sdk/protocol/src/types.ts 的 HarnessSdkNotificationMap（4 个通知）。
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
/// - `Ok(Some(kind))`：已知方法解析成功
/// - `Ok(None)`：未知方法（协议演进，跳过）
/// - `Err`：已知方法但内容畸形（记日志，不该静默）
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
