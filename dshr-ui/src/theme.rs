//! 官方设计系统（deepseek-harness packages/client/ui-theme/src/styles/design-platform.css）。
//!
//! 语义名对齐 `--dsw-alias-*`；深浅两套 palette。所有自定义控件样式从这里构建——
//! 页面代码不出现裸色值。字体对齐官方：系统栈（Segoe UI / PingFang SC / Microsoft YaHei…）。
use iced::widget::{button, container, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

/// 设计系统调色板（深浅两套，语义名对齐官方 --dsw-alias-*）。
/// 部分字段为官方 token 契约，待对应功能（markdown 代码块/警告/层级）接入后使用。
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Palette {
    /// 页面底色（bg-base）：深 bluish-950 / 浅 bluish-50。
    pub bg_base: Color,
    /// 分层表面（layer-1/2/3，越上层越亮）。
    pub bg_layer1: Color,
    pub bg_layer2: Color,
    pub bg_layer3: Color,
    /// 侧边栏底色（sidebar-fill）。
    pub sidebar_fill: Color,
    /// 文字（label-primary / secondary / tertiary / caption）。
    pub label_primary: Color,
    pub label_secondary: Color,
    pub label_tertiary: Color,
    pub label_caption: Color,
    /// 边框（border-l1 细分 / l2 明显）。
    pub border_l1: Color,
    pub border_l2: Color,
    /// 品牌强调（state-business-primary，deepseek 蓝）。
    pub accent: Color,
    pub accent_hover: Color,
    /// 聊天气泡底 / 代码块底（markdown-code-block）。
    pub bubble: Color,
    pub code_block: Color,
    /// 侧边栏导航项（active / hover 填充）。
    pub nav_active: Color,
    pub nav_hover: Color,
    /// 交互 hover（interactive-bg-hover）。
    pub interactive_hover: Color,
    /// 状态色（success / error / warn）。
    pub success: Color,
    pub error: Color,
    pub warn: Color,
    /// 主按钮（白底深字 = brand-primary 反转）。
    pub primary_btn_bg: Color,
    pub primary_btn_text: Color,
    /// 模态遮罩（bg-mask-1）。
    pub mask: Color,
    /// 底部图标栏底色（与页面底色区分：浅色下用灰）。
    pub statusbar_bg: Color,
}

impl Palette {
    /// 深色（默认，对齐 `body[data-ds-dark-theme]`）。
    pub fn dark() -> Self {
        Self {
            bg_base: rgb(21, 21, 23),
            bg_layer1: rgb(35, 35, 36),
            bg_layer2: rgb(44, 44, 46),
            bg_layer3: rgb(53, 54, 56),
            sidebar_fill: rgb(27, 27, 28),
            label_primary: rgb(249, 250, 251),
            label_secondary: rgb(207, 211, 214),
            label_tertiary: rgb(173, 178, 184),
            label_caption: rgb(129, 133, 140),
            border_l1: rgba(255, 255, 255, 0.06),
            border_l2: rgba(255, 255, 255, 0.12),
            accent: rgb(103, 158, 254),
            accent_hover: rgb(65, 118, 230),
            bubble: rgb(44, 44, 46),
            code_block: rgb(27, 27, 28),
            nav_active: rgb(67, 69, 74),
            nav_hover: rgb(44, 44, 46),
            interactive_hover: rgba(255, 255, 255, 0.08),
            success: rgb(34, 197, 94),
            error: rgb(242, 90, 90),
            warn: rgb(245, 158, 11),
            primary_btn_bg: rgb(249, 250, 251),
            primary_btn_text: rgb(15, 17, 21),
            mask: rgba(0, 0, 0, 0.5),
            statusbar_bg: rgb(35, 35, 36),
        }
    }

    /// 浅色（对齐 body 默认）。
    pub fn light() -> Self {
        Self {
            bg_base: rgb(249, 250, 251),
            bg_layer1: rgb(249, 250, 251),
            bg_layer2: rgb(249, 250, 251),
            bg_layer3: rgb(249, 250, 251),
            sidebar_fill: rgb(249, 250, 251),
            label_primary: rgb(15, 17, 21),
            label_secondary: rgb(97, 102, 107),
            label_tertiary: rgb(129, 133, 140),
            label_caption: rgb(151, 157, 166),
            border_l1: rgba(0, 0, 0, 0.04),
            border_l2: rgba(0, 0, 0, 0.1),
            accent: rgb(65, 118, 230),
            accent_hover: rgb(37, 99, 235),
            bubble: rgb(249, 250, 251),
            code_block: rgb(250, 250, 250),
            nav_active: rgb(235, 238, 242),
            nav_hover: rgb(241, 243, 245),
            interactive_hover: rgba(38, 49, 72, 0.06),
            success: rgb(34, 197, 94),
            error: rgb(236, 19, 19),
            warn: rgb(245, 158, 11),
            primary_btn_bg: rgb(15, 17, 21),
            primary_btn_text: rgb(249, 250, 251),
            mask: rgba(0, 0, 0, 0.24),
            statusbar_bg: rgb(235, 238, 242),
        }
    }

    /// 按深浅开关取调色板。
    pub fn pick(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// 生命周期状态色（对话状态行 / 侧边栏状态点 / 底部图标栏共用）：
    /// running 强调蓝、stopped warn（用户停止，不算错误）、failed 错误红、其余 caption 灰。
    pub fn status_color(&self, status: crate::model::ChatStatus) -> Color {
        use crate::model::ChatStatus;
        match status {
            ChatStatus::Running => self.accent,
            ChatStatus::Stopped => self.warn,
            ChatStatus::Failed => self.error,
            ChatStatus::Off | ChatStatus::Idle => self.label_caption,
        }
    }
}

/// 圆角表面容器（官方卡片/面板：背景 + 圆角）。
pub fn surface(_p: Palette, bg: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// 带边框的容器（官方分隔/工具卡片：l1 细边框 + 圆角）。
pub fn bordered(p: Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        border: Border {
            radius: radius.into(),
            color: p.border_l1,
            width: 1.0,
        },
        ..container::Style::default()
    }
}

/// 输入框容器（官方 input-major：layer2 底 + l2 边框 + 圆角）。
pub fn input_box(p: Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg_layer2)),
        border: Border {
            radius: radius.into(),
            color: p.border_l2,
            width: 1.0,
        },
        ..container::Style::default()
    }
}

/// 主按钮（官方 primary：白底深字 / 深底白字，hover 微亮）。
pub fn primary_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => p.primary_btn_bg,
            _ => p.primary_btn_bg,
        })),
        text_color: p.primary_btn_text,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 幽灵按钮（官方 secondary：透明底，hover 交互填充）。
pub fn ghost_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => p.nav_hover,
            _ => Color::TRANSPARENT,
        })),
        text_color: p.label_secondary,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 圆形强调按钮（官方发送箭头：accent 底 + 圆角拉满，hover 微亮）。
pub fn circle_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => p.accent_hover,
            _ => p.accent,
        })),
        text_color: p.primary_btn_text,
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 纯文字按钮（无背景无边框无 hover 填充——行尾 ⋯/+ 用；悬停行时才出现，和背景同色）。
pub fn plain_button(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: None,
        text_color: p.label_tertiary,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 透明编辑器样式（composer 内层：无框无背景——外层 input_box 是唯一框）。
pub fn editor_flat(p: Palette) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, _| text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: p.label_caption,
        value: p.label_primary,
        selection: p.accent,
    }
}

/// 导航项按钮（侧边栏/分区导航：hover 填充、active 深一档）。
pub fn nav_button(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(if active {
            p.nav_active
        } else {
            match status {
                button::Status::Hovered => p.nav_hover,
                _ => Color::TRANSPARENT,
            }
        })),
        text_color: if active {
            p.label_primary
        } else {
            p.label_secondary
        },
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// 文本输入框（Zed 式：layer2 底 + 圆角 + focus 时 accent 边框）。
pub fn text_field(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: Background::Color(p.bg_layer2),
        border: Border {
            radius: 8.0.into(),
            color: match status {
                text_input::Status::Focused { .. } => p.accent,
                _ => p.border_l2,
            },
            width: 1.0,
        },
        icon: p.label_caption,
        placeholder: p.label_caption,
        value: p.label_primary,
        selection: p.accent,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}
