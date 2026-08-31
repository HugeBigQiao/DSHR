//! dsh → dshr 事件处理：通知分发 + 会话事件骨架（48 事件按"处理逻辑"拆族）。
//!
//! 骨架（本文件）：lossless 落库 + tracker 懒创建 + 族分发 + 转 UI。
//! 族文件：只有有真实逻辑的族才单开文件（turn 有落库副作用；message/tool/title
//! 的转换集中在 core/transcode——它 <400 行，是"翻译官"；其余事件只落库）。

pub mod fallback;
pub mod title;
pub mod turn;

use dshr_protocol::notifications::Kind;
use dshr_protocol::rpc::Notification;
use dshr_protocol::session_event::SessionEvent;

use crate::core::session::SessionTracker;
use crate::core::transcode;
use crate::inline_err;
use crate::task::RuntimeTask;
use crate::task::events::{UiEvent, UiStatus};

impl RuntimeTask {
    /// 处理一条通知：按 Kind 分发 → 落库 → 转 UI。
    pub fn handle_notification(&mut self, notification: Notification) {
        match dshr_protocol::notifications::parse(&notification) {
            Ok(Some(kind)) => match kind {
                Kind::SessionEvent(n) => {
                    self.handle_session_event(&n.session_id, &n.event);
                }
                Kind::SessionStatus(n) => {
                    let status = match n.status {
                        dshr_protocol::notifications::SessionStatus::Idle => "idle",
                        dshr_protocol::notifications::SessionStatus::Running => "running",
                    };
                    let _ = self
                        .store
                        .lock()
                        .unwrap()
                        .update_session_status(&n.session_id, status);
                }
                Kind::SubagentStarted(n) => {
                    // 血缘：子会话挂到父会话下（懒创建不覆盖已有行）。
                    if !self.trackers.contains_key(&n.child_session_id) {
                        let _ = self.store.lock().unwrap().insert_session(
                            &n.child_session_id,
                            &self.info.id,
                            self.info.workspace.as_deref().unwrap_or_default(),
                            Some(&n.parent_session_id),
                            crate::task::now_ms(),
                            Some("idle"),
                        );
                    }
                }
                Kind::SubagentFinished(_) => {}
            },
            Ok(None) => fallback::handle_unknown_method(&notification.method),
            Err(e) => {
                // 对话内错误：红色显示在对话流。
                inline_err!(&self.ev_tx, "通知解析失败: {e}");
            }
        }
    }

    /// 一条 session.event：lossless 落库（events 表永远先写）→ turn 落库 → 转 UI。
    fn handle_session_event(&mut self, session_id: &str, event: &SessionEvent) {
        // 流式增量不落库（决策 19）：assistant/chunk 逐 token 一条，一个长回答就几万行；
        // 且是纯冗余——文本全文在 assistant/message（turns.assistant_text 也有汇总），
        // token 用量在 turns 表，UI 不渲染它（event_to_ui 返回 None）。
        // 跳过后 events 表一轮 ~20 行；seq 空洞不影响 (session_id, seq) 主键与增量同步。
        if matches!(event, SessionEvent::AssistantChunk { .. }) {
            return;
        }
        // 1. lossless 底线：events 表 + last_seq 书签。
        let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
        let (turn, step) = event.turn_step();
        let _ = self.store.lock().unwrap().insert_event(
            session_id,
            event.seq() as i64,
            event.event_type(),
            event.time() as i64,
            turn.map(|t| t as i64),
            step.map(|s| s as i64),
            &payload,
        );
        let _ = self
            .store
            .lock()
            .unwrap()
            .update_session_last_seq(session_id, event.seq() as i64);

        // 1.5 会话标题：落库 + 自动命名钩子（不需要 tracker，先于懒创建处理）。
        if matches!(event, SessionEvent::SessionTitle { .. }) {
            title::handle(self, session_id, event);
        }

        // 2. 未知 session 懒创建（子会话事件先于通知到达的情况）。
        let tracker = self
            .trackers
            .entry(session_id.to_string())
            .or_insert_with(SessionTracker::new);

        // 3. turn 生命周期：开行/回填（含 requests.turn_id 回填）。
        // 字段级拆分借用（info/store/ev_tx + tracker），避免与 self.trackers 冲突。
        if matches!(
            event,
            SessionEvent::TurnStart { .. } | SessionEvent::TurnEnd { .. }
        ) {
            turn::handle(
                &self.info,
                &self.store,
                &self.ev_tx,
                session_id,
                event,
                tracker,
            );
        }

        // 4. 转 UI（消息/工具/标题，翻译在 core/transcode）。
        if let Some(ui) = transcode::event_to_ui(&self.info.id, session_id, event, tracker) {
            let _ = self.ev_tx.send(ui);
        }
    }
}

/// 事件流关闭时（runtime 退出）的收尾：把状态同步成 Closed。
pub fn on_stream_closed(task: &RuntimeTask) {
    let _ = task.ev_tx.send(UiEvent::Status {
        runtime_id: task.info.id.clone(),
        status: UiStatus::Closed,
        name: task.info.name.clone(),
        workspace: task.info.workspace.clone(),
    });
}
