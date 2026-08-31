//! turn 生命周期处理（48 事件中唯一有落库副作用的族）：
//! turn/start 开 turns 行 + 回填 requests.turn_id；turn/end 回填耗时/reason/token。

use std::sync::{Arc, Mutex};

use dshr_protocol::session_event::SessionEvent;
use tokio::sync::mpsc;

use crate::core::session::SessionTracker;
use crate::core::store::Store;
use crate::core::transcode;
use crate::task::bridge::RtInfo;
use crate::task::events::UiEvent;

/// 处理 turn/start、turn/end（骨架分发进来，其余事件不会到这里）。
/// 接收：归属信息 + 库 + 事件通道（字段级拆分借用，避免与 tracker 冲突）+ 状态机。
/// 生成：turns 行开/关 + requests.turn_id 回填 + UiEvent::TurnEnd。
pub fn handle(
    info: &RtInfo,
    store: &Arc<Mutex<Store>>,
    ev_tx: &mpsc::UnboundedSender<UiEvent>,
    session_id: &str,
    event: &SessionEvent,
    tracker: &mut SessionTracker,
) {
    match event {
        SessionEvent::TurnStart { time, data, .. } => {
            let turn_id =
                transcode::on_turn_start(&info.id, session_id, data.turn, *time as i64, tracker);
            let _ = store.lock().unwrap().insert_turn(
                &turn_id,
                &info.id,
                session_id,
                data.turn as i64,
                *time as i64,
            );
            let _ = store
                .lock()
                .unwrap()
                .update_request_turn_id(&info.id, session_id, &turn_id);
        }
        SessionEvent::TurnEnd { time, data, .. } => {
            if let Some((fin, reason)) = transcode::on_turn_end(&data.reason, *time as i64, tracker)
            {
                let duration = fin.ended_at.saturating_sub(fin.started_at);
                let u = fin.usage.as_ref();
                let _ = store.lock().unwrap().finish_turn(
                    &fin.turn_id,
                    Some(fin.ended_at),
                    Some(duration),
                    Some(&reason),
                    u.map(|x| x.input_tokens as i64),
                    u.map(|x| x.output_tokens as i64),
                    u.and_then(|x| x.cache_read_tokens).map(|x| x as i64),
                    u.and_then(|x| x.cache_write_tokens).map(|x| x as i64),
                    u.and_then(|x| x.reasoning_tokens).map(|x| x as i64),
                    fin.user_text.as_deref(),
                    fin.assistant_text.as_deref(),
                );
                let _ = ev_tx.send(UiEvent::TurnEnd {
                    runtime_id: info.id.clone(),
                    session_id: session_id.to_string(),
                    turn: fin.turn,
                    reason,
                    usage: fin.usage,
                });
            }
        }
        _ => {}
    }
}
