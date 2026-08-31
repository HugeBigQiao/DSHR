//! 任务页：三栏布局（左：任务树 / 中：对话 / 右：文件树）。
//!
//! 本文件：状态类型（TaskPane/RtView/...）+ 后台事件应用 + 用户操作处理 + 三栏组装。
//! 渲染拆到 [`sidebar`]（左）/ [`chat`]（中）/ [`files`]（右），各文件 <400 行（决策 22）。

pub mod chat;
pub mod files;
pub mod ops;
pub mod sidebar;

pub use ops::*;

use iced::Element;
use iced::widget::text_editor;

use dshr_state::{Command, UiEvent, UiStatus};

use crate::app::App;
use crate::message::{MenuTarget, Message};

/// 改名编辑态（内联输入）。
pub struct RenameState {
    pub target: MenuTarget,
    pub input: String,
}

/// 任务页状态（App.task）。
pub struct TaskPane {
    pub runtimes: Vec<RtView>,
    /// 当前选中会话 id。
    pub selected: Option<String>,
    /// 选中会话的归属 runtime（文件树/工作区操作定位用）。
    pub active_runtime: Option<String>,
    /// 当前选中会话的消息缓存。
    pub messages: Vec<MsgView>,
    /// 输入框（多行编辑器，支持伸缩）。
    pub input: text_editor::Content<iced::Renderer>,
    pub input_expanded: bool,
    /// 添加 runtime 弹窗状态。
    pub add_mode: bool,
    pub name_input: String,
    pub path_input: String,
    /// "..."菜单当前展开目标。
    pub menu: Option<MenuTarget>,
    /// 内联改名态（None = 未在改名）。
    pub renaming: Option<RenameState>,
    /// 补设工作区弹窗状态。
    pub workspace_add: bool,
    pub workspace_input: String,
    /// 文件树：当前目录 + 条目。
    pub file_path: String,
    pub files: Vec<dshr_state::UiFileEntry>,
}

impl Default for TaskPane {
    fn default() -> Self {
        Self {
            runtimes: Vec::new(),
            selected: None,
            active_runtime: None,
            messages: Vec::new(),
            input: text_editor::Content::new(),
            input_expanded: false,
            add_mode: false,
            name_input: String::new(),
            path_input: String::new(),
            menu: None,
            renaming: None,
            workspace_add: false,
            workspace_input: String::new(),
            file_path: String::new(),
            files: Vec::new(),
        }
    }
}

/// 侧边栏里的一个 runtime（渲染树）。
pub struct RtView {
    pub id: String,
    pub name: String,
    /// 工作区（None = 未设置，UI 显示提示）。
    pub workspace: Option<String>,
    pub status: UiStatus,
    pub sessions: Vec<SessionView>,
}

/// 侧边栏里的一个会话。
pub struct SessionView {
    pub id: String,
    pub title: String,
}

/// 聊天区的一行（简单版：消息 / 工具摘要 / 轮结束 / 信息 / 错误）。
pub enum MsgView {
    User(String),
    Assistant {
        text: String,
        reasoning: Option<String>,
    },
    Tool(String),
    TurnEnd(String),
    Info(String),
    /// 对话内错误（红色字体）。
    Error(String),
}

/// 下载进度弹窗状态（dsh 首次安装/更新时显示）。
pub struct FetchUi {
    /// 进度行（保留最近 60 行，避免刷屏）。
    pub lines: Vec<String>,
    /// 是否已结束（显示关闭按钮）。
    pub done: bool,
    pub ok: bool,
}

impl Default for FetchUi {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            done: false,
            ok: false,
        }
    }
}

/// 一条后台事件 → 任务页状态（runtime 全生命周期事件只服务本页）。
pub fn apply_event(app: &mut App, ev: UiEvent) {
    match ev {
        UiEvent::Status {
            runtime_id,
            status,
            name,
            workspace,
        } => {
            // 首次到达（Connecting）时创建侧边栏节点；之后只更新状态。
            if let Some(rt) = app.task.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                rt.status = status;
                if workspace.is_some() {
                    rt.workspace = workspace.clone();
                }
            } else {
                app.task.runtimes.push(RtView {
                    id: runtime_id.clone(),
                    name,
                    workspace: workspace.clone(),
                    status,
                    sessions: Vec::new(),
                });
            }
            // 工作区补设成功（Ready + 有 workspace）→ 刷新文件树。
            if status == UiStatus::Ready && workspace.is_some() {
                request_file_tree(app, &runtime_id);
            }
            // 第一个 runtime Ready 后自动开会话，方便直接发消息。
            if status == UiStatus::Ready && app.task.selected.is_none() {
                if let Some(rt) = app.task.runtimes.iter().find(|r| r.id == runtime_id) {
                    if let Some(state) = &app.state {
                        state.send_command(Command::NewSession {
                            runtime_id: rt.id.clone(),
                        });
                    }
                }
            }
        }
        UiEvent::RuntimeRenamed { runtime_id, name } => {
            if let Some(rt) = app.task.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                rt.name = name;
            }
        }
        UiEvent::SessionCreated {
            runtime_id,
            session_id,
        } => {
            if let Some(rt) = app.task.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                rt.sessions.push(SessionView {
                    id: session_id.clone(),
                    title: "新会话".to_string(),
                });
            }
            if app.task.selected.is_none() {
                app.task.selected = Some(session_id.clone());
                app.task.active_runtime = Some(runtime_id.clone());
                app.task.messages.clear();
                request_file_tree(app, &runtime_id);
            }
        }
        UiEvent::SessionRemoved {
            runtime_id,
            session_id,
        } => {
            // 归档/删除后从侧边栏移除会话；若选中被移除则清空选择。
            if let Some(rt) = app.task.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                rt.sessions.retain(|s| s.id != session_id);
            }
            if app.task.selected.as_deref() == Some(session_id.as_str()) {
                app.task.selected = None;
                app.task.active_runtime = None;
                app.task.messages.clear();
            }
        }
        UiEvent::Title {
            runtime_id,
            session_id,
            title,
            ..
        } => {
            if let Some(rt) = app.task.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                if let Some(s) = rt.sessions.iter_mut().find(|s| s.id == session_id) {
                    s.title = title.clone();
                }
            }
        }
        UiEvent::Message {
            session_id, msg, ..
        } => {
            if app.task.selected.as_deref() != Some(session_id.as_str()) {
                return;
            }
            match msg.role {
                dshr_protocol::session_event::message::MessageRole::User => {
                    app.task.messages.push(MsgView::User(msg.text));
                }
                dshr_protocol::session_event::message::MessageRole::Assistant => {
                    app.task.messages.push(MsgView::Assistant {
                        text: msg.text,
                        reasoning: msg.reasoning,
                    });
                }
                _ => {}
            }
        }
        UiEvent::ToolUse {
            session_id, tool, ..
        } => {
            if app.task.selected.as_deref() != Some(session_id.as_str()) {
                return;
            }
            let icon = if tool.is_error { "⛔" } else { "🔧" };
            let dur = tool
                .duration_ms
                .map(|d| format!(" ({d}ms)"))
                .unwrap_or_default();
            app.task
                .messages
                .push(MsgView::Tool(format!("{icon} {}{dur}", tool.name)));
            if let Some(result) = tool.result {
                app.task.messages.push(MsgView::Tool(result));
            }
        }
        UiEvent::TurnEnd { usage, reason, .. } => {
            let usage_str = usage
                .map(|u| {
                    format!(
                        " in {} / out {} / reasoning {}",
                        u.input_tokens,
                        u.output_tokens,
                        u.reasoning_tokens.unwrap_or(0)
                    )
                })
                .unwrap_or_default();
            app.task.messages.push(MsgView::TurnEnd(format!(
                "—— 回合结束（{reason}）{usage_str} ——"
            )));
        }
        UiEvent::FileTree {
            runtime_id,
            path,
            entries,
            ..
        } => {
            if app.task.runtimes.iter().any(|r| r.id == runtime_id) {
                app.task.file_path = path;
                app.task.files = entries;
            }
        }
        UiEvent::Log { message, .. } => {
            app.task
                .messages
                .push(MsgView::Info(format!("⚙ {message}")));
        }
        UiEvent::Toast(msg) => {
            // 操作/全局错误 → 弹窗（App.dialog，view 层叠加）。
            app.dialog = Some(msg);
        }
        UiEvent::InlineError(e) => {
            // 对话内错误 → 红色显示在对话流。
            app.task.messages.push(MsgView::Error(e));
        }
        UiEvent::FetchProgress(line) => {
            let ui = app.fetch.get_or_insert_with(FetchUi::default);
            ui.lines.push(line);
            if ui.lines.len() > 60 {
                ui.lines.remove(0);
            }
        }
        UiEvent::FetchDone { ok, message } => {
            let ui = app.fetch.get_or_insert_with(FetchUi::default);
            ui.lines.push(message);
            ui.done = true;
            ui.ok = ok;
        }
    }
}

/// 请求文件树（仅当该 runtime 有工作区时）。
fn request_file_tree(app: &App, runtime_id: &str) {
    if let Some(rt) = app.task.runtimes.iter().find(|r| r.id == runtime_id) {
        if rt.workspace.is_some() {
            if let Some(state) = &app.state {
                state.send_command(Command::ListWorkspace {
                    runtime_id: runtime_id.to_string(),
                    path: String::new(),
                });
            }
        }
    }
}

/// 任务页：左（任务树）/ 中（对话）/ 右（文件树）三栏。
pub fn task_page(app: &App) -> Element<'_, Message> {
    let mut row = iced::widget::row![sidebar::sidebar(app), chat::chat_area(app),];
    // 有工作区的 runtime 才显示文件树（右栏）。
    let has_workspace = app
        .task
        .active_runtime
        .as_ref()
        .and_then(|id| app.task.runtimes.iter().find(|rt| &rt.id == id))
        .and_then(|rt| rt.workspace.as_ref())
        .is_some();
    if has_workspace {
        row = row.push(files::file_tree(app));
    }
    row.into()
}
