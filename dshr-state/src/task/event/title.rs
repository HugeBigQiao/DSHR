//! session/title 事件处理：落库会话标题 + runtime 自动命名钩子。
//!
//! 自动命名（决策 21）：runtime 默认"新任务"，首个会话标题到达时自动把
//! runtime 名同步成标题；用户手动改名（Rename 命令）后停止跟随。

use dshr_protocol::session_event::SessionEvent;

use crate::task::RuntimeTask;
use crate::task::events::UiEvent;

/// 处理 SessionTitle：落库 + （可选）自动命名 runtime。
/// 接收：归属 + 事件（骨架分发进来，仅 Title）。
/// 生成：sessions.title 落库；首个标题时 runtime 改名 + RuntimeRenamed 事件。
pub fn handle(task: &mut RuntimeTask, session_id: &str, event: &SessionEvent) {
    let SessionEvent::SessionTitle { data, .. } = event else {
        return;
    };
    let title = data.title.trim().to_string();
    if title.is_empty() {
        return;
    }
    let _ = task
        .store
        .lock()
        .unwrap()
        .update_session_title(session_id, &title);

    // 自动命名：仅当用户没手动改过名（auto_named）且还没自动命名过（titled）。
    if task.auto_named && !task.titled {
        task.titled = true;
        task.info.name = title.clone();
        let _ = task
            .store
            .lock()
            .unwrap()
            .update_runtime_name(&task.info.id, &title);
        let _ = task.ev_tx.send(UiEvent::RuntimeRenamed {
            runtime_id: task.info.id.clone(),
            name: title,
        });
    }
}
