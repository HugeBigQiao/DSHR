//! 根组件：App 状态机 + 根视图分发。
//! 布局（Zed 风格）：顶栏（页面标签 + 窗口控制，兼作无边框窗框）→ 正文 → 底部图标栏。
//!
//! s3 真桥接线（DESIGN §11.4 / M3.6）：App 持有总线命令通道发送端（bridge Ready 事件
//! 交付），按钮/发送动作 → BridgeCmd 走命令通道 → dshr_state::engine 常驻会话中台驱动
//! runtime；engine 每通知折叠（并落库）后整发 Snapshot → 本文件把快照刷进 model 视图
//! 模型。UI 只搬运命令/事件，不直接 import SDK/协议类型（engine 事件经 BridgeEvent::Engine
//! 嵌套透传）。
use std::collections::HashSet;

use iced::widget::text_editor;
use iced::{Element, Length, Subscription, Task, Theme};
use tokio::sync::mpsc;

use crate::bridge::{BridgeCmd, BridgeEvent, EngineEvent};
use crate::model::{AppData, ChatState, ChatStatus, RuntimeView, SessionView, session_title};
use crate::{bridge, monitor, nav, setting, statusbar, task, theme};

/// 单 runtime 槽位 id（s3 收敛为单 runtime；多 runtime 树留后续）。
const RT_ID: &str = "rt-1";

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

/// 根消息：分发到各页 + 窗口/布局控制 + bridge 事件。
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
    /// bridge 事件（engine → UI：启动结果/快照/停止/错误）。
    Bridge(BridgeEvent),
}

/// 根状态。
#[derive(Debug)]
pub struct App {
    pub page: Page,
    /// 深/浅色（配置页「切换主题」）。
    pub dark: bool,
    /// 全局字号基准（默认 14）。
    pub font_size: u16,
    /// 数据视图（bridge Snapshot 事件刷新；聊天区/统计/侧边栏都读它）。
    pub data: AppData,
    /// 侧边栏收起（底部图标栏切换）。
    pub sidebar_collapsed: bool,
    /// 当前展开的 ⋯ 菜单：(runtime_id, session_id?)。
    pub menu: Option<(String, Option<String>)>,
    /// 侧边栏当前 hover 行：(runtime_id, session_id?)；悬停显示 ⋯/+ 与行背景。
    pub hover: Option<(String, Option<String>)>,
    /// composer 草稿（多行编辑器，自动扩展高度）。
    pub composer: text_editor::Content,
    /// 工具卡展开集合（按消息 seq 索引：快照整体刷新时索引仍稳定）。
    pub expanded_tools: HashSet<u64>,
    /// bridge 命令通道发送端（Ready 事件交付；None = 总线未就绪）。
    cmd_tx: Option<mpsc::Sender<BridgeCmd>>,
    /// Ready 到达前点过「新建 runtime」→ 就绪后补发 Start。
    pending_start: bool,
    /// 主窗口 id（订阅捕获）。
    window_id: Option<iced::window::Id>,
    /// 配置页状态。
    setting: setting::SettingPane,
}

impl App {
    pub fn new() -> Self {
        Self {
            page: Page::Task,
            dark: true,
            font_size: 14,
            data: AppData::default(),
            sidebar_collapsed: false,
            menu: None,
            hover: None,
            composer: text_editor::Content::new(),
            expanded_tools: HashSet::new(),
            cmd_tx: None,
            pending_start: false,
            window_id: None,
            setting: setting::SettingPane::new(),
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
            Message::Bridge(ev) => self.handle_bridge(ev),
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

    // —— bridge 命令发送（非阻塞：tokio mpsc try_send；满则丢，UI 低频不会满）——

    fn send_cmd(&mut self, cmd: BridgeCmd) {
        if let Some(tx) = &mut self.cmd_tx {
            let _ = tx.try_send(cmd);
        }
    }

    // —— bridge 事件处理（engine → UI）——

    fn handle_bridge(&mut self, ev: BridgeEvent) {
        match ev {
            BridgeEvent::Ready(tx) => {
                self.cmd_tx = Some(tx);
                // Ready 前点过「新建 runtime」→ 补发（总线就绪竞态）。
                if self.pending_start {
                    self.pending_start = false;
                    self.send_cmd(BridgeCmd::Start);
                }
            }
            BridgeEvent::Engine(EngineEvent::Started { label, session_id }) => {
                // 新 runtime/新会话：侧边栏单槽 + 清空上一轮数据（engine 的 Folder 已重置，
                // 紧随其后的空快照 Snapshot 再次刷新聊天区；此处先立会话行）。
                self.expanded_tools.clear();
                self.data.chat = ChatState {
                    session_id: session_id.clone(),
                    status: ChatStatus::Idle,
                    status_line: "runtime 已启动（Fake 会随 prompt 回显事件）".to_string(),
                    ..ChatState::new()
                };
                self.data.runtimes = vec![RuntimeView {
                    id: RT_ID.to_string(),
                    name: label,
                    expanded: true,
                    sessions: vec![SessionView {
                        id: session_id.clone(),
                        title: session_title(&None, &session_id),
                    }],
                    selected_session: Some(session_id),
                }];
            }
            BridgeEvent::Engine(EngineEvent::Snapshot(snap)) => {
                let sid_changed =
                    !snap.session_id.is_empty() && snap.session_id != self.data.chat.session_id;
                if sid_changed {
                    // 换会话（Start/ResetSession）：展开态按新 seq 重新算。
                    self.expanded_tools.clear();
                }
                self.data.chat.apply_snapshot(&snap);
                self.sync_session_row();
            }
            BridgeEvent::Engine(EngineEvent::Stopped { reason }) => {
                self.data.chat.status = ChatStatus::Stopped;
                self.data.chat.status_line = if reason.is_empty() {
                    "runtime 已停止（历史消息保留）".to_string()
                } else {
                    reason
                };
            }
            BridgeEvent::Engine(EngineEvent::Failed { reason }) => {
                self.data.chat.status = ChatStatus::Failed;
                self.data.chat.status_line = reason;
            }
        }
    }

    /// 侧边栏会话行与 chat 对齐：标题（session/title 定题，否则"会话 <短id>"）+ id。
    fn sync_session_row(&mut self) {
        let Some(rt) = self.data.runtimes.first_mut() else {
            return;
        };
        let title = session_title(&self.data.chat.title, &self.data.chat.session_id);
        if rt.sessions.is_empty() {
            rt.sessions.push(SessionView {
                id: self.data.chat.session_id.clone(),
                title,
            });
        } else {
            rt.sessions[0].id = self.data.chat.session_id.clone();
            rt.sessions[0].title = title;
        }
        rt.selected_session = Some(rt.sessions[0].id.clone());
        self.data.chat.session_id = rt.sessions[0].id.clone();
    }

    // —— 任务页动作（按钮 → 真实语义）——

    fn handle_task(&mut self, msg: crate::task::Message) {
        match msg {
            crate::task::Message::ComposerEdit(action) => self.composer.perform(action),
            crate::task::Message::Send => self.send_draft(),
            crate::task::Message::ToggleTool(seq) => {
                if !self.expanded_tools.remove(&seq) {
                    self.expanded_tools.insert(seq);
                }
            }
            crate::task::Message::Hover(hover) => self.hover = hover,
            crate::task::Message::MenuToggle(runtime_id, session_id) => {
                self.menu = if self.menu == Some((runtime_id.clone(), session_id.clone())) {
                    None
                } else {
                    Some((runtime_id, session_id))
                };
            }
            crate::task::Message::NewRuntime => {
                // = 启动 runtime（Fake/Real 判定在 dshr_state::engine；已运行则 engine 忽略）。
                if self.cmd_tx.is_some() {
                    self.send_cmd(BridgeCmd::Start);
                } else {
                    self.pending_start = true; // 总线未就绪：Ready 后补发。
                }
                self.menu = None;
            }
            crate::task::Message::ToggleRuntimeExpand(runtime_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.expanded = !rt.expanded;
                }
            }
            crate::task::Message::NewSession(runtime_id) => {
                // = 重置当前会话（新 session id；多会话管理留后续）。
                let _ = runtime_id;
                self.send_cmd(BridgeCmd::ResetSession);
                self.menu = None;
            }
            crate::task::Message::DeleteRuntime(id) => {
                // = 停止 runtime（进程 shutdown；数据删除 = store/会话树接入后的事）。
                let _ = id;
                self.send_cmd(BridgeCmd::Stop);
                self.menu = None;
            }
            crate::task::Message::ArchiveRuntime(id) => {
                // 同 DeleteRuntime（归档 = 停止；保留数据可查待 store 接线）。
                self.handle_task(crate::task::Message::DeleteRuntime(id));
            }
            crate::task::Message::DeleteSession(runtime_id, session_id) => {
                // = 重置当前会话（单会话收敛：删除即清当前，Folder 归档留后续）。
                let _ = (runtime_id, session_id);
                self.send_cmd(BridgeCmd::ResetSession);
                self.menu = None;
            }
            crate::task::Message::ArchiveSession(runtime_id, session_id) => {
                self.handle_task(crate::task::Message::DeleteSession(runtime_id, session_id));
            }
            crate::task::Message::SelectSession(runtime_id, session_id) => {
                if let Some(rt) = self.data.runtimes.iter_mut().find(|r| r.id == runtime_id) {
                    rt.selected_session = Some(session_id.clone());
                }
                self.data.chat.session_id = session_id;
            }
        }
    }

    /// 发送 composer 草稿 → 命令通道 Prompt（真管线：session/prompt → 事件 → 快照）。
    /// 只在会话 idle 时允许（running 期间 worker 按序处理；未启动/已停时保留草稿，
    /// 状态行已提示）。
    fn send_draft(&mut self) {
        let text = self.composer.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.data.chat.status != ChatStatus::Idle {
            return;
        }
        self.composer = text_editor::Content::default();
        self.send_cmd(BridgeCmd::Prompt { text });
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
        if self.dark { Theme::Dark } else { Theme::Light }
    }

    /// 当前设计系统调色板。
    pub fn palette(&self) -> theme::Palette {
        theme::Palette::pick(self.dark)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // 主窗口 id（窗口控制）+ bridge 常驻总线（runtime 事件/命令通道）。
        Subscription::batch([
            iced::window::open_events().map(Message::WindowId),
            bridge::subscribe().map(Message::Bridge),
        ])
    }

    /// 全局字号（f32；iced 0.14 size 接受 `Into<Pixels>`）。
    pub fn fs(&self, base: u16) -> f32 {
        (self.font_size as f32 * base as f32 / 14.0).max(8.0)
    }
}
