//! 根组件：App 状态机 + 根视图分发（类似 Vue/React 的根，管挂载和分发）。
//!
//! - 全局：引擎句柄（AppState）、页面、外观、启动错误
//! - 各页状态在 `task` / `monitor` / `setting`（每页一个文件：状态 + 处理 + 渲染）
//! - update 只做分发（body 委托到各页的 handle_*）；view 只做挂载（match 页面 → 各页渲染）
//! - 公共渲染工具（[`fs`] 字号 / [`card`] 卡片 / 配色常量）也在这，各页共用

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::container::Style as ContainerStyle;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Subscription, Task, Theme};

use dshr_state::core::config;
use dshr_state::{AppState, Command, EventReceiver};

use crate::message::Message;
use crate::monitor::MonitorPane;
use crate::nav;
use crate::setting::{SettingPane, load_editors};
use crate::task::TaskPane;
use crate::{monitor, setting, task};

// ---- 公共渲染工具（各页共用）----

/// 配色（深浅主题都兼容的中性色；气泡色不随主题）
pub(crate) const ACCENT: Color = Color::from_rgb(0.36, 0.66, 0.99); // 选中蓝
pub(crate) const MUTED: Color = Color::from_rgb(0.55, 0.58, 0.66); // 次要文字
pub(crate) const USER_BUBBLE: Color = Color::from_rgb(0.23, 0.38, 0.62); // user 气泡
pub(crate) const ASSIST_BUBBLE: Color = Color::from_rgb(0.25, 0.27, 0.35); // assistant 气泡
pub(crate) const TOOL_GREEN: Color = Color::from_rgb(0.45, 0.72, 0.55);
pub(crate) const WARN: Color = Color::from_rgb(0.9, 0.55, 0.4);

/// 全局字号：以 `app.font_size`（默认 14）为基准线性缩放，下限 8。
/// 返回 f32（iced 0.14 的 size 接受 `Into<Pixels>`，u16 不实现，f32 实现）。
pub(crate) fn fs(app: &App, base: u16) -> f32 {
    (app.font_size as f32 * base as f32 / 14.0).max(8.0)
}

/// 圆角卡片样式（container 背景 + 边框圆角）。
pub(crate) fn card(bg: Option<Color>, radius: f32) -> impl Fn(&Theme) -> ContainerStyle {
    move |_| ContainerStyle {
        background: bg.map(Background::Color),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..ContainerStyle::default()
    }
}

/// 顶部菜单页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 任务：对话面板（sidebar + chat_area）。
    Task,
    /// 监控：数据看板（M3 填充，先占位）。
    Monitor,
    /// 配置：config.json / secrets.json / cordis.yml 的查看与编辑。
    Config,
}

/// 配置主题 id → iced Theme（未知 id 回退 tokyo-night）。
/// gray 是自定义中性灰 palette（iced 内置没有灰）。
pub fn theme_from_str(id: &str) -> Theme {
    use iced::theme::Custom;
    use iced::{Color, Theme as T};
    match id {
        "light" => T::Light,
        "dark" => T::Dark,
        "gray" => T::Custom(Arc::new(Custom::new(
            "gray".to_string(),
            iced::theme::Palette {
                background: Color::from_rgb(0.52, 0.53, 0.57),
                text: Color::from_rgb(0.95, 0.95, 0.96),
                primary: Color::from_rgb(0.42, 0.48, 0.55),
                success: Color::from_rgb(0.42, 0.72, 0.55),
                warning: Color::from_rgb(0.85, 0.7, 0.4),
                danger: Color::from_rgb(0.85, 0.42, 0.42),
            },
        ))),
        "dracula" => T::Dracula,
        "solarized" => T::SolarizedDark,
        _ => T::TokyoNight,
    }
}

/// 事件订阅句柄：subscription 的 data 需要 `Hash` 判重（变了会重启订阅）。
/// 用 `Arc` 指针 hash：引擎启动后地址不变 → 订阅不意外重启。
#[derive(Clone)]
struct EventsHandle(EventReceiver);

impl std::hash::Hash for EventsHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), state);
    }
}

impl PartialEq for EventsHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for EventsHandle {}

/// UI 状态（iced 的 Model）。
pub struct App {
    /// 引擎句柄（后台线程的 UI 端：发命令用）。
    pub state: Option<AppState>,
    /// 事件接收端（subscription 常驻收流用；Arc clone 自 AppState）。
    pub events: Option<EventReceiver>,
    pub page: Page,
    /// 当前 iced 主题（外观配置，即时生效）。
    pub theme: Theme,
    /// 全局字号基准（view 按它缩放）。
    pub font_size: u16,
    /// 启动错误（配置缺失等；错误页仍可进配置页修复）。
    pub boot_error: Option<String>,
    /// 弹窗（操作/全局错误；view 层叠加显示）。
    pub dialog: Option<String>,
    /// 下载进度弹窗（dsh 首次安装/更新；view 层叠加显示）。
    pub fetch: Option<crate::task::FetchUi>,
    /// 各页状态。
    pub task: TaskPane,
    /// 监控页（M3 占位，字段暂未读取）。
    #[allow(dead_code)]
    pub monitor: MonitorPane,
    pub setting: SettingPane,
}

impl App {
    /// 启动：加载配置（config.json/secrets.json/cordis.yml）→ 起后台引擎线程。
    /// 生成：(App, 初始 Task)。配置失败/缺 key 时进入 boot_error 提示。
    pub fn new() -> (Self, Task<Message>) {
        // workspace 根 = dshr-ui 的上一级（dshr/）
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");

        let mut app = Self {
            state: None,
            events: None,
            page: Page::Task,
            theme: Theme::TokyoNight,
            font_size: 14,
            boot_error: None,
            dialog: None,
            fetch: None,
            task: TaskPane::default(),
            monitor: MonitorPane::default(),
            setting: SettingPane::default(),
        };

        match config::load(&root) {
            Ok(config) => {
                app.setting.data_dir = config.data_dir.clone();
                // 外观：主题即时生效，字号进 view 缩放（默认 14 = 原写死大小）。
                app.setting.ui_theme = config.dshr.ui.theme.clone();
                app.theme = theme_from_str(&config.dshr.ui.theme);
                app.font_size = config.dshr.ui.font_size.clamp(8, 28);
                app.setting.font_size_input = app.font_size.to_string();
                // 镜像源（npm_registry，空 = 官方 registry）。
                app.setting.npm_registry_input = config.dshr.npm_registry.clone();
                // 校验 secrets：没有 key 就别起引擎（配置页可填）。
                if config.secrets.api_key.is_none() {
                    app.boot_error = Some(
                        "未找到 API key：请到 配置 页填写 data/secrets.json 的 api_key（或直接编辑文件）".into(),
                    );
                    load_editors(&mut app);
                    return (app, Task::none());
                }
                // 校验运行时可用的前置：dsh 未装时需要 harness_root（官方仓库），
                // 否则 node 进程 current_dir("") spawn 失败 → runtime 起不来。
                // 下载 dsh 后（决策 23）不再需要官方仓库。
                let dsh_dir = app
                    .setting
                    .data_dir
                    .parent()
                    .map(|p| p.join("dsh"))
                    .unwrap_or_else(|| Path::new("dsh").to_path_buf());
                if !dshr_state::task::fetch::is_installed(&dsh_dir)
                    && config.dshr.harness_root.is_empty()
                {
                    app.boot_error = Some(
                        "dsh 运行时未安装且未设置 harness_root（官方仓库路径）：请到 配置 页填写 config.json 的 harness_root（下载功能暂未开放）".into(),
                    );
                    load_editors(&mut app);
                    return (app, Task::none());
                }
                match AppState::start(config) {
                    Ok(state) => {
                        // 事件接收端 clone 给 subscription（事件流直收，不轮询）。
                        app.events = Some(state.take_events());
                        app.state = Some(state);
                        load_editors(&mut app);
                    }
                    Err(e) => app.boot_error = Some(e.to_string()),
                }
                // 下载功能本期禁用（代码保留）：首次运行不自动 FetchDsh，
                // 配置页也不显示下载按钮；镜像源配置好以后重新开放。
            }
            Err(e) => app.boot_error = Some(e.to_string()),
        }
        (app, Task::none())
    }

    /// 事件订阅：直接挂到 AppState 的 events 通道上（事件驱动，零延迟、不空转）。
    /// 相比 100ms 轮询：事件一到就产生 Message::Event 送进 update，流式输出无顿挫。
    pub fn subscription(&self) -> Subscription<Message> {
        match &self.events {
            Some(events) => {
                let handle = EventsHandle(events.clone());
                Subscription::run_with(handle, |handle| {
                    let events = handle.0.clone();
                    // unfold：持续 recv，通道关闭（引擎退出）则流结束。
                    // tokio::sync::Mutex 的 guard 跨 await 是 Send（subscription 要求 stream Send）。
                    futures::stream::unfold(events, |events| async move {
                        let ev = events.lock().await.recv().await?;
                        Some((Message::Event(ev), events))
                    })
                })
            }
            None => Subscription::none(),
        }
    }

    /// iced update：接 Message → 分发到各页处理 / 全局处理。
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // 事件流直收（subscription 挂 events 通道，事件到即处理）→ 任务页应用。
            Message::Event(ev) => {
                task::apply_event(self, ev);
                Task::none()
            }
            Message::Navigate(page) => {
                self.page = page;
                // 从启动错误页进入正常页（配置修复后重启生效）。
                self.boot_error = None;
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
            Message::DismissDialog => {
                self.dialog = None;
                Task::none()
            }
            Message::Noop => Task::none(),
            Message::FetchDsh => {
                if let Some(state) = &self.state {
                    state.send_command(Command::FetchDsh);
                }
                Task::none()
            }
            Message::DismissFetch => {
                self.fetch = None;
                Task::none()
            }
            // ---- 任务页 ----
            Message::AddPressed => task::handle_add_pressed(self),
            Message::CancelAdd => task::handle_cancel_add(self),
            Message::NameChanged(v) => task::handle_name_changed(self, v),
            Message::PathChanged(v) => task::handle_path_changed(self, v),
            Message::ConfirmAdd => task::handle_confirm_add(self),
            Message::NewSession(runtime_id) => task::handle_new_session(self, runtime_id),
            Message::SelectSession(session_id) => task::handle_select_session(self, session_id),
            Message::InputAction(action) => task::handle_input_action(self, action),
            Message::SendPressed => task::handle_send_pressed(self),
            Message::ToggleInputExpand => task::handle_toggle_input_expand(self),
            Message::ToggleMenu(target) => task::handle_toggle_menu(self, target),
            Message::StartRename(target) => task::handle_start_rename(self, target),
            Message::RenameChanged(v) => task::handle_rename_changed(self, v),
            Message::ConfirmRename => task::handle_confirm_rename(self),
            Message::CancelRename => task::handle_cancel_rename(self),
            Message::WorkspaceAdd => task::handle_workspace_add(self),
            Message::WorkspaceChanged(v) => task::handle_workspace_changed(self, v),
            Message::ConfirmWorkspace => task::handle_confirm_workspace(self),
            Message::CancelWorkspace => task::handle_cancel_workspace(self),
            Message::ArchiveRuntime(runtime_id) => task::handle_archive_runtime(self, runtime_id),
            Message::DeleteRuntime(runtime_id) => task::handle_delete_runtime(self, runtime_id),
            Message::ArchiveSession(session_id) => task::handle_archive_session(self, session_id),
            Message::DeleteSession(session_id) => task::handle_delete_session(self, session_id),
            Message::FileOpen(path) => task::handle_file_open(self, path),
            Message::FileUp => task::handle_file_up(self),
            // ---- 配置页 ----
            Message::ConfigEdit(pane, action) => setting::handle_config_edit(self, pane, action),
            Message::ConfigSave(pane) => setting::handle_config_save(self, pane),
            Message::ConfigReset(pane) => setting::handle_config_reset(self, pane),
            // ---- 外观 ----
            Message::ThemeSelect(name) => setting::handle_theme_select(self, name),
            Message::FontSizeChanged(v) => setting::handle_font_size_changed(self, v),
            Message::AppearanceSave => setting::handle_appearance_save(self),
            // ---- dsh 下载/镜像源 ----
            Message::RegistryChanged(v) => setting::handle_registry_changed(self, v),
            Message::RegistrySave => setting::handle_registry_save(self),
        }
    }

    /// 根视图：顶部导航 + 按页内容（boot_error 时仍可进配置页修复）。
    /// 弹窗：fetch（下载 dsh）优先于 dialog（错误），用 Stack 叠加半透明遮罩。
    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = if let Some(err) = &self.boot_error {
            // 配置不完整：不锁死界面，给出修复路径（配置页可填）。
            let mut col = column![text(format!("配置不完整：{err}")).size(fs(self, 16))]
                .spacing(10)
                .padding(20);
            col = col.push(
                text("到「配置」页填写并保存后重启生效。")
                    .size(fs(self, 13))
                    .color(MUTED),
            );
            col = col.push(
                button("去配置页")
                    .on_press(Message::Navigate(Page::Config))
                    .padding([6, 16]),
            );
            container(col)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        } else {
            match self.page {
                Page::Task => task::task_page(self),
                Page::Monitor => monitor::monitor_page(self),
                Page::Config => setting::setting_page(self),
            }
        };
        let base: Element<'_, Message> = column![nav::top_nav(self), body].into();

        // 叠加层：fetch 下载弹窗 > 错误弹窗 > 无。
        let overlay: Option<Element<'_, Message>> = if let Some(fetch) = &self.fetch {
            Some(self.fetch_overlay(fetch))
        } else {
            self.dialog.as_ref().map(|msg| self.dialog_overlay(msg))
        };

        match overlay {
            Some(layer) => {
                let mut layers: iced::widget::Stack<'_, Message, Theme, iced::Renderer> =
                    iced::widget::Stack::new();
                layers = layers.push(base).push(layer);
                layers.into()
            }
            None => base,
        }
    }

    /// 半透明全屏遮罩（点击关闭）。
    fn mask(&self, close: Message) -> iced::widget::Button<'_, Message, Theme, iced::Renderer> {
        button("")
            .on_press(close)
            .style(|_theme: &Theme, _status: iced::widget::button::Status| {
                iced::widget::button::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                    ..iced::widget::button::Style::default()
                }
            })
            .width(Length::Fill)
            .height(Length::Fill)
    }

    /// 居中卡片（背景/文字跟随主题）。
    fn card_layer<'a>(
        &'a self,
        content: impl Into<Element<'a, Message>> + 'a,
    ) -> iced::widget::Container<'a, Message, Theme, iced::Renderer> {
        container(
            container(content.into()).style(|theme: &Theme| ContainerStyle {
                background: Some(Background::Color(theme.palette().background)),
                border: Border {
                    radius: 10.0.into(),
                    ..Border::default()
                },
                ..ContainerStyle::default()
            }),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
    }

    /// 错误弹窗层：遮罩 + 居中消息 + 确定。
    fn dialog_overlay<'a>(&'a self, msg: &'a str) -> Element<'a, Message> {
        let mut layers: iced::widget::Stack<'_, Message, Theme, iced::Renderer> =
            iced::widget::Stack::new();
        layers = layers.push(self.mask(Message::DismissDialog)).push(
            self.card_layer(
                column![
                    text(msg).size(fs(self, 14)),
                    Space::new().height(14),
                    button("确定").on_press(Message::DismissDialog),
                ]
                .spacing(8)
                .padding(20),
            ),
        );
        layers.into()
    }

    /// 下载进度弹窗层：遮罩（下载中不可点关闭）+ 标题 + 进度行 + 完成/失败按钮。
    fn fetch_overlay<'a>(&'a self, fetch: &'a crate::task::FetchUi) -> Element<'a, Message> {
        let mut lines = column![].spacing(2).padding([6, 8]);
        for line in &fetch.lines {
            let color = if line.starts_with("npm ERR") || line.starts_with("未找到") {
                Color::from_rgb(0.92, 0.35, 0.35)
            } else {
                Color::from_rgb(0.7, 0.75, 0.82)
            };
            lines = lines.push(text(line.as_str()).size(fs(self, 10)).color(color));
        }
        let status = if fetch.done {
            if fetch.ok {
                "✅ 完成".to_string()
            } else {
                "❌ 失败".to_string()
            }
        } else {
            "⏳ 正在下载…".to_string()
        };

        let mut layers: iced::widget::Stack<'_, Message, Theme, iced::Renderer> =
            iced::widget::Stack::new();
        // 下载中遮罩不可关（点击无效）；结束后可点遮罩关闭。
        let mask = if fetch.done {
            self.mask(Message::DismissFetch)
        } else {
            self.mask(Message::Noop)
        };
        layers = layers.push(mask).push(
            self.card_layer(
                column![
                    text("下载 dsh 运行时").size(fs(self, 15)),
                    text(status).size(fs(self, 12)).color(MUTED),
                    container(scrollable(lines).height(200))
                        .width(380)
                        .style(card(None, 6.0)),
                    row![
                        Space::new().width(Length::Fill),
                        if fetch.done {
                            button("关闭").on_press(Message::DismissFetch)
                        } else {
                            button("后台继续").on_press(Message::DismissFetch)
                        },
                    ],
                ]
                .spacing(10)
                .padding(20),
            ),
        );
        layers.into()
    }
}
