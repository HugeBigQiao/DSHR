//! run API：对应官方 packages/sdk/client/src/api.ts 的 DeepSeekHarness.run 语义（receipt-to-idle）。
//!
//! 主要用途：提供一次「发消息并等到答完」的高层调用 `HarnessClient::run`，
//! 返回 `RunResult`（finalResponse + 本次收集的事件/通知）。
//! 为什么需要：绝大多数调用方只想要「问一句、拿到回复」，不想自己处理订阅游标、
//! 回执配对与 idle 判定这三件容易出错的事；把它们封成一个方法，才能保证
//! 「先订阅再 prompt」这类顺序约束不被各调用方重复踩坑。它是 client.rs 之上的唯一便捷层，
//! 故与总装师分文件（client.rs 保持薄）。
//! 上接：`dshr-state` 的 raw 层/全链路入口（M2 前由 `session.rs` 使用）；
//!       集成测试（重建后放 tests/，见 tests/_conventions.md）。
//! 下接：`crate::client::HarnessClient`（prompt / subscribe）、`crate::subscription::Subscription`
//!       （会话树过滤与等待）、`dsh_sdk_protocol::notifications`（解析通知）、
//!       `dsh_sdk_protocol::session_event::SessionEvent`（识别回执与助手消息）。
//!
//! 官方对应：packages/sdk/client/src/api.ts 的 `DeepSeekHarness.run` 与 `RunResult`
//!（`HarnessSession` 的多轮语义暂未 port，本文件只做单次 run）。
//!
//! 流程：prompt 入队 → 等 `agent/inbox/spliced` 回执（inserted 含 messageId）→
//! 收集事件到根会话 `session.status: idle` → 返回间隔内最后一条根会话助手文本（finalResponse）。
use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::notifications::{self, Kind};
use dsh_sdk_protocol::requests::{SdkPromptContentBlock, SessionPromptParams};
use dsh_sdk_protocol::rpc::Notification;
use dsh_sdk_protocol::session_event::SessionEvent;

use crate::client::HarnessClient;
use crate::error::Error;
use crate::subscription::Subscription;

/// 一次 run 的结果（对应官方 RunResult，api.ts）。
///
/// 为什么需要：把「最终回答」与「过程证据」一次交回——调用方通常只要 final_response，
/// 但排查/落库需要 events 与 notifications；三者同批返回才能保证它们属于同一次 run。
#[derive(Debug)]
pub struct RunResult {
    pub session_id: String,
    /// 间隔内根会话最后一条已提交的助手文本（对应官方 finalResponse）。
    pub final_response: Option<String>,
    /// 间隔内收集的会话树事件。
    pub events: Vec<SessionEvent>,
    /// 间隔内收集的全部通知（含状态/子会话）
    pub notifications: Vec<Notification>,
}

impl HarnessClient {
    /// run：prompt → 等 inbox 回执 → 收集事件到根会话 idle。
    ///
    /// # 参数
    /// - `session_id`：目标会话（未知 id 由服务端懒创建；**必须唯一**，复用固定 id 会撞官方
    ///   磁盘持久化日志，见 DESIGN §10）。
    /// - `blocks`：本次要发的内容块（文本/内联图片）。
    /// - `timeout_ms`：整次 run 的墙钟上限。
    ///
    /// # 返回
    /// `RunResult`：间隔内的根会话 final_response、会话树事件与会话全部通知。
    ///
    /// # 错误
    /// `Error::RequestTimeout`（`run(<session_id>)`，等待超时，含「idle 一直不来」）、
    /// `Error::TransportClosed`（runtime 中途退出）、以及 prompt 自身的请求错误。
    ///
    /// 官方：api.ts 的 DeepSeekHarness.run（receipt-to-idle；finalResponse = 最后一条
    ///       已提交的根会话助手文本，不按因果归属单条 prompt——steering/注入可能先产出）。
    /// `timeout_ms` 是 SDK 扩展（官方不设 run 超时；这里防 runtime 挂死）。
    pub async fn run(
        &mut self,
        session_id: &str,
        blocks: Vec<SdkPromptContentBlock>,
        timeout_ms: u64,
    ) -> Result<RunResult, Error> {
        // 先订阅再 prompt：避免漏掉回执（广播游标在 prompt 前）。
        let mut sub = Subscription::scoped(self.subscribe(), session_id);
        let params = SessionPromptParams {
            session_id: session_id.to_string(),
            content_blocks: blocks,
        };
        let receipt = self.prompt(&params).await?.message_id;

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        let mut events = Vec::new();
        let mut notifications = Vec::new();
        let mut final_response: Option<String> = None;
        let mut receipt_seen = false;

        loop {
            let n = match tokio::time::timeout_at(deadline, sub.next()).await {
                Err(_) => {
                    return Err(Error::RequestTimeout {
                        method: format!("run({session_id})"),
                        timeout_ms,
                    });
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(n)) => n,
            };

            let mut done = false;
            if n.method == "session.status" {
                // 完成条件：根会话 idle 且已见回执。
                let sid = n
                    .params
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str);
                let status = n.params.get("status").and_then(serde_json::Value::as_str);
                done = sid == Some(session_id) && status == Some("idle") && receipt_seen;
            } else if n.method == "session.event" {
                if let Ok(Some(Kind::SessionEvent(notif))) = notifications::parse(&n) {
                    let is_root = notif.session_id == session_id;
                    match &notif.event {
                        // 回执：inserted 含本次 prompt 的 messageId。
                        SessionEvent::AgentInboxSpliced { data, .. } => {
                            if data.inserted.iter().any(|m| m.id == receipt) {
                                receipt_seen = true;
                            }
                        }
                        // finalResponse：根会话最后一条已提交的助手消息文本。
                        SessionEvent::AssistantMessage { data, .. } if is_root => {
                            if let Some(text) = assistant_text(&data.message.content) {
                                final_response = Some(text);
                            }
                        }
                        _ => {}
                    }
                    events.push(notif.event);
                }
            }
            notifications.push(n);
            if done {
                break;
            }
        }

        Ok(RunResult {
            session_id: session_id.to_string(),
            final_response,
            events,
            notifications,
        })
    }
}

/// 提取一条助手消息的文本（text 块拼接；无文本块返回 None）。
fn assistant_text(blocks: &[ContentBlock]) -> Option<String> {
    let parts: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.concat())
    }
}
