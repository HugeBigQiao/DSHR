//! 会话生命周期/日志类事件族：`session/end-seed`、`session-log-deepseek/delivery-accepted`。
//!
//! `session/end-seed` 属核心包 `SessionEventMap`（packages/core/session/src/types.ts）；
//! `delivery-accepted` 由 session-log-deepseek 插件注册（packages/session/
//! session-log-deepseek/src/types.ts 的 declare module，L54-63）。`session/title` 等
//! 会话内其他事件在 title.rs / request.rs 等族文件。
use serde::{Deserialize, Serialize};

/// `session/end-seed` 的 data：空对象，位置和 time 携带含义。
/// 官方：packages/core/session/src/types.ts 的 SessionEventMap['session/end-seed']
/// 用在构造种子结束的事件（之前的 seq 均来自种子：resume/fork/replay）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEndSeedData {}

/// `session-log-deepseek/delivery-accepted` 的 data：官方 DeepSeek 会话日志上传送达确认。
/// 官方：packages/session/session-log-deepseek/src/types.ts 的 declare module（L54-63）
/// 用在增量上传被端点接受的事件（throughSeq = 已接受请求里最后一条 canonical event；
/// 继承 fork 标记会保留父会话 id）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAcceptedData {
    /// 已接受投递携带的会话 id。
    pub session_id: String,
    /// 已接受请求包含的最后一条事件序号（官方 branded SessionSeq）。
    pub through_seq: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::session_event::SessionEvent;

    /// 官方形状的 delivery-accepted 事件可解析并 roundtrip。
    #[test]
    fn delivery_accepted_event_roundtrips() {
        let wire = json!({
            "type": "session-log-deepseek/delivery-accepted",
            "seq": 5,
            "time": 500,
            "data": {"sessionId": "s-1", "throughSeq": 4}
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("delivery-accepted 应可解析");
        match &event {
            SessionEvent::SessionLogDeepseekDeliveryAccepted { data, .. } => {
                assert_eq!(data.session_id, "s-1");
                assert_eq!(data.through_seq, 4);
            }
            other => panic!("应解析为 DeliveryAccepted，实际 {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }
}
