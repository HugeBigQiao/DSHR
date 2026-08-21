//! App 状态机：iced 的 Model/Update 部分（渲染在 view.rs）。
//!
//! 职责：持有 AppState 句柄 + UI 侧视图状态；接 Message → 更新状态/发命令；
//! 接 UiEvent（Tick 轮询）→ apply_event 更新渲染缓存。
//! view 只是薄委托（渲染逻辑在 view.rs）。

use std::path::Path;
use std::time::Duration;

use iced::{Subscription, Task};

use dshr_state::core::config;
use dshr_state::ui::{AppState, Command, UiEvent, UiStatus};

use crate::message::Message;
use crate::model::{MsgView, RtView, SessionView};

/// UI 状态。
pub struct App {
    pub state: Option<AppState>,
    pub runtimes: Vec<RtView>,
    /// 当前选中会话 id。
    pub selected: Option<String>,
    /// 当前选中会话的消息缓存。
    pub messages: Vec<MsgView>,
    pub input: String,
    /// 添加 runtime 弹窗状态。
    pub add_mode: bool,
    pub name_input: String,
    pub path_input: String,
    /// 启动错误（配置缺失等）。
    pub boot_error: Option<String>,
}

impl App {
    /// 启动：加载 .env 配置 → 起后台引擎线程（AppState::start）。
    /// 生成：(App, 初始 Task)。配置失败时进入 boot_error 全屏提示。
    pub fn new() -> (Self, Task<Message>) {
        // workspace 根 = dshr-ui 的上一级（dshr/）
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let base = Self {
            state: None,
            runtimes: Vec::new(),
            selected: None,
            messages: Vec::new(),
            input: String::new(),
            add_mode: false,
            name_input: String::new(),
            path_input: String::new(),
            boot_error: None,
        };
        match config::load(&root).and_then(AppState::start) {
            Ok(state) => (
                Self {
                    state: Some(state),
                    ..base
                },
                Task::none(),
            ),
            Err(e) => (
                Self {
                    boot_error: Some(e.to_string()),
                    ..base
                },
                Task::none(),
            ),
        }
    }

    /// 100ms 轮询：后台线程的事件 → update（流式渲染 v2 前够用）。
    pub fn subscription(&self) -> Subscription<Message> {
        if self.state.is_some() {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// 把一条 UiEvent 应用到 UI 状态（渲染缓存 + 侧边栏树）。
    fn apply_event(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::Status {
                runtime_id,
                status,
                name,
                workspace,
            } => {
                // 首次到达（Connecting）时创建侧边栏节点；之后只更新状态。
                if let Some(rt) = self.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.status = status;
                } else {
                    self.runtimes.push(RtView {
                        id: runtime_id.clone(),
                        name,
                        workspace,
                        status,
                        sessions: Vec::new(),
                    });
                }
                // 第一个 runtime Ready 后自动开会话，方便直接发消息。
                if status == UiStatus::Ready && self.selected.is_none() {
                    if let Some(rt) = self.runtimes.iter().find(|r| r.id == runtime_id) {
                        if let Some(state) = &self.state {
                            state.send_command(Command::NewSession {
                                runtime_id: rt.id.clone(),
                            });
                        }
                    }
                }
            }
            UiEvent::SessionCreated {
                runtime_id,
                session_id,
            } => {
                if let Some(rt) = self.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.sessions.push(SessionView {
                        id: session_id.clone(),
                        title: "新会话".to_string(),
                    });
                }
                if self.selected.is_none() {
                    self.selected = Some(session_id);
                    self.messages.clear();
                }
            }
            UiEvent::Title {
                session_id, title, ..
            } => {
                for rt in &mut self.runtimes {
                    if let Some(s) = rt.sessions.iter_mut().find(|s| s.id == session_id) {
                        s.title = title.clone();
                    }
                }
            }
            UiEvent::Message {
                session_id, msg, ..
            } => {
                if self.selected.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                match msg.role {
                    dshr_protocol::session_event::message::MessageRole::User => {
                        self.messages.push(MsgView::User(msg.text));
                    }
                    dshr_protocol::session_event::message::MessageRole::Assistant => {
                        self.messages.push(MsgView::Assistant {
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
                if self.selected.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                let icon = if tool.is_error { "⛔" } else { "🔧" };
                let dur = tool
                    .duration_ms
                    .map(|d| format!(" ({d}ms)"))
                    .unwrap_or_default();
                self.messages
                    .push(MsgView::Tool(format!("{icon} {}{dur}", tool.name)));
                // 工具结果文本放下一行（简单版：直接追加）。
                if let Some(result) = tool.result {
                    self.messages.push(MsgView::Tool(result));
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
                self.messages.push(MsgView::TurnEnd(format!(
                    "—— 回合结束（{reason}）{usage_str} ——"
                )));
            }
            UiEvent::Log { message, .. } => {
                // 调试期：stderr 直接显示在聊天区（浅灰小字），方便定位 runtime 报错。
                self.messages.push(MsgView::Info(format!("⚙ {message}")));
            }
            UiEvent::Error(e) => self.messages.push(MsgView::Info(format!("⚠ {e}"))),
        }
    }

    /// iced update：接 Message → 更新状态 / 发命令 / 收事件。
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                // 先收完再应用，避免 self 双重可变借用。
                let mut events = Vec::new();
                if let Some(state) = &mut self.state {
                    while let Some(ev) = state.try_recv() {
                        events.push(ev);
                    }
                }
                for ev in events {
                    self.apply_event(ev);
                }
                Task::none()
            }
            Message::AddPressed => {
                self.add_mode = true;
                self.name_input.clear();
                self.path_input.clear();
                Task::none()
            }
            Message::CancelAdd => {
                self.add_mode = false;
                Task::none()
            }
            Message::NameChanged(v) => {
                self.name_input = v;
                Task::none()
            }
            Message::PathChanged(v) => {
                self.path_input = v;
                Task::none()
            }
            Message::ConfirmAdd => {
                let name = self.name_input.trim().to_string();
                let cwd = self.path_input.trim().to_string();
                self.add_mode = false;
                if !name.is_empty() && !cwd.is_empty() {
                    if let Some(state) = &self.state {
                        state.send_command(Command::Start { name, cwd });
                    }
                }
                Task::none()
            }
            Message::NewSession(runtime_id) => {
                if let Some(state) = &self.state {
                    state.send_command(Command::NewSession { runtime_id });
                }
                Task::none()
            }
            Message::SelectSession(session_id) => {
                self.selected = Some(session_id);
                self.messages.clear(); // 简单版：切换会话只显示之后的新消息
                Task::none()
            }
            Message::InputChanged(v) => {
                self.input = v;
                Task::none()
            }
            Message::SendPressed => {
                let text = self.input.trim().to_string();
                self.input.clear();
                if !text.is_empty() {
                    if let Some(session_id) = &self.selected {
                        if let Some(state) = &self.state {
                            state.send_command(Command::Send {
                                session_id: session_id.clone(),
                                text,
                            });
                        }
                    }
                }
                Task::none()
            }
            Message::ArchiveRuntime(runtime_id) => {
                if let Some(state) = &self.state {
                    state.send_command(Command::ArchiveRuntime { runtime_id });
                }
                Task::none()
            }
            Message::Close => {
                if let Some(state) = &self.state {
                    state.send_command(Command::Shutdown);
                }
                // 给后台 shutdown（bridge.shutdown 是异步的）留收尾时间，避免 node 残留。
                std::thread::sleep(Duration::from_millis(800));
                std::process::exit(0);
            }
        }
    }

    /// 渲染（薄委托，实现见 view.rs）。
    pub fn view(&self) -> iced::Element<'_, Message> {
        crate::view::view(self)
    }
}
