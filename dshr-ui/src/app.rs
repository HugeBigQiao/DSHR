//! 根组件：App 状态机 + 根视图分发。
//! 布局（Zed 风格）：顶栏（页面标签 + 窗口控制，兼作无边框窗框）→ 正文 → 底部图标栏。
//!
//! s3 真桥接线（分层见 dshr/DESIGN.md §3）：App 持有总线命令通道发送端
//! （bridge Ready 事件交付），按钮/发送动作 → BridgeCmd 走命令通道 → dshr_state 的
//! engine/raw 驱动 runtime；每次通知折叠（并落库）后整发 Snapshot → 本文件把快照刷进
//! model 视图模型。UI 只搬运命令/事件，不直接 import SDK/协议类型（engine 事件经
//! BridgeEvent::Engine 嵌套透传）。
//!
//! 主要用途：持有全部跨页面的 UI 状态（当前页、主题/字号、bridge 命令发送端、窗口 id、
//! 菜单/hover/工具卡展开等交互态），并把四类消息（导航、各页消息、窗口命令、bridge 事件）
//! 统一分发到对应处理函数；`view` 只按 `page` 选一个页面 body，再拼顶栏 + 底部图标栏。
//! 为什么需要：这是 iced 的单一可变状态点——各页面模块（task/files/setting）都是无状态的
//! `view(&App)`，共享态（主题、字体基准、菜单、订阅、命令通道）必须集中在一处，否则每个
//! 页面都要自持一份副本并互相同步。它同时是 UI 与 state 层唯一的接缝：只有这里认识
//! `BridgeEvent`/`BridgeCmd`，其余页面只生产 `Message`。
//! 上接：`main.rs`（`iced::application(App::new, App::update, App::view)` + `App::subscription`）。
//! 下接：`bridge`（bridge::subscribe/BridgeCmd/BridgeEvent）、`model`（AppData/ChatState…）、
//! `task`/`files`/`setting`/`monitor`/`nav`/`statusbar`/`theme`。
//! 官方对应：`packages/client/ui-layout/src/client/AppFrame.tsx` 的 `AppFrame`
//! （三栏 root 槽 + 列宽解算）；页面选择相当于官方 `ui-sidebar` 的 panel 导航
//! （`SidebarRoot` 的 `selectPanel`）。
use std::collections::HashSet;

use iced::widget::text_editor;
use iced::{Element, Length, Subscription, Task, Theme};
use tokio::sync::mpsc;

use crate::bridge::{BridgeCmd, BridgeEvent, EngineEvent, RuntimeId, SessionId};
use crate::model::{AppData, ChatState, ChatStatus, RuntimeView, SessionView, session_title};
use crate::{bridge, monitor, nav, setting, statusbar, task, theme};

/// 单 runtime 槽位 id（s3 收敛为单 runtime；多 runtime 树留后续）。
///
/// 为什么需要：`RuntimeView` 需要稳定 id 才能与侧边栏菜单/hover/选中态比对；当前只有一个
/// runtime，用常量即可（多 runtime 时改为注册表分配）。
const RT_ID: &str = "rt-1";

/// 四个主页面（任务 / 文件 / 监控 / 配置）。
///
/// 为什么需要：`App::view` 与 `nav.rs` 的标签都用它做唯一判据（比较用 `PartialEq`），
/// 避免用字符串比较页面名（页面名属于 UI 文案，可改；枚举判别不会跟着变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 任务页：侧边栏会话树 + 对话区 + 右侧预留（task.rs）。
    Task,
    /// 文件页：工作区文件树 + 代码编辑器（files.rs）。
    Files,
    /// 监控页：数据看板占位（monitor.rs，M3.8 填充）。
    Monitor,
    /// 配置页：分区导航 + 分组表单（setting.rs）。
    Setting,
}

/// 窗口控制命令（Zed 顶栏右侧 + 拖动）。
///
/// 为什么需要：无边框窗口（`decorations: false`）没有系统标题栏，最小化/最大化/关闭/拖动
/// 只能由顶栏按钮发出；把四种动作收敛成一个枚举，`update` 里一处翻译成 iced window action，
/// `nav.rs` 只管发命令、不认识 `iced::window`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCmd {
    /// 最小化主窗口（顶栏左起第一个字形按钮）。
    Minimize,
    /// 最大化/还原主窗口。
    Maximize,
    /// 关闭主窗口（等于退出应用）。
    Close,
    /// 拖动无边框窗口（顶栏空白处按下）。
    Drag,
}

/// 根消息：分发到各页 + 窗口/布局控制 + bridge 事件。
///
/// 为什么需要：iced 的 `update` 只接受一个消息类型，四类来源（页面导航、子页面交互、
/// 窗口控制、异步 bridge 事件）必须在这里合并；子页面消息经 `Task`/`Files`/`Setting`
/// 变体嵌套转发，保证子页面模块不必知道彼此。
#[derive(Debug, Clone)]
pub enum Message {
    /// 顶栏标签点击 → 切到目标页；切到文件页时若树为空会顺带触发一次 `Refresh`。
    Nav(Page),
    /// 任务页交互（会话树/对话区/composer），转发给 `handle_task`。
    Task(crate::task::Message),
    /// 文件页交互，转发给 `FilesState::handle`。
    Files(crate::files::Message),
    /// 配置页交互（表单输入/保存），转发给 `SettingPane::handle`。
    Setting(crate::setting::Message),
    /// 顶栏窗口控制按钮/拖动区。
    Window(WindowCmd),
    /// 主窗口 id（订阅 window::open_events 捕获，窗口控制用）。
    WindowId(iced::window::Id),
    /// 底部图标栏：收起/展开侧边栏。
    ToggleSidebar,
    /// bridge 事件（engine → UI：启动结果/快照/停止/错误）。
    Bridge(BridgeEvent),
}

/// 根状态。
///
/// 为什么需要：见文件头「为什么需要」——这是全程唯一的可变 UI 状态；页面模块只读它。
pub struct App {
    pub page: Page,
    /// 深/浅色（配置页「切换主题」）。
    pub dark: bool,
    /// 全局字号基准（默认 14）。
    pub font_size: u16,
    /// 数据视图（bridge Snapshot 事件刷新；聊天区/统计/侧边栏都读它）。
    pub data: AppData,
    /// Files page state.
    pub files: crate::files::FilesState,
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
    /// 当前 runtime 的 id（UI 目前是单 runtime 视图；engine 已支持多 runtime，
    /// 侧边栏多条目是后续里程碑）。`None` = 尚未启动过任何 runtime。
    ///
    /// 为什么需要：新 engine 的命令与事件都带 `RuntimeId`（多 runtime 的路由依据），
    /// 而 UI 侧仍是单槽展示——所以这里保存"当前那一个"的 id，发命令时带上它。
    /// 每次点「新建 runtime」都会生成新 id（与 engine 的槽位一一对应，不复用）。
    runtime_id: Option<RuntimeId>,
    /// Ready 到达前点过「新建 runtime」→ 就绪后补发 Start。
    pending_start: bool,
    /// 主窗口 id（订阅捕获）。
    window_id: Option<iced::window::Id>,
    /// 配置页状态。
    setting: setting::SettingPane,
}

impl App {
    /// 构造初始状态（默认任务页、深色、字号 14、未启动 runtime）。
    ///
    /// 为什么需要：iced `application(App::new, ..)` 要求一个无参构造函数；同时它承担一处
    /// 启动期提示——配置页加载完发现 API key 为空时，把回退 Fake 的说明写进 `status_line`，
    /// 让用户不必先点开配置页才知道原因。
    /// 入参/出参：无；返回装配好的 `App`（`cmd_tx`/`window_id` 为空，等订阅就绪再填）。
    pub fn new() -> Self {
        let mut app = Self {
            page: Page::Task,
            dark: true,
            font_size: 14,
            data: AppData::default(),
            files: crate::files::FilesState::new(true),
            sidebar_collapsed: false,
            menu: None,
            hover: None,
            composer: text_editor::Content::new(),
            expanded_tools: HashSet::new(),
            cmd_tx: None,
            pending_start: false,
            runtime_id: None,
            window_id: None,
            setting: setting::SettingPane::new(),
        };
        if app.setting.api_key_missing() {
            app.data.chat.status_line =
                "API key 未配置：Real 模式会回退 Fake；请到配置页填写 API key。".to_string();
        }
        app
    }

    /// iced 的消息入口：把 `Message` 翻译成状态变更（+ 可选的异步 `Task`）。
    ///
    /// 为什么需要：UI 的一切交互（点击、输入、引擎事件）都从这里进来；把分发集中在一处，
    /// 各页面模块才能保持无状态并把「自己的消息」定义在自己文件里。
    /// 入参/出参：`message` 为一条根消息；返回 iced `Task<Message>`（多数分支 `Task::none()`，
    /// 只有文件页的目录/文件扫描与初次 `Refresh` 会返回真正的异步任务）。
    /// 错误条件：无（错误以 `status_line`/`status` 文本形式体现在状态里，不从这里返回）。
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Nav(page) => {
                self.page = page;
                if page == Page::Files && self.files.children.is_empty() && !self.files.loading {
                    return self
                        .files
                        .handle(crate::files::Message::Refresh)
                        .map(Message::Files);
                }
            }
            Message::Task(msg) => self.handle_task(msg),
            Message::Files(msg) => return self.files.handle(msg).map(Message::Files),
            Message::Setting(msg) => self.handle_setting(msg),
            Message::Window(cmd) => return self.window_command(cmd),
            Message::WindowId(id) => {
                self.window_id = Some(id);
                // 诊断开关：DSHR_AUTO_OPEN=<工作区相对路径> 时启动即打开该文件并切到文件页。
                // 用于在不便用鼠标交互时验证「打开文件」的耗时/内存/卡顿。
                if let Ok(path) = std::env::var("DSHR_AUTO_OPEN")
                    && !path.is_empty()
                {
                    self.page = Page::Files;
                    return self
                        .files
                        .handle(crate::files::Message::SelectFile(path))
                        .map(Message::Files);
                }
            }
            Message::ToggleSidebar => self.sidebar_collapsed = !self.sidebar_collapsed,
            Message::Bridge(ev) => self.handle_bridge(ev),
        }
        Task::none()
    }

    /// 窗口控制（Zed 顶栏按钮 → iced window actions；无边框窗口拖动）。
    ///
    /// 为什么需要：无边框窗口没有任何系统窗框，这四种动作只能由顶栏发出；且 iced 的
    /// window action 都需要 `window::Id`，而 id 要到 `window::open_events()` 订阅送达才知道
    /// （无 MAIN 常量），所以翻译动作这一步必须能容忍「id 还没到」。
    /// 入参/出参：`cmd` 为目标动作；返回对应 iced 任务。窗口 id 未知时返回 `Task::none()`
    /// （早于首帧的点击被静默丢弃，不会 panic）。
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

    /// 把一条命令投进 bridge 命令通道。
    ///
    /// 为什么需要：`update` 是同步的，不能 await；而命令要跨到 engine 的异步世界。用
    /// `try_send` 保证 UI 永不因引擎慢而卡住（代价：通道满时静默丢一条，UI 交互频率远低于容量）。
    /// 入参/出参：`cmd` 为待投递命令；无返回值。总线未就绪（`cmd_tx` 为 None）时整体忽略——
    /// 需要「就绪后补发」语义的调用方（新建 runtime）须自行先记 `pending_start`。
    fn send_cmd(&mut self, cmd: BridgeCmd) {
        if let Some(tx) = &mut self.cmd_tx {
            let _ = tx.try_send(cmd);
        }
    }

    // —— bridge 事件处理（engine → UI）——

    /// 消费一条 bridge 事件（总线就绪 / engine 四个生命周期事件）。
    ///
    /// 为什么需要：engine 与总线是唯一的外部异步输入，UI 状态只能在这里被外部改写；把
    /// 「engine 事件语义 → UI 状态」的映射集中在一个 match 里，页面渲染代码就不必理解事件。
    /// 入参/出参：`ev` 为 `Ready` 或 `Engine(..)`；无返回值。
    /// 主要功能：`Ready` 记住命令通道并在有 pending 启动请求时补发 `Start`；
    /// `Started` 立出单 runtime/单会话行并清空展开态；`Snapshot` 换会话时清展开态后整刷
    /// `data.chat` 并同步侧边栏行；`Stopped`/`Failed` 只改状态与状态行（历史消息保留）。
    fn handle_bridge(&mut self, ev: BridgeEvent) {
        match ev {
            BridgeEvent::Ready(tx) => {
                self.cmd_tx = Some(tx);
                // Ready 前点过「新建 runtime」→ 补发（总线就绪竞态）。
                if self.pending_start {
                    self.pending_start = false;
                    self.start_runtime();
                }
            }
            BridgeEvent::Engine(EngineEvent::Started { id, label, session }) => {
                // 新 runtime/新会话：侧边栏单槽 + 清空上一轮数据（engine 的会话态已重置，
                // 紧随其后的空快照 Snapshot 再次刷新聊天区；此处先立会话行）。
                self.expanded_tools.clear();
                self.runtime_id = Some(id);
                let sid = session.as_str().to_string();
                self.data.chat = ChatState {
                    session_id: sid.clone(),
                    status: ChatStatus::Idle,
                    status_line: "runtime 已启动（Fake 会随 prompt 回显事件）".to_string(),
                    ..ChatState::new()
                };
                self.data.runtimes = vec![RuntimeView {
                    id: RT_ID.to_string(),
                    name: label,
                    expanded: true,
                    sessions: vec![SessionView {
                        id: sid.clone(),
                        title: session_title(&None, &sid),
                    }],
                    selected_session: Some(sid),
                }];
            }
            BridgeEvent::Engine(EngineEvent::Snapshot { snapshot, .. }) => {
                let snap = *snapshot;
                let sid_changed =
                    !snap.session_id.is_empty() && snap.session_id != self.data.chat.session_id;
                if sid_changed {
                    // 换会话（Start/ResetSession）：展开态按新 seq 重新算。
                    self.expanded_tools.clear();
                }
                self.data.chat.apply_snapshot(&snap);
                self.sync_session_row();
            }
            BridgeEvent::Engine(EngineEvent::SessionReset { session, .. }) => {
                // 换会话：新 id 由 engine 生成并在此回报。侧边栏换行、展开态清空、
                // 聊天区状态复位——随后的空快照会再刷一次消息列表。
                self.expanded_tools.clear();
                let sid = session.as_str().to_string();
                self.data.chat = ChatState {
                    session_id: sid.clone(),
                    status: ChatStatus::Idle,
                    status_line: "新会话已建立".to_string(),
                    ..ChatState::new()
                };
                if let Some(rt) = self.data.runtimes.first_mut() {
                    rt.sessions = vec![SessionView {
                        id: sid.clone(),
                        title: session_title(&None, &sid),
                    }];
                    rt.selected_session = Some(sid);
                }
            }
            BridgeEvent::Engine(EngineEvent::Stopped { reason, .. }) => {
                self.data.chat.status = ChatStatus::Stopped;
                self.data.chat.status_line = if reason.is_empty() {
                    "runtime 已停止（历史消息保留）".to_string()
                } else {
                    reason
                };
            }
            BridgeEvent::Engine(EngineEvent::Failed { reason, .. }) => {
                self.data.chat.status = ChatStatus::Failed;
                self.data.chat.status_line = reason;
            }
        }
    }

    /// 发起「新建 runtime」：生成新 id 并让 engine 拉起进程。
    ///
    /// 为什么 id 在 UI 侧生成：用户点下按钮那一刻侧边栏就要出现一行（进程可能还要几秒才起来、
    /// 也可能起不来），所以 id 必须先有。engine 只负责把它落实到槽位。
    /// 命令通道未就绪时记 `pending_start`，等 `Ready` 补发（总线就绪竞态）。
    fn start_runtime(&mut self) {
        let id = RuntimeId::generate();
        self.runtime_id = Some(id.clone());
        self.send_cmd(BridgeCmd::StartRuntime { id });
    }

    /// 请求在当前 runtime 内换一个新会话（新 id 由 engine 生成，经 `SessionReset` 回报）。
    ///
    /// 为什么不在 UI 侧生成 id：id 规则（`s-<epoch>`）与唯一性约束属于 state 层
    ///（真实 dsh 按 id 落盘会话日志，重复会撞 `session already has a persisted log on disk`）。
    /// 未启动 runtime 时无操作（没有会话可换）。
    fn reset_current_session(&mut self) {
        if let Some(id) = self.runtime_id.clone() {
            self.send_cmd(BridgeCmd::ResetSession { id });
        }
    }

    /// 侧边栏会话行与 chat 对齐：标题（session/title 定题，否则"会话 <短id>"）+ id。
    ///
    /// 为什么需要：侧边栏的会话行是快照之外的冗余状态（s3 单会话收敛的产物），快照每次刷新
    /// 都会带来新的 session_id/标题；不在这里回写就会出现「聊天区换了会话、侧边栏还标着旧
    /// 标题」的不一致。
    /// 入参/出参：无；就地改写 `data.runtimes[0]` 的首个会话行与 `data.chat.session_id`。
    /// 主要功能：幂等同步——runtime 为空时直接返回；已有会话行时改它，没有则补一行。
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

    /// 任务页消息 → 真实语义（发送/重置会话/停止 runtime/树交互）。
    ///
    /// 为什么需要：这一层是「UI 文案动作」到「engine 命令」的翻译表。侧边栏不少菜单项在当前
    /// 单会话阶段还没有独立后端语义（删除/归档 runtime、删除/归档会话），只能映射到最近的
    /// 可用命令（Stop / ResetSession）；把所有映射集中在这里，后端补齐时只改这一处。
    /// 入参/出参：`msg` 为任务页消息；无返回值。
    /// 错误条件：无——无法映射到时保留草稿/不动状态（并在状态行给出提示）。
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
                // = 启动一个 runtime（Fake/Real 判定在 dshr-state 的 raw 层；已存在同 id 则忽略）。
                if self.cmd_tx.is_some() {
                    self.start_runtime();
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
                self.reset_current_session();
                self.menu = None;
            }
            crate::task::Message::DeleteRuntime(id) => {
                // = 停止 runtime（进程 shutdown；数据删除 = store/会话树接入后的事）。
                let _ = id;
                if let Some(rid) = self.runtime_id.clone() {
                    self.send_cmd(BridgeCmd::StopRuntime { id: rid });
                }
                self.menu = None;
            }
            crate::task::Message::ArchiveRuntime(id) => {
                // 同 DeleteRuntime（归档 = 停止；保留数据可查待 store 接线）。
                self.handle_task(crate::task::Message::DeleteRuntime(id));
            }
            crate::task::Message::DeleteSession(runtime_id, session_id) => {
                // = 重置当前会话（单会话收敛：删除即清当前，Folder 归档留后续）。
                let _ = (runtime_id, session_id);
                self.reset_current_session();
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
    ///
    /// 为什么需要：草稿清空与「什么时候允许发送」是交互契约的一部分（空文本不产生请求；
    /// 非 idle 不发送且不丢草稿），集中在这里避免每个触发点（Enter、↑ 按钮）各写一遍。
    /// 入参/出参：无（读 `self.composer`）；无返回值。副作用：允许发送时清空草稿并发 `Prompt`。
    fn send_draft(&mut self) {
        let text = self.composer.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.data.chat.status != ChatStatus::Idle {
            return;
        }
        self.composer = text_editor::Content::default();
        if let (Some(id), Some(session)) = (self.runtime_id.clone(), {
            let sid = self.data.chat.session_id.clone();
            if sid.is_empty() {
                None
            } else {
                Some(SessionId::new(sid))
            }
        }) {
            self.send_cmd(BridgeCmd::Prompt { id, session, text });
        }
    }

    /// 配置页消息转发：主题开关由 App 消费（其余留在 `SettingPane`）。
    ///
    /// 为什么需要：深浅色是全局的（顶栏/底部栏/文件页编辑器都要跟随），不能只存在配置页里；
    /// 所以 `ThemeToggle` 在这里被截获，同时把新主题推给文件页编辑器（它自带主题状态）。
    /// 入参/出参：`msg` 为配置页消息；无返回值。
    fn handle_setting(&mut self, msg: crate::setting::Message) {
        match msg {
            crate::setting::Message::ThemeToggle => self.dark = !self.dark,
            _ => {}
        }
        self.files.set_dark(self.dark);
        self.setting.handle(msg);
    }

    /// 根视图：按 `page` 选页面 body，再套顶栏（+ 任务页的底部图标栏）。
    ///
    /// 为什么需要：iced 只接受一个根 `Element`；四页与外框的组合关系（底部图标栏只属于任务页、
    /// 背景色统一走 `theme::surface(p.bg_base)`）正是 UI 的顶层结构，只在这里表达一次。
    /// 入参/出参：`&self`（整份 App 状态，页面各自按需读）；返回根 `Element<Message>`。
    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let p = theme::Palette::pick(self.dark);
        let body = match self.page {
            Page::Task => task::view(self).map(Message::Task),
            Page::Files => crate::files::view(self).map(Message::Files),
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
    ///
    /// 为什么需要：iced 内置控件（text_input/text_editor 等）在未显式给样式时会取应用主题；
    /// dshr 的设计 token 走 `Palette`，但内置控件的默认底色仍需这个 `Theme` 兜住。
    /// 入参/出参：无；返回 `Theme::Dark`/`Theme::Light`。
    pub fn theme(&self) -> Theme {
        if self.dark { Theme::Dark } else { Theme::Light }
    }

    /// 当前设计系统调色板。
    ///
    /// 为什么需要：页面与控件样式都要同一份 `Palette`（theme.rs 的闭包样式全部以它为输入），
    /// 提供统一入口可避免各页面自己判断深浅色。
    /// 入参/出参：无；返回按 `dark` 选中的 `theme::Palette`（Copy）。
    pub fn palette(&self) -> theme::Palette {
        theme::Palette::pick(self.dark)
    }

    /// 常驻订阅：主窗口 id 捕获 + bridge 总线。
    ///
    /// 为什么需要：iced 里「异步事件进入 update」的唯一通道是订阅。两条都是常驻的——
    /// 无边框窗口必须拿到 `window::Id` 才能发窗口控制动作（无 MAIN 常量，首个
    /// `Id::unique()` 即主窗口）；总线流则必须永不结束（见 bridge.rs 的 iced 0.14 说明：
    /// 订阅身份不变时已结束的流不会被重建），否则 runtime 启停一次就再也收不到事件。
    /// 入参/出参：`&self`（当前未用于条件订阅，保持恒返回）；返回合并后的 `Subscription<Message>`。
    pub fn subscription(&self) -> Subscription<Message> {
        // 主窗口 id（窗口控制）+ bridge 常驻总线（runtime 事件/命令通道）。
        Subscription::batch([
            iced::window::open_events().map(Message::WindowId),
            bridge::subscribe().map(Message::Bridge),
        ])
    }

    /// 全局字号（f32；iced 0.14 size 接受 `Into<Pixels>`）。
    ///
    /// 为什么需要：`font_size` 是用户可调的全局基准，各页面散布大量字号常量；用 14 作为 1.0
    /// 基准做比例换算，改一个数就能整体缩放，并给 8.0 下限避免缩到不可读。
    /// 入参/出参：`base` 为该控件在基准字号下的期望尺寸；返回按比例换算后的 f32 字号。
    pub fn fs(&self, base: u16) -> f32 {
        (self.font_size as f32 * base as f32 / 14.0).max(8.0)
    }
}
