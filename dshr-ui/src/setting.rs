//! 配置页：普通页面（非模态），左右分栏——左：配置类别导航；右：每类别可配置项。
//! 节俭版内容：通用（外观）/ 模型 / API（config.json 读写）。
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, text, text_input};
use iced::{Element, Length};

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
                        .unwrap_or("0.1.2-alpha.3")
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

    /// 渲染配置页（普通页面：左类别导航 + 右内容）。
    pub fn view<'a>(&'a self, app: &'a App) -> Element<'a, Message> {
        let p = app.palette();
        let sections: [(&str, &str); 3] = [("general", "通用"), ("models", "模型"), ("api", "API")];
        let rail = sections.iter().fold(column![].spacing(2), |col, (id, label)| {
            col.push(
                button(text(*label).size(app.fs(13)))
                    .on_press(Message::Section((*id).to_string()))
                    .style(theme::nav_button(p, self.section == *id))
                    .padding([8, 12])
                    .width(Length::Fill),
            )
        });
        let content = match self.section.as_str() {
            "models" => column![
                text("模型").size(15).color(p.label_primary),
                field("provider", &self.provider, Message::Provider, app),
                field("model", &self.model, Message::Model, app),
                field("dsh-version", &self.dsh_version, Message::DshVersion, app),
            ],
            "api" => column![
                text("API").size(15).color(p.label_primary),
                field("api-key", &self.api_key, Message::ApiKey, app),
            ],
            _ => column![
                text("通用").size(15).color(p.label_primary),
                row![
                    button(text("切换主题"))
                        .on_press(Message::ThemeToggle)
                        .style(theme::ghost_button(p))
                        .padding([6, 12]),
                ],
            ],
        };
        container(row![
            container(rail).width(Length::Fixed(200.0)).padding([0, 8]),
            container(column![
                content,
                text(&self.note).size(12).color(p.label_tertiary),
                row![button(text("保存"))
                    .on_press(Message::Save)
                    .style(theme::primary_button(p))
                    .padding([6, 16])],
            ])
            .padding(16)
            .width(Length::Fill),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .into()
    }
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
        text(label).size(app.fs(13)).color(p.label_secondary).width(Length::Fixed(100.0)),
        text_input(label, value).on_input(on_input).width(Length::Fill),
    ]
    .spacing(8)
    .into()
}
