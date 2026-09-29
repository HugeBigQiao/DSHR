//! 顶栏（Zed 风格）：左侧页面标签（任务/文件/监控/配置），右侧窗口控制（− □ ✕）。
//! 布局对齐官方设计系统：layer1 底 + 标签 active 填充 + 幽灵窗口按钮。
//!
//! 窗口图标用**文字字形**而非 canvas 自绘：canvas 版实测在 tiny-skia 后端下不出图
//! （画布拿到 0 尺寸 → 按钮成了隐形热区，顶栏右侧看起来空无一物）。字形取自
//! U+2212 / U+25A1 / U+2715，在 Noto Sans SC 下三者视觉重量接近。
//! 热区按桌面惯例（22×16，KDE/Windows 同档）。
//!
//! 主要用途：渲染整个窗口的顶栏——四个页面标签 + 拖动区 + 三个窗口控制按钮；
//! 它同时是无边框窗口的「窗框」（`decorations: false` 后这就是唯一可拖动/关闭的地方）。
//! 为什么需要：页面标签的选中态、窗口控制字形与拖动区三者共享同一行布局约束（行必须显式
//! `width(Fill)`，否则窗口按钮会挤在标签后而不是贴右缘；空白区必须是一个独立按钮才能拖动），
//! 单独成文件才能把「顶栏几何」讲清楚；`app.rs` 只负责把这些点击翻成 `Message`。
//! 它约束：窗口图标只能是文字字形（不能改回 canvas——见上）；标签文案属于产品文案。
//! 上接：`app::App::view`（任务页/非任务页都调它）。
//! 下接：`app::{Message, Page, WindowCmd}`、`theme::{nav_button, ghost_button, surface}`。
//! 官方对应：无整块对应——官方是浏览器内客户端，没有窗口控制；拖动区思路对应
//! `packages/client/web/src/window-drag/regions.ts` 的 `DRAG_MARK`/`isDraggableAt`，
//! 页面标签的角色对应 `ui-sidebar` 的 panel 导航（`SidebarRoot` 的 `selectPanel`/面板行）。
use iced::widget::{Space, button, container, row, text};
use iced::{Element, Length};

use crate::app::{App, Message, Page, WindowCmd};
use crate::theme;

/// 窗口图标种类（决定用哪个字形）。
///
/// 为什么需要：三个按钮的差异只有字形与目标命令，用枚举把「字形选择」与「热区/样式」分开，
/// 避免三处复制的按钮代码各写各的字号/对齐。
#[derive(Debug, Clone, Copy)]
enum WinIconKind {
    /// 最小化：U+2212 减号。
    Minimize,
    /// 最大化：U+25A1 空心方框。
    Maximize,
    /// 关闭：U+2715 叉。
    Close,
}

/// 渲染顶栏。
///
/// 为什么需要：见文件头——无边框窗口的窗框全靠这一个函数，且它同时承载页面导航。
/// 入参/出参：`app` 提供当前页（决定标签选中态）与调色板/字号；返回顶栏 `Element<Message>`。
/// 主要功能：四个标签（点 → `Message::Nav`）→ 一个撑满的空白按钮（点 → `WindowCmd::Drag`）
/// → 三个 22×16 的窗口控制按钮；整行 layer1 底色并垂直居中。
pub fn nav<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let tab = |label: &'static str, target: Page| {
        button(text(label).size(app.fs(13)))
            .on_press(Message::Nav(target))
            .style(theme::nav_button(p, app.page == target))
            .padding([6, 14])
    };
    // 窗口控制按钮：热区 22×16、图标字号 12（三个字形视觉重量接近：
    // U+2212 减号 / U+25A1 方框 / U+2715 叉）。
    //
    // 注意：原先用 canvas 自绘这三个图标，实测在 tiny-skia 后端下**完全不出图**
    // （画布拿到 0 尺寸，按钮成了隐形热区）。改用文字字形后渲染正常，
    // 这也是「顶栏右侧看不到关闭/最大化/最小化」的直接原因。
    let win = |kind: WinIconKind, cmd: WindowCmd| {
        let glyph = match kind {
            WinIconKind::Minimize => "\u{2212}",
            WinIconKind::Maximize => "\u{25a1}",
            WinIconKind::Close => "\u{2715}",
        };
        button(
            text(glyph)
                .size(12.0)
                .color(p.label_secondary)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center),
        )
        .on_press(Message::Window(cmd))
        .style(theme::ghost_button(p))
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(16.0))
        .padding([0, 0])
    };
    container(row![
        tab("任务", Page::Task),
        tab("\u{6587}\u{4ef6}", Page::Files),
        tab("监控", Page::Monitor),
        tab("配置", Page::Setting),
        // 空白区 = 无边框窗口拖动区（Zed 式：顶栏空白处按住拖动）。
        button(Space::new().width(Length::Fill))
            .on_press(Message::Window(WindowCmd::Drag))
            .style(theme::ghost_button(p))
            .height(Length::Fixed(16.0)),
        win(WinIconKind::Minimize, WindowCmd::Minimize),
        win(WinIconKind::Maximize, WindowCmd::Maximize),
        win(WinIconKind::Close, WindowCmd::Close),
    ])
    // 行必须显式撑满：Row 默认 Shrink，否则窗口控制按钮会挤在标签后面而不是贴右边缘。
    .width(Length::Fill)
    .align_y(iced::alignment::Vertical::Center)
    .padding([6, 8])
    .style(theme::surface(p, p.bg_layer1, 0.0))
    .into()
}
