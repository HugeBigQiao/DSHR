//! 根组件：App 状态机 + 根视图分发。
//! 布局（Zed 风格）：顶栏（页面标签 + 窗口控制，兼作无边框窗框）→ 正文 → 底部图标栏。
use iced::widget::text_editor;
use iced::{Element, Length, Subscription, Task, Theme};

use crate::bridge::PlaceholderBridge;
use crate::model::{AppData, MsgView, RuntimeView, SessionView};
use crate::monitor;
use crate::nav;
use crate::setting::SettingPane;
use crate::statusbar;
use crate::task;
use crate::theme;

/// 三页（任务 / 监控 / 配置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Task,
    Monitor,
    Setting,
}

/// 窗口控制命令（Zed 顶栏右侧 + 拖动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCmd {
    Minimize,
    Maximize,
    Close,
    /// 拖动无边框窗口（顶栏空白处按下）。
    Drag,
}

/// 根消息：分发到各页 + 窗口/布局控制。
#[derive(Debug, Clone)]
pub enum Message {
    Nav(Page),
    Task(crate::task::Message),
    Setting(crate::setting::Message),
    Window(WindowCmd),
    /// 主窗口 id（订阅 window::open_events 捕获，窗口控制用）。
    WindowId(iced::window::Id),
    /// 底部图标栏：收起/展开侧边栏。
    ToggleSidebar,
}

/// 根状态。
#[derive(Debug)]
pub struct App {
    pub page: Page,
    /// 深/浅色（配置页「切换主题」）。
    pub dark: bool,
    /// 全局字号基准（默认 14）。
    pub font_size: u16,
    /// 数据快照（bridge 提供；state 接入后由真实桥更新）。
    pub data: AppData,
    /// 侧边栏收起（底部图标栏切换）。
    pub sidebar_collapsed: bool,
    /// 当前展开的 ⋯ 菜单：(runtime_id, session_id?)。
    pub menu: Option<(String, Option<String>)>,
    /// 侧边栏当前 hover 行：(runtime_id, session_id?)；悬停显示 ⋯/+ 与行背景。
    pub hover: Option<(String, Option<String>)>,
    /// composer 草稿（多行编辑器，自动扩展高度）。
    pub composer: text_editor::Content,
    /// 主窗口 id（订阅捕获）。
    window_id: Option<iced::window::Id>,
    /// TODO(bridge)：state 冻结期间用占位桥；接 state 后换真实实现（届时本字段被读取）。
    #[allow(dead_code)]
    bridge: PlaceholderBridge,
    /// 配置页状态。
    setting: SettingPane,
}

impl App {
    pub fn new() -> Self {
        let bridge = PlaceholderBridge::new();
        let data = bridge.snapshot();
        Self {
            page: Page::Task,
            dark: true,
            font_size: 14,
            data,
            sidebar_collapsed: false,
            menu: None,
            hover: None,
            composer: text_editor::Content::new(),
            window_id: None,
            bridge,
            setting: SettingPane::new(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Nav(page) => self.page = page,
            Message::Task(msg) => self.handle_task(msg),
            Message::Setting(msg) => self.handle_setting(msg),
            Message::Window(cmd) => return self.window_command(cmd),
            Message::WindowId(id) => self.window_id = Some(id),
            Message::ToggleSidebar => self.sidebar_collapsed = !self.sidebar_collapsed,
        }
        Task::none()
    }

    /// 窗口控制（Zed 顶栏按钮 → iced window actions；无边框窗口拖动）。
    fn window_command(&self, cmd: WindowCmd) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };
        match cmd {
            WindowCmd::Minimize => iced::window::minimize(id, true),
            WindowCmd::Maximize => iced::window::maximize(id, true),
            WindowCmd::Close => iced::window::close(id),
            WindowCmd::Drag => iced::window::drag(id),
        }
    }

    fn handle_task(&mut self, msg: crate::task::Message) {
        match msg {
            crate::task::Message::ComposerEdit(action) => self.composer.perform(action),
            crate::task::Message::Send => self.send_draft(),
            crate::task::Message::ToggleTool(index) => {
                if let Some(MsgView {
                    tool: Some(tool), ..
                }) = self.data.chat.messages.get_mut(index)
                {
                    tool.expanded = !tool.expanded;
                }
            }
            crate::task::Message::Hover(hover) => self.hover = hover,
            crate::task::Message::NewRuntime => {
                let id = format!("rt-{}", self.data.runtimes.len() + 1);
                self.data.runtimes.push(RuntimeView {
                    id: id.clone(),
                    name: format!("新 runtime {}", self.data.runtimes.len() + 1),
                    expanded: true,
                    sessions: vec![SessionView {
                        id: format!("{id}-s1"),
                        title: "新会话".to_string(),
                    }],
                    selected_session: Some(format!("{id}-s1")),
                });
                self.menu = None;
            }
            crate::task::Message::ToggleRuntimeExpand(runtime_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.expanded = !rt.expanded;
                }
            }
            crate::task::Message::NewSession(runtime_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    let sid = format!("{}-s{}", runtime_id, rt.sessions.len() + 1);
                    rt.sessions.push(SessionView {
                        id: sid.clone(),
                        title: format!("会话 {}", rt.sessions.len() + 1),
                    });
                    rt.selected_session = Some(sid);
                }
                self.menu = None;
            }
            crate::task::Message::DeleteRuntime(id) => {
                self.data.runtimes.retain(|r| r.id != id);
                self.menu = None;
            }
            crate::task::Message::ArchiveRuntime(id) => {
                // 归档 = 移除（骨架阶段；真实实现 = 标记 archived + 保留数据可查）。
                self.data.runtimes.retain(|r| r.id != id);
                self.menu = None;
            }
            crate::task::Message::DeleteSession(runtime_id, session_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.sessions.retain(|s| s.id != session_id);
                    if rt.selected_session.as_deref() == Some(session_id.as_str()) {
                        rt.selected_session = rt.sessions.first().map(|s| s.id.clone());
                    }
                }
                self.menu = None;
            }
            crate::task::Message::ArchiveSession(runtime_id, session_id) => {
                // 归档 = 移除（骨架阶段）。
                self.handle_task(crate::task::Message::DeleteSession(runtime_id, session_id));
            }
            crate::task::Message::MenuToggle(runtime_id, session_id) => {
                self.menu = if self.menu == Some((runtime_id.clone(), session_id.clone())) {
                    None
                } else {
                    Some((runtime_id, session_id))
                };
            }
            crate::task::Message::SelectSession(runtime_id, session_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.selected_session = Some(session_id.clone());
                }
                self.data.chat.session_id = session_id;
            }
        }
    }

    /// 发送草稿（TODO(bridge)：交给 state → SDK；占位桥本地回显）。
    fn send_draft(&mut self) {
        use crate::model::MsgKind;
        let text = self.composer.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer = text_editor::Content::default();
        self.data.chat.messages.push(MsgView {
            kind: MsgKind::User,
            text,
            reasoning: None,
            tool: None,
            time_label: "刚刚".to_string(),
        });
        self.data.chat.messages.push(MsgView {
            kind: MsgKind::Assistant,
            text: "（占位回复：接 dshr-state 后这里会显示真实模型输出）".to_string(),
            reasoning: None,
            tool: None,
            time_label: "刚刚".to_string(),
        });
    }

    fn handle_setting(&mut self, msg: crate::setting::Message) {
        match msg {
            crate::setting::Message::ThemeToggle => self.dark = !self.dark,
            _ => {}
        }
        self.setting.handle(msg);
    }

    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let p = theme::Palette::pick(self.dark);
        let body = match self.page {
            Page::Task => task::view(self).map(Message::Task),
            Page::Monitor => monitor::view(self),
            Page::Setting => self.setting.view(self).map(Message::Setting),
        };
        // 底部图标栏只属于任务页（监控/配置不需要）。
        let frame = if self.page == Page::Task {
            iced::widget::column![nav::nav(self), body, statusbar::view(self)]
        } else {
            iced::widget::column![nav::nav(self), body]
        };
        iced::widget::container(frame)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::surface(p, p.bg_base, 0.0))
            .into()
    }

    /// 当前主题（跟随 dark 开关，供 iced 内置控件默认样式）。
    pub fn theme(&self) -> Theme {
        if self.dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    /// 当前设计系统调色板。
    pub fn palette(&self) -> theme::Palette {
        theme::Palette::pick(self.dark)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // 捕获主窗口 id（窗口控制按钮用）；TODO(bridge)：接 state 事件流后再加一路。
        iced::window::open_events().map(Message::WindowId)
    }

    /// 全局字号（f32；iced 0.14 size 接受 `Into<Pixels>`）。
    pub fn fs(&self, base: u16) -> f32 {
        (self.font_size as f32 * base as f32 / 14.0).max(8.0)
    }
}
