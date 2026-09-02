//! 配置页（Zed 式设置页：左侧分区导航 + 右侧分组表单）。
//! 读写 workspace 根 config.json（罗盘落地后迁 data/config.json + data/secrets.json，见 DESIGN §11）。
use std::path::{Path, PathBuf};

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Background, Element, Length};

use crate::app::App;
use crate::theme;

/// 配置页消息。
#[derive(Debug, Clone)]
pub enum Message {
    Section(String),
    ApiKey(String),
    Provider(String),
    Model(String),
    DshVersion(String),
    Save,
    /// 深/浅色切换（App 处理）。
    ThemeToggle,
}

/// 分区清单：(id, 标题, 副标题)。
const SECTIONS: [(&str, &str, &str); 4] = [
    ("general", "通用", "外观与界面"),
    ("models", "模型", "provider / model"),
    ("runtime", "运行时", "dsh 本体版本"),
    ("api", "API 密钥", "本地凭证"),
];

/// 配置页状态。
#[derive(Debug, Default)]
pub struct SettingPane {
    section: String,
    api_key: String,
    provider: String,
    model: String,
    dsh_version: String,
    note: String,
}

impl SettingPane {
    pub fn new() -> Self {
        let mut pane = Self {
            section: "general".to_string(),
            ..Self::default()
        };
        pane.load();
        pane
    }

    fn config_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace 根")
            .join("config.json")
    }

    fn load(&mut self) {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
                    self.api_key = v
                        .get("api-key")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    self.provider = v
                        .get("provider")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("deepseek-official")
                        .to_string();
                    self.model = v
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("deepseek-v4-flash")
                        .to_string();
                    self.dsh_version = v
                        .get("dsh-version")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("0.1.2-alpha.5")
                        .to_string();
                    self.note = format!("已加载 {}", path.display());
                }
                Err(e) => self.note = format!("解析失败: {e}"),
            },
            Err(e) => self.note = format!("读取失败: {e}"),
        }
    }

    fn save(&mut self) {
        let v = serde_json::json!({
            "api-key": self.api_key,
            "provider": self.provider,
            "model": self.model,
            "dsh-version": self.dsh_version,
        });
        let path = Self::config_path();
        match std::fs::write(&path, serde_json::to_string_pretty(&v).expect("序列化")) {
            Ok(()) => self.note = format!("已保存 {}", path.display()),
            Err(e) => self.note = format!("保存失败: {e}"),
        }
    }

    /// 处理配置页消息（ThemeToggle 由 App 消费）。
    pub fn handle(&mut self, msg: Message) {
        match msg {
            Message::Section(s) => self.section = s,
            Message::ApiKey(v) => self.api_key = v,
            Message::Provider(v) => self.provider = v,
            Message::Model(v) => self.model = v,
            Message::DshVersion(v) => self.dsh_version = v,
            Message::Save => self.save(),
            Message::ThemeToggle => {}
        }
    }

    /// 当前分区标题与副标题。
    fn section_meta(&self) -> (&'static str, &'static str) {
        SECTIONS
            .iter()
            .find(|(id, ..)| *id == self.section)
            .map(|(_, title, hint)| (*title, *hint))
            .unwrap_or(("通用", "外观与界面"))
    }

    /// 渲染配置页：左分区导航（选中项带 accent 竖条）+ 右分组内容。
    pub fn view<'a>(&'a self, app: &'a App) -> Element<'a, Message> {
        let p = app.palette();
        let (title, hint) = self.section_meta();

        let rail = column![
            text("设置").size(app.fs(11)).color(p.label_caption),
            Space::new().height(8),
            SECTIONS
                .iter()
                .fold(column![].spacing(6), |col, (id, label, _hint)| {
                    let active = self.section == *id;
                    let item = button(text(*label).size(app.fs(13)))
                        .on_press(Message::Section((*id).to_string()))
                        .style(theme::nav_button(p, active))
                        .padding([7, 10])
                        .width(Length::Fill);
                    // Zed 式选中指示：左缘 2px accent 竖条。
                    let bar = container(Space::new())
                        .width(Length::Fixed(2.0))
                        .height(Length::Fixed(16.0))
                        .style(move |_| container::Style {
                            background: if active {
                                Some(Background::Color(p.accent))
                            } else {
                                None
                            },
                            ..container::Style::default()
                        });
                    col.push(
                        row![bar, item]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center),
                    )
                }),
            Space::new().height(Length::Fill),
            text(Self::config_path().display().to_string())
                .size(app.fs(10))
                .color(p.label_caption),
        ]
        .width(Length::Fixed(220.0))
        .padding(iced::Padding {
            top: 2.0,
            right: 12.0,
            bottom: 12.0,
            left: 8.0,
        });

        let header = row![
            column![
                text(title).size(app.fs(15)).color(p.label_primary),
                text(hint).size(app.fs(11)).color(p.label_caption),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            container(text("config.json").size(app.fs(11)).color(p.label_tertiary))
                .padding([3, 10])
                .style(theme::bordered(p, 6.0)),
        ]
        .align_y(iced::alignment::Vertical::Center);

        let divider = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(theme::surface(p, p.border_l1, 0.0));

        let body = column![
            header,
            Space::new().height(12),
            divider,
            Space::new().height(14),
            self.section_content(app),
            Space::new().height(Length::Fill),
            row![
                text(&self.note).size(app.fs(11)).color(p.label_tertiary),
                Space::new().width(Length::Fill),
                button(text("保存").size(app.fs(13)))
                    .on_press(Message::Save)
                    .style(theme::primary_button(p))
                    .padding([6, 18]),
            ]
            .align_y(iced::alignment::Vertical::Center),
        ];

        container(row![
            container(rail)
                .height(Length::Fill)
                .style(theme::surface(p, p.sidebar_fill, 0.0)),
            container(Space::new())
                .width(Length::Fixed(1.0))
                .height(Length::Fill)
                .style(theme::surface(p, p.border_l1, 0.0)),
            scrollable(container(body).width(Length::Fill).padding(16))
                .width(Length::Fill)
                .height(Length::Fill),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// 当前分区的内容（一组表单行 + 说明小字）。
    fn section_content<'a>(&'a self, app: &'a App) -> Element<'a, Message> {
        let p = app.palette();
        let content = match self.section.as_str() {
            "models" => column![
                group_caption("模型路由", "每次请求经 provider 路由到 model。", p, app),
                field("provider", &self.provider, Message::Provider, app),
                field("model", &self.model, Message::Model, app),
            ]
            .spacing(14),
            "runtime" => column![
                group_caption(
                    "dsh 运行时",
                    "npm dist-tag：alpha = 0.1.2-alpha.5；latest（0.1.1-rc.2）无 sdk profile，必须显式锁 alpha.x。",
                    p,
                    app,
                ),
                field("dsh-version", &self.dsh_version, Message::DshVersion, app),
            ]
            .spacing(14),
            "api" => column![
                group_caption(
                    "DeepSeek API",
                    "仅存本地 config.json（罗盘落地后拆到 data/secrets.json，见 DESIGN §11）。",
                    p,
                    app,
                ),
                field("api-key", &self.api_key, Message::ApiKey, app),
            ]
            .spacing(14),
            _ => column![
                group_caption(
                    "外观",
                    "深色为默认；浅色使用官方 light 调色板（细节仍在核）。",
                    p,
                    app
                ),
                row![
                    text("主题")
                        .size(app.fs(13))
                        .color(p.label_secondary)
                        .width(Length::Fixed(120.0)),
                    button(text("深色").size(app.fs(13)))
                        .on_press(Message::ThemeToggle)
                        .style(theme::nav_button(p, app.dark))
                        .padding([6, 14]),
                    button(text("浅色").size(app.fs(13)))
                        .on_press(Message::ThemeToggle)
                        .style(theme::nav_button(p, !app.dark))
                        .padding([6, 14]),
                ]
                .spacing(8),
            ]
            .spacing(14),
        };
        content.into()
    }
}

/// 分区小标题 + 说明（首行小标题，次行 caption 说明）。
fn group_caption<'a>(
    title: &'static str,
    hint: &'static str,
    p: theme::Palette,
    app: &'a App,
) -> Element<'a, Message> {
    column![
        text(title).size(app.fs(13)).color(p.label_secondary),
        text(hint).size(app.fs(11)).color(p.label_caption),
    ]
    .spacing(4)
    .into()
}

/// 一行「标签 + 输入框」。
fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
    app: &'a App,
) -> Element<'a, Message> {
    let p = app.palette();
    row![
        text(label)
            .size(app.fs(13))
            .color(p.label_secondary)
            .width(Length::Fixed(120.0)),
        text_input(label, value)
            .on_input(on_input)
            .padding([6, 10])
            .style(theme::text_field(p))
            .width(Length::Fill),
    ]
    .spacing(10)
    .into()
}
