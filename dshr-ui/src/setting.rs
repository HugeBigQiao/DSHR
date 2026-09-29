//! 配置页（Zed 式设置页：左侧分区导航 + 右侧分组表单）。
//! 读写 workspace 根 config.json（罗盘落地后迁 data/config.json + data/secrets.json，见 DESIGN.md §8.1）。
//!
//! 主要用途：四个分区（通用/模型/运行时/API）的表单——编辑 provider / model / dsh-version
//! 与 API key，点「保存」落盘，并把读取/保存结果以状态行小字反馈。
//! 为什么需要：配置是 UI 唯一直接读写文件的地方（不经 state 的 engine/raw），且它同时管两个
//! 落点（`config.json` 普通配置 + `secrets.json` 敏感凭据）——两者的写入顺序、失败回滚语义
//! 与提示文案都集中在这里，别的页面只读 `App::setting.api_key_missing()` 做提示。
//! 它约束：**保存是显式按钮**（不是自动保存），且 API key 绝不写进 config.json。
//! 上接：`app::{App::handle_setting, App::new}`（`api_key_missing` 决定启动提示）。
//! 下接：`dshr_state::secrets`（load/save API key）、`std::fs`（config.json）、`theme`（表单样式）。
//! 官方对应：`packages/client/ui-settings-general/src/client/SettingsRoot.tsx` 的 `SettingsRoot`
//! （左分区导航 + 右内容）；表单行形态对应 `packages/client/ui-primitives/src/settings-form/SettingsForm.tsx`。
use std::path::{Path, PathBuf};

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Background, Element, Length};

use dshr_state::secrets;

use crate::app::App;
use crate::theme;

/// 配置页消息。
///
/// 为什么需要：配置页是受控表单——每个输入框只发「新值」，落盘统一走 `Save`；
/// 把「编辑」与「提交」分成不同变体，才能保证编辑中途不写盘（config.json 非自动保存）。
#[derive(Debug, Clone)]
pub enum Message {
    /// 切换左侧分区（参数为 `SECTIONS` 的 id）。
    Section(String),
    /// API key 输入框变更（只改内存，点保存才落 secrets.json）。
    ApiKey(String),
    /// provider 输入框变更（只改内存）。
    Provider(String),
    /// model 输入框变更（只改内存）。
    Model(String),
    /// dsh-version 输入框变更（只改内存；改它影响 runtime 获取的锁定版本）。
    DshVersion(String),
    /// 点「保存」：写 config.json + secrets.json。
    Save,
    /// 深/浅色切换（App 处理）。
    ThemeToggle,
}

/// 分区清单：(id, 标题, 副标题)。
///
/// 为什么需要：导航项与右栏标题/说明都由它驱动，避免「一个分区加了三处字符串」；
/// id 是 `section_content` 的分支判据，标题/副标题是产品文案（改名不影响逻辑）。
const SECTIONS: [(&str, &str, &str); 4] = [
    ("general", "通用", "外观与界面"),
    ("models", "模型", "provider / model"),
    ("runtime", "运行时", "dsh 本体版本"),
    ("api", "API 密钥", "本地凭证"),
];

/// 配置页状态。
///
/// 为什么需要：表单是「一次编辑、显式提交」的模型，所以必须持有各字段的**内存草稿**
/// （`provider`/`model`/`dsh_version`/`api_key`）与结果提示（`note`），而不是每次击键都读文件。
#[derive(Debug, Default)]
pub struct SettingPane {
    /// 当前分区 id（`SECTIONS` 之一）。
    section: String,
    /// API key 草稿（初始来自 secrets.json）。
    api_key: String,
    /// provider 草稿（初始来自 config.json）。
    provider: String,
    /// model 草稿（初始来自 config.json）。
    model: String,
    /// dsh 本体版本草稿（初始来自 config.json；缺省为锁定版本）。
    dsh_version: String,
    /// 状态行提示（加载/保存结果、解析失败原因）。
    note: String,
}

impl SettingPane {
    /// 构造配置页并立即加载现有配置。
    ///
    /// 为什么需要：配置页必须一打开就显示真实值（而不是空表单），所以加载放在构造里；
    /// 失败也不 panic，而是写进 `note` 让用户看到原因。
    /// 入参/出参：无；返回已加载的 `SettingPane`（`section` 固定为 general）。
    pub fn new() -> Self {
        let mut pane = Self {
            section: "general".to_string(),
            ..Self::default()
        };
        pane.load();
        pane
    }

    /// config.json 的绝对路径（workspace 根 = 本 crate 的上级目录）。
    ///
    /// 为什么需要：路径必须在「读」与「写」两处一致；用 `CARGO_MANIFEST_DIR` 推导而不是运行时
    /// cwd，保证从任何工作目录启动都指向同一个文件。
    /// 入参/出参：无；返回 `<dshr>/config.json`（父目录缺失会 panic——编译期常量保证不会）。
    fn config_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace 根")
            .join("config.json")
    }

    /// 读 config.json + secrets.json 填进表单草稿，并把结果写进 `note`。
    ///
    /// 为什么需要：三个字段都各自有「缺省值」策略（读不到就用默认 provider/model/锁定版本），
    /// 集中一处才能保证缺文件、缺字段、JSON 坏掉三种情况都有明确提示而不是静默空白。
    /// 入参/出参：无；无返回值。错误条件：文件不存在/解析失败/API key 缺失——都不 panic，
    /// 分别落到 `note`（API key 为空时给出「会回退 Fake」的明确警告）。
    fn load(&mut self) {
        let path = Self::config_path();
        let workspace = path.parent().unwrap_or_else(|| Path::new("."));
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
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
                        .unwrap_or("0.1.7-rc.2")
                        .to_string();
                    self.api_key = secrets::load_api_key(workspace).unwrap_or_default();
                    self.note = if self.api_key.trim().is_empty() {
                        "未配置 API key：Real 模式会回退 Fake，请填写并保存。".to_string()
                    } else {
                        format!("已加载 {}", path.display())
                    };
                }
                Err(e) => self.note = format!("解析失败: {e}"),
            },
            Err(e) => self.note = format!("读取失败: {e}"),
        }
    }

    /// 把表单草稿写盘：config.json（provider/model/dsh-version）+ secrets.json（API key）。
    ///
    /// 为什么需要：两个落点必须一起成功才谈得上「已保存」——所以这里先写 config.json，
    /// 失败即返回；再写 secrets.json，失败也只改 `note`。把失败分支留在本函数，是为了让
    /// 调用方（`handle`）永远是纯状态转移。
    /// 入参/出参：无；无返回值。错误条件：写文件失败 / secrets 保存失败——都不 panic，
    /// 分别以 `note` 文案反馈（空 API key 是合法输入，但会明确提示 Real 模式将回退 Fake）。
    fn save(&mut self) {
        let path = Self::config_path();
        let workspace = path.parent().unwrap_or_else(|| Path::new("."));
        let value = serde_json::json!({
            "provider": self.provider,
            "model": self.model,
            "dsh-version": self.dsh_version,
        });
        if let Err(error) = std::fs::write(
            &path,
            serde_json::to_string_pretty(&value).expect("序列化 config.json"),
        ) {
            self.note = format!("保存失败: {error}");
            return;
        }
        if let Err(error) = secrets::save_api_key(workspace, &self.api_key) {
            self.note = format!("保存 API key 失败: {error}");
            return;
        }
        self.note = if self.api_key.trim().is_empty() {
            "已保存；API key 为空，Real 模式将回退 Fake".to_string()
        } else {
            format!("已保存 {}", path.display())
        };
    }

    /// API key 是否为空（用于启动提示/设置页警告）。
    ///
    /// 为什么需要：这是「会不会回退 Fake」的唯一判据，`App::new` 与配置页正文都要用它；
    /// 以 `trim()` 判断，避免只有空格时被当成已配置（那种 key 一定无效）。
    /// 入参/出参：无；返回 true = 未配置。
    pub fn api_key_missing(&self) -> bool {
        self.api_key.trim().is_empty()
    }

    /// 处理配置页消息（ThemeToggle 由 App 消费）。
    ///
    /// 为什么需要：把「输入 → 草稿」与「提交 → 落盘」收敛成唯一入口，App 只需转发。
    /// 入参/出参：`msg` 为配置页消息；无返回值。
    /// 注意：`ThemeToggle` 在这里是空分支（真处理在 `App::handle_setting`），保留分支是为了
    /// 匹配穷尽、避免将来加变体时漏掉。
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
    ///
    /// 为什么需要：右栏标题必须跟随左侧选中项；从 `SECTIONS` 反查而不是另存一份，
    /// 保证标题/副标题与导航项永远同源（id 不匹配时回落到「通用」而不是空白）。
    /// 入参/出参：无；返回 `(标题, 副标题)` 静态文案。
    fn section_meta(&self) -> (&'static str, &'static str) {
        SECTIONS
            .iter()
            .find(|(id, ..)| *id == self.section)
            .map(|(_, title, hint)| (*title, *hint))
            .unwrap_or(("通用", "外观与界面"))
    }

    /// 渲染配置页：左分区导航（选中项带 accent 竖条）+ 右分组内容。
    ///
    /// 为什么需要：Zed 式设置页的骨架（固定宽导航栏 + 1px 分隔 + 可滚动正文 + 底部保存行）
    /// 只在这里表达一次；`section_content` 只负责「当前分区的内容」。
    /// 入参/出参：`app` 提供调色板/字号/深色开关；返回配置页 `Element<Message>`。
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
                text(&self.note)
                    .size(app.fs(11))
                    .color(if self.api_key_missing() {
                        p.error
                    } else {
                        p.label_tertiary
                    }),
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
    ///
    /// 为什么需要：四个分区的表单差异只在字段组合，集中在这个 match 里就能让 `view` 保持
    /// 「骨架 + 内容」两层；说明小字（caption）承担「这个字段是干什么的」的解释职责。
    /// 入参/出参：`app` 提供调色板/字号；返回当前分区的 `Element<Message>`。
    /// 注意：`_` 分支 = 通用分区（含主题切换按钮），新增分区需在此加分支。
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
                    "版本必须显式指定（dshr 不做语义化版本解析）。当前 npm：latest = 0.1.7-rc.2、alpha = 0.1.7-alpha.2、next = 0.2.0-rc.1；其中 0.1.5 及以后含 SDK profile 装配（dsh-sdk-app），0.1.1-rc.2 不含、用它会把 sdk profile 跑挂。",
                    p,
                    app,
                ),
                field("dsh-version", &self.dsh_version, Message::DshVersion, app),
            ]
            .spacing(14),
            "api" => {
                let mut content = column![
                    group_caption(
                        "DeepSeek API",
                        "仅存本地 data/secrets.json（Unix 0600；不再写 config.json）。",
                        p,
                        app,
                    ),
                    secret_field("api-key", &self.api_key, Message::ApiKey, app),
                ]
                .spacing(14);
                if self.api_key_missing() {
                    content = content.push(
                        text("API key 为空：Real 模式会回退 Fake；请填写并保存。")
                            .size(app.fs(11))
                            .color(p.error),
                    );
                }
                content
            }
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
///
/// 为什么需要：官方设置面板的每个分组都是「标题 + caption 说明」两层；抽成函数保证四个分区
/// 的行距/字号一致。入参：`title`/`hint` 为静态文案，`p` 调色板，`app` 取字号基准；
/// 返回该分组头部的 `Element<Message>`。
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
///
/// 为什么需要：三个普通字段（provider/model/dsh-version）只有标签与消息构造器不同，
/// 抽成一行工厂避免三份几乎相同的布局代码漂移（标签固定宽 120 才能对齐输入框左缘）。
/// 入参：`label`/`value` 当前文本，`on_input` 为「新值 → 消息」的构造器（fn 指针，无捕获），
/// `app` 取调色板/字号；返回该行的 `Element<Message>`。
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

/// 密钥输入行：同 `field`，但遮蔽显示。
///
/// 为什么需要：API key 不能明文上屏（肩窥/录屏）；`secure(true)` 让 iced 只画圆点，
/// 又保留正常的编辑体验。入参/出参同 `field`。
fn secret_field<'a>(
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
            .secure(true)
            .on_input(on_input)
            .padding([6, 10])
            .style(theme::text_field(p))
            .width(Length::Fill),
    ]
    .spacing(10)
    .into()
}
