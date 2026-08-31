//! 配置页：dshr 自身配置 + secrets + cordis + 外观（主题/字号）。
//!
//! 数据来源：data/ 下三个配置文件，同步读写（命令式，不走事件流）。
//! 职责：本页状态 [`SettingPane`] + 用户操作处理（handle_*）+ 渲染（appearance_block / config_block）。

use std::path::PathBuf;

use iced::widget::text_editor;
use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Element, Length, Task};

use dshr_state::core::config::{self, DshrConfig, Secrets};

use crate::app::{App, MUTED, TOOL_GREEN, WARN, card, fs};
use crate::message::Message;

/// 配置页的三个编辑区块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPane {
    /// dshr 自身配置（config.json）。
    Config,
    /// 敏感配置（secrets.json）。
    Secrets,
    /// dsh 传输配置（cordis.yml）。
    Cordis,
}

/// 配置页状态（App.setting）。
pub struct SettingPane {
    /// data 目录（配置页读写文件用）。
    pub data_dir: PathBuf,
    /// 外观：当前主题 id（配置页「外观」区块编辑态）。
    pub ui_theme: String,
    /// 外观：字号输入框内容（保存时校验）。
    pub font_size_input: String,
    /// dsh 下载镜像源（npm_registry；空 = 官方 registry）。
    pub npm_registry_input: String,
    /// 三个配置文件的多行编辑缓冲。
    pub config_editor: text_editor::Content<iced::Renderer>,
    pub secrets_editor: text_editor::Content<iced::Renderer>,
    pub cordis_editor: text_editor::Content<iced::Renderer>,
}

impl Default for SettingPane {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::new(),
            ui_theme: "tokyo-night".to_string(),
            font_size_input: "14".to_string(),
            npm_registry_input: String::new(),
            config_editor: text_editor::Content::new(),
            secrets_editor: text_editor::Content::new(),
            cordis_editor: text_editor::Content::new(),
        }
    }
}

/// 把三个配置文件读进编辑缓冲区（启动和恢复默认后用）。
pub fn load_editors(app: &mut App) {
    let read =
        |name: &str| std::fs::read_to_string(app.setting.data_dir.join(name)).unwrap_or_default();
    app.setting.config_editor = text_editor::Content::with_text(&read("config.json"));
    app.setting.secrets_editor = text_editor::Content::with_text(&read("secrets.json"));
    app.setting.cordis_editor = text_editor::Content::with_text(&read("cordis.yml"));
}

// ---- 用户操作处理（update 委托进来）----

pub fn handle_config_edit(
    app: &mut App,
    pane: ConfigPane,
    action: text_editor::Action,
) -> Task<Message> {
    let editor = match pane {
        ConfigPane::Config => &mut app.setting.config_editor,
        ConfigPane::Secrets => &mut app.setting.secrets_editor,
        ConfigPane::Cordis => &mut app.setting.cordis_editor,
    };
    editor.perform(action);
    Task::none()
}

pub fn handle_config_save(app: &mut App, pane: ConfigPane) -> Task<Message> {
    let (file, text) = match pane {
        ConfigPane::Config => ("config.json", app.setting.config_editor.text()),
        ConfigPane::Secrets => ("secrets.json", app.setting.secrets_editor.text()),
        ConfigPane::Cordis => ("cordis.yml", app.setting.cordis_editor.text()),
    };
    let path = app.setting.data_dir.join(file);
    if let Err(e) = config::save_text(&path, &text) {
        // 操作级错误 → 弹窗。
        app.dialog = Some(format!("保存 {file} 失败: {e}"));
    } else {
        app.task.messages.push(crate::task::MsgView::Info(format!(
            "✅ 已保存 {file}（重启后生效）"
        )));
    }
    Task::none()
}

pub fn handle_config_reset(app: &mut App, pane: ConfigPane) -> Task<Message> {
    let file = match pane {
        ConfigPane::Config => {
            config::write_json_template(
                &app.setting.data_dir.join("config.json"),
                &DshrConfig::default(),
            );
            "config.json"
        }
        ConfigPane::Secrets => {
            config::write_json_template(
                &app.setting.data_dir.join("secrets.json"),
                &Secrets::default(),
            );
            "secrets.json"
        }
        ConfigPane::Cordis => {
            if let Err(e) = config::save_text(
                &app.setting.data_dir.join("cordis.yml"),
                dshr_state::core::config::CORDIS_TEMPLATE,
            ) {
                app.dialog = Some(format!("重置 cordis.yml 失败: {e}"));
                return Task::none();
            }
            "cordis.yml"
        }
    };
    load_editors(app);
    app.task.messages.push(crate::task::MsgView::Info(format!(
        "✅ 已重置 {file} 为默认模板"
    )));
    Task::none()
}

pub fn handle_theme_select(app: &mut App, name: String) -> Task<Message> {
    // 即时预览：更新主题；保存按钮才落盘 config.json。
    app.setting.ui_theme = name.clone();
    app.theme = crate::app::theme_from_str(&name);
    Task::none()
}

pub fn handle_font_size_changed(app: &mut App, v: String) -> Task<Message> {
    app.setting.font_size_input = v;
    // 即时预览（非法输入回退当前值，保存时才校验）。
    if let Ok(n) = app.setting.font_size_input.parse::<u16>() {
        app.font_size = n.clamp(8, 28);
    }
    Task::none()
}

pub fn handle_appearance_save(app: &mut App) -> Task<Message> {
    // 校验字号输入，回写 config.json 的 ui 字段（其余字段不动）。
    app.font_size = app
        .setting
        .font_size_input
        .parse::<u16>()
        .unwrap_or(14)
        .clamp(8, 28);
    app.setting.font_size_input = app.font_size.to_string();
    let path = app.setting.data_dir.join("config.json");
    let mut saved = false;
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
            v["ui"] = serde_json::json!({
                "theme": app.setting.ui_theme,
                "font_size": app.font_size,
            });
            if let Ok(json) = serde_json::to_string_pretty(&v) {
                saved = std::fs::write(&path, json).is_ok();
            }
        }
    }
    app.task.messages.push(crate::task::MsgView::Info(if saved {
        "✅ 外观已保存（主题即时生效，重启后仍保留）".to_string()
    } else {
        // 操作级错误 → 弹窗。
        app.dialog = Some("外观保存失败：config.json 读取/写入出错".to_string());
        return Task::none();
    }));
    Task::none()
}

/// 镜像源输入变化（不落盘，保存按钮才写 config.json）。
pub fn handle_registry_changed(app: &mut App, v: String) -> Task<Message> {
    app.setting.npm_registry_input = v;
    Task::none()
}

/// 保存镜像源：patch config.json 的 npm_registry 字段（其余字段不动）。
/// 空 = 官方 registry；下载功能重新开放后生效。
pub fn handle_registry_save(app: &mut App) -> Task<Message> {
    let path = app.setting.data_dir.join("config.json");
    let mut saved = false;
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
            v["npm_registry"] =
                serde_json::Value::String(app.setting.npm_registry_input.trim().to_string());
            if let Ok(json) = serde_json::to_string_pretty(&v) {
                saved = std::fs::write(&path, json).is_ok();
            }
        }
    }
    app.task.messages.push(crate::task::MsgView::Info(if saved {
        "✅ 镜像源已保存（下载功能开放后生效）".to_string()
    } else {
        "镜像源保存失败：config.json 读取/写入出错".to_string()
    }));
    Task::none()
}

// ---- 渲染 ----

/// 配置页：dsh 运行时 + 外观区块 + 三个编辑区块（config / secrets / cordis）。
pub fn setting_page(app: &App) -> Element<'_, Message> {
    column![
        text("配置（修改后点保存，重启生效）").size(fs(app, 18)),
        dsh_block(app),
        appearance_block(app),
        config_block(
            app,
            ConfigPane::Config,
            "config.json · dshr 自身配置",
            &app.setting.config_editor
        ),
        config_block(
            app,
            ConfigPane::Secrets,
            "secrets.json · API key 等敏感项",
            &app.setting.secrets_editor
        ),
        config_block(
            app,
            ConfigPane::Cordis,
            "cordis.yml · dsh 传输配置（插件装配）",
            &app.setting.cordis_editor
        ),
    ]
    .spacing(12)
    .padding(20)
    .into()
}

/// dsh 运行时区块：安装状态（下载功能本期禁用）+ 镜像源输入（决策 23）。
fn dsh_block(app: &App) -> Element<'_, Message> {
    let dsh_dir = app
        .setting
        .data_dir
        .parent()
        .map(|p| p.join("dsh"))
        .unwrap_or_else(|| std::path::Path::new("dsh").to_path_buf());
    let installed = dshr_state::task::fetch::is_installed(&dsh_dir);

    container(
        column![
            row![
                text("dsh 无头运行时").size(fs(app, 13)).color(MUTED),
                Space::new().width(10),
                text(if installed {
                    "已安装 ✓"
                } else {
                    "未安装"
                })
                .size(fs(app, 12))
                .color(if installed { TOOL_GREEN } else { WARN }),
                Space::new().width(Length::Fill),
                // 决策 24：下载功能本期禁用（代码保留在 state/fetch.rs，开放后换回按钮）。
                text("下载功能暂未开放").size(fs(app, 11)).color(MUTED),
            ]
            .spacing(6)
            .padding([10, 12]),
            row![
                text("npm 镜像源").size(fs(app, 12)).color(MUTED),
                text_input(
                    "留空 = 官方 registry（如 https://registry.npmmirror.com）",
                    &app.setting.npm_registry_input
                )
                .on_input(Message::RegistryChanged)
                .width(Length::Fill)
                .padding([4, 8]),
                button("保存").on_press(Message::RegistrySave),
            ]
            .spacing(8)
            .padding([6, 12]),
        ]
        .spacing(2),
    )
    .style(card(None, 8.0))
    .into()
}

/// 外观区块：主题选择（即时预览）+ 字号 + 保存。
fn appearance_block(app: &App) -> Element<'_, Message> {
    // (显示名, 主题 id)
    const OPTIONS: [(&str, &str); 6] = [
        ("白", "light"),
        ("黑", "dark"),
        ("灰", "gray"),
        ("东京夜", "tokyo-night"),
        ("德古拉", "dracula"),
        ("日光", "solarized"),
    ];
    let mut theme_row = row![].spacing(6);
    for (label, id) in OPTIONS {
        let active = app.setting.ui_theme == id;
        let btn =
            button(text(label).size(fs(app, 13))).on_press(Message::ThemeSelect(id.to_string()));
        // 选中项用主色按钮，未选中用次要按钮（iced 0.14 的 style fn）。
        theme_row = theme_row.push(btn.style(if active {
            iced::widget::button::primary
        } else {
            iced::widget::button::secondary
        }));
    }

    container(
        column![
            text("外观（主题即时预览，保存后持久化）")
                .size(fs(app, 13))
                .color(MUTED),
            theme_row,
            row![
                text("字号").size(fs(app, 13)),
                text_input("14", &app.setting.font_size_input)
                    .on_input(Message::FontSizeChanged)
                    .width(60)
                    .padding([2, 6]),
                text("（8-28，默认 14）").size(fs(app, 11)).color(MUTED),
                Space::new().width(Length::Fill),
                button("保存外观").on_press(Message::AppearanceSave),
            ]
            .spacing(8),
        ]
        .spacing(6)
        .padding([10, 12]),
    )
    .style(card(None, 8.0))
    .into()
}

/// 一个编辑区块：标题 + 多行编辑器 + 保存/恢复默认按钮。
fn config_block<'a>(
    app: &App,
    pane: ConfigPane,
    title: &'a str,
    editor: &'a text_editor::Content<iced::Renderer>,
) -> Element<'a, Message> {
    column![
        text(title).size(fs(app, 13)).color(MUTED),
        container(
            text_editor(editor)
                .on_action(move |action| Message::ConfigEdit(pane, action))
                .height(Length::FillPortion(3))
                .padding(8)
        )
        .style(card(None, 6.0)),
        row![
            Space::new().width(Length::Fill),
            button("恢复默认").on_press(Message::ConfigReset(pane)),
            button("保存").on_press(Message::ConfigSave(pane)),
        ]
        .spacing(8),
    ]
    .spacing(6)
    .into()
}
