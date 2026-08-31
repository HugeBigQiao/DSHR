//! 任务页用户操作处理（update 委托进来；与 [`super`] 的类型定义分开，保持 <400 行）。

use iced::Task;
use iced::widget::text_editor;

use dshr_state::Command;

use crate::app::App;
use crate::message::{MenuTarget, Message};
use crate::task::{RenameState, request_file_tree};

pub fn handle_add_pressed(app: &mut App) -> Task<Message> {
    app.task.add_mode = true;
    app.task.name_input.clear();
    app.task.path_input.clear();
    Task::none()
}

pub fn handle_cancel_add(app: &mut App) -> Task<Message> {
    app.task.add_mode = false;
    Task::none()
}

pub fn handle_name_changed(app: &mut App, v: String) -> Task<Message> {
    app.task.name_input = v;
    Task::none()
}

pub fn handle_path_changed(app: &mut App, v: String) -> Task<Message> {
    app.task.path_input = v;
    Task::none()
}

pub fn handle_confirm_add(app: &mut App) -> Task<Message> {
    let name = app.task.name_input.trim().to_string();
    // 决策 21：工作区可不填（空 → None，之后可补设，一旦设置锁死）。
    let cwd = {
        let c = app.task.path_input.trim().to_string();
        if c.is_empty() { None } else { Some(c) }
    };
    app.task.add_mode = false;
    if !name.is_empty() {
        if let Some(state) = &app.state {
            state.send_command(Command::Start { name, cwd });
        }
    }
    Task::none()
}

pub fn handle_new_session(app: &mut App, runtime_id: String) -> Task<Message> {
    if let Some(state) = &app.state {
        state.send_command(Command::NewSession { runtime_id });
    }
    Task::none()
}

pub fn handle_select_session(app: &mut App, session_id: String) -> Task<Message> {
    // 定位归属 runtime（文件树/工作区用）。
    let runtime_id = app
        .task
        .runtimes
        .iter()
        .find(|rt| rt.sessions.iter().any(|s| s.id == session_id))
        .map(|rt| rt.id.clone());
    app.task.selected = Some(session_id);
    app.task.active_runtime = runtime_id.clone();
    app.task.messages.clear(); // 简单版：切换会话只显示之后的新消息
    if let Some(rt_id) = runtime_id {
        request_file_tree(app, &rt_id);
    }
    Task::none()
}

pub fn handle_input_action(app: &mut App, action: text_editor::Action) -> Task<Message> {
    app.task.input.perform(action);
    Task::none()
}

pub fn handle_send_pressed(app: &mut App) -> Task<Message> {
    let text = app.task.input.text().trim().to_string();
    app.task.input = text_editor::Content::new();
    if !text.is_empty() {
        if let Some(session_id) = &app.task.selected {
            if let Some(state) = &app.state {
                state.send_command(Command::Send {
                    session_id: session_id.clone(),
                    text,
                });
            }
        }
    }
    Task::none()
}

pub fn handle_toggle_input_expand(app: &mut App) -> Task<Message> {
    app.task.input_expanded = !app.task.input_expanded;
    Task::none()
}

pub fn handle_toggle_menu(app: &mut App, target: MenuTarget) -> Task<Message> {
    // 点同一个目标 = 收起；否则切换（含关闭正在进行的改名）。
    if app.task.menu.as_ref() == Some(&target) {
        app.task.menu = None;
    } else {
        app.task.menu = Some(target);
    }
    app.task.renaming = None;
    Task::none()
}

pub fn handle_start_rename(app: &mut App, target: MenuTarget) -> Task<Message> {
    // 进入改名态：预填当前名字。
    let current = match &target {
        MenuTarget::Runtime(id) => app
            .task
            .runtimes
            .iter()
            .find(|rt| &rt.id == id)
            .map(|rt| rt.name.clone()),
        MenuTarget::Session(id) => app
            .task
            .runtimes
            .iter()
            .flat_map(|rt| &rt.sessions)
            .find(|s| &s.id == id)
            .map(|s| s.title.clone()),
    };
    app.task.renaming = Some(RenameState {
        target,
        input: current.unwrap_or_default(),
    });
    Task::none()
}

pub fn handle_rename_changed(app: &mut App, v: String) -> Task<Message> {
    if let Some(r) = &mut app.task.renaming {
        r.input = v;
    }
    Task::none()
}

pub fn handle_confirm_rename(app: &mut App) -> Task<Message> {
    if let Some(state) = &app.state {
        if let Some(r) = app.task.renaming.take() {
            let name = r.input.trim().to_string();
            if !name.is_empty() {
                match r.target {
                    MenuTarget::Runtime(id) => {
                        state.send_command(Command::RenameRuntime {
                            runtime_id: id,
                            name,
                        });
                    }
                    MenuTarget::Session(id) => {
                        state.send_command(Command::RenameSession {
                            session_id: id,
                            name,
                        });
                    }
                }
            }
        }
    }
    app.task.menu = None;
    Task::none()
}

pub fn handle_cancel_rename(app: &mut App) -> Task<Message> {
    app.task.renaming = None;
    Task::none()
}

pub fn handle_workspace_changed(app: &mut App, v: String) -> Task<Message> {
    app.task.workspace_input = v;
    Task::none()
}

pub fn handle_workspace_add(app: &mut App) -> Task<Message> {
    app.task.workspace_add = true;
    app.task.workspace_input.clear();
    Task::none()
}

pub fn handle_confirm_workspace(app: &mut App) -> Task<Message> {
    let cwd = app.task.workspace_input.trim().to_string();
    app.task.workspace_add = false;
    app.task.workspace_input.clear();
    if !cwd.is_empty() {
        if let Some(state) = &app.state {
            if let Some(rt_id) = app.task.active_runtime.clone() {
                state.send_command(Command::SetWorkspace {
                    runtime_id: rt_id,
                    cwd,
                });
            }
        }
    }
    Task::none()
}

pub fn handle_cancel_workspace(app: &mut App) -> Task<Message> {
    app.task.workspace_add = false;
    app.task.workspace_input.clear();
    Task::none()
}

pub fn handle_archive_runtime(app: &mut App, runtime_id: String) -> Task<Message> {
    if let Some(state) = &app.state {
        state.send_command(Command::ArchiveRuntime { runtime_id });
    }
    app.task.menu = None;
    Task::none()
}

pub fn handle_delete_runtime(app: &mut App, runtime_id: String) -> Task<Message> {
    // 决策 20：彻底删除（连坐全部数据），UI 后续加二次确认。
    if let Some(state) = &app.state {
        state.send_command(Command::DeleteRuntime { runtime_id });
    }
    app.task.menu = None;
    Task::none()
}

pub fn handle_archive_session(app: &mut App, session_id: String) -> Task<Message> {
    if let Some(state) = &app.state {
        state.send_command(Command::ArchiveSession { session_id });
    }
    app.task.menu = None;
    Task::none()
}

pub fn handle_delete_session(app: &mut App, session_id: String) -> Task<Message> {
    if let Some(state) = &app.state {
        state.send_command(Command::DeleteSession { session_id });
    }
    app.task.menu = None;
    Task::none()
}

pub fn handle_file_open(app: &mut App, path: String) -> Task<Message> {
    if let Some(state) = &app.state {
        if let Some(rt_id) = app.task.active_runtime.clone() {
            state.send_command(Command::ListWorkspace {
                runtime_id: rt_id,
                path,
            });
        }
    }
    Task::none()
}

pub fn handle_file_up(app: &mut App) -> Task<Message> {
    // 去掉最后一段（"a/b" → "a"；"a" → ""）。
    let parent = app
        .task
        .file_path
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default();
    if let Some(state) = &app.state {
        if let Some(rt_id) = app.task.active_runtime.clone() {
            state.send_command(Command::ListWorkspace {
                runtime_id: rt_id,
                path: parent,
            });
        }
    }
    Task::none()
}
