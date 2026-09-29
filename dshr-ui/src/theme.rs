//! 官方设计系统（deepseek-harness packages/client/ui-theme/src/styles/design-platform.css）。
//!
//! 语义名对齐 `--dsw-alias-*`；深浅两套 palette。所有自定义控件样式从这里构建——
//! 页面代码不出现裸色值。字体对齐官方：系统栈（Segoe UI / PingFang SC / Microsoft YaHei…）。
//!
//! 主要用途：提供 `Palette`（颜色 token）+ 一组 `impl Fn(&Theme) -> Style` 的样式闭包
//! （容器/按钮/输入框/编辑器/导航项），页面把它们直接交给 iced 的 `.style(..)`。
//! 为什么需要：官方 token 是单一真源，页面里出现裸色值就无法整体换肤/对齐官方；把 token 与
//! 闭包集中在一个文件，才能保证「深浅两套都齐」「同一个控件在各页长得一样」。它约束：
//! 页面代码不得写死颜色，只能经 `app.palette()` + 本文件的样式函数取值（少数官方契约字段
//! 已声明但暂未接线，见 `#[allow(dead_code)]`）。
//! 上接：`app::App::{palette, theme}`；被 `nav/statusbar/task/files/setting/widgets` 全部使用。
//! 下接：iced 的 `container/button/text_editor/text_input::Style` 与 `Theme`。
//! 官方对应：`packages/client/ui-theme/src/styles/design-platform.css` 的 `--dsw-alias-*`
//! 变量（bg-base / layer-1..3 / label-* / border-l1..l2 / state-business-primary / bg-mask-1 …），
//! 另参考 `packages/client/ui-primitives/src/Button.tsx` 的按钮层级（primary/secondary/ghost）。
use iced::widget::{button, container, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

/// 设计系统调色板（深浅两套，语义名对齐官方 --dsw-alias-*）。
/// 部分字段为官方 token 契约，待对应功能（markdown 代码块/警告/层级）接入后使用。
///
/// 为什么需要：把官方 CSS 变量表翻译成编译期字段，页面就能按语义名取色（`p.border_l1`）
/// 而不是记住 rgb 值；`Copy` 让样式闭包随手捕获一份，不必传引用。
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
///
/// 为什么需要：`bg` 随调用点变化（页面底/气泡/分隔线），但「纯色底 + 圆角」这个形状反复出现；
/// 用参数化闭包避免为每种底色各写一个函数。入参：`bg` 底色、`radius` 圆角；无 `p` 参与
/// （保留首参以与其它样式函数同签名，便于统一取用）。返回 iced 容器样式闭包。
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
///
/// 为什么需要：工具卡片、`config.json` 标签等需要「只有描边、没有底色」的卡片；底色留空
/// 才能透出所在层的背景（气泡内 / 侧边栏内都不串色）。
/// 入参：`p` 取边框色、`radius` 圆角；返回容器样式闭包。
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
///
/// 为什么需要：composer 用「外层容器画框、内层 text_editor 全透明」的两层做法（见
/// `editor_flat`）——把焦点边框与圆角交给这一层，才能让多行编辑器随内容长高而不破框。
/// 入参：`p` 取底色/边框色、`radius` 圆角；返回容器样式闭包。
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
///
/// 为什么需要：发送/保存这类「页面上唯一的主操作」需要与幽灵按钮明显区分；深色下是白底深字、
/// 浅色下是深底白字，因此底色必须从 `Palette` 取（`primary_btn_bg`/`primary_btn_text` 在深浅
/// 两套里互为反转）。
/// 入参：`p` 提供底色与字色；返回按钮样式闭包（iced 传入 `button::Status`）。
/// 注：当前 Hovered 与默认分支取同一底色（未做亮化差异）。
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
///
/// 为什么需要：顶栏标签/侧边栏行/窗口控制/菜单项都要「平时无存在感、指到才有反馈」；
/// 透明底让它们叠在任意层底色上都成立。
/// 入参：`p` 取 hover 填充与字色；返回按钮样式闭包。
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
///
/// 为什么需要：composer 右下角的发送箭头是页面上唯一的品牌色实心按钮（`radius: 999` 恒定圆形，
/// 不随尺寸变化）；accent 是本项目里「可以点」的最强语义。
/// 入参：`p` 取 accent/accent_hover 与字色；返回按钮样式闭包（Hovered 时用 `accent_hover`）。
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
///
/// 为什么需要：侧边栏的行尾操作槽位常显（占位不跳动），但视觉上必须与行背景一致、
/// 且忽略任何 `Status`（否则鼠标扫过时槽位会闪一个方块，行 hover 高亮已经给了反馈）。
/// 入参：`p` 只取字色；返回按钮样式闭包（完全忽略 `Status`）。
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
///
/// 为什么需要：composer 的高度随内容扩展，若编辑器自带宽高与边框，扩展时会与外层框错位；
/// 让它只负责文字/光标/选区颜色，形状完全交给外层容器。
/// 入参：`p` 取 placeholder/value/selection 色；返回 `text_editor` 样式闭包（忽略 `Status`，
/// 即聚焦不加边框——焦点由外层容器表达）。
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
///
/// 为什么需要：顶栏标签、配置页分区、文件树选中行都需要「选中态压过 hover 态」的三态样式；
/// `active` 用参数而不是 `Status`，是因为选中与鼠标位置正交（选中的项在鼠标移开时必须保持高亮）。
/// 入参：`p` 取填充/文字色，`active` 表示当前项是否选中；返回按钮样式闭包。
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
///
/// 为什么需要：配置页表单需要「聚焦即高亮」的可编辑反馈，而 iced 的 `text_input::Status`
/// 只有在样式闭包里才可见——所以焦点边框必须在这里算。
/// 入参：`p` 取底色/边框/accent 与文字色；返回 `text_input` 样式闭包（`Focused` 用 accent 描边）。
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

/// 8 位 RGB → `Color`（调色板表里的简写助手）。
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// 8 位 RGB + alpha → `Color`（边框/遮罩这类半透明 token 用）。
fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}
