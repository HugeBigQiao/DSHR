//! 底部图标栏（Zed 风格 status bar）：矮行、全图标。底色与页面区分（statusbar_bg）。
//! 左侧：收起/展开侧边栏；随后是当前 runtime 名 + 状态（未启动时为空）。
//! 状态栏提示走这里：如 "Fake runtime（无 config.json）"（自动回落说明）。
//!
//! 主要用途：任务页底部的常驻信息条——侧边栏开关按钮、runtime 名（按状态着色）、
//! 以及 `chat.status_line`（未启动提示/停止原因/失败原因）。
//! 为什么需要：它是「没有对话框也能看到状态」的出口——engine 的 Started/Stopped/Failed 与
//! 启动期提示只能以文字形式存在，必须有地方展示；同时侧边栏收起后，这里是唯一能把它展开回来的
//! 入口（顶栏四页标签不含该功能）。它约束：只属于任务页（`app::view` 按页决定是否拼它）。
//! 上接：`app::App::view`（仅 `Page::Task`）。
//! 下接：`app::{App, Message::ToggleSidebar}`、`model::ChatStatus`、`theme`。
//! 官方对应：无对应控件——官方（`AppFrame.tsx` 注释明确「neither platform keeps an icon rail」）
//! 没有底部图标栏；侧边栏开关在官方是 `packages/client/ui-sidebar/src/client/SidebarRoot.tsx`
//! 的 `toggleSidebar` 按钮（桌面端置于 `HeaderLeadingControls.tsx`）。
use iced::widget::{Space, button, container, row, text};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::model::ChatStatus;
use crate::theme;

/// 渲染底部图标栏。
///
/// 为什么需要：见文件头——状态文字与侧边栏开关的唯一出口。
/// 入参/出参：`app` 提供 `data.runtimes`（取首个 runtime 名）、`data.chat.status/status_line`
/// 与调色板/字号；返回底部条 `Element<Message>`（未启动 runtime 时只显示侧边栏按钮）。
/// 主要功能：≡ 按钮发 `Message::ToggleSidebar`；runtime 名按其状态着色；状态行非空时补一小字。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let mut left = row![
        button(text("≡").size(app.fs(14)).color(p.label_secondary))
            .on_press(Message::ToggleSidebar)
            .style(theme::ghost_button(p))
            .padding([3, 10]),
    ]
    .spacing(10);
    // runtime 行：名字 + 会话状态色（s3 单 runtime；多 runtime 留 s4）。
    if let Some(rt) = app.data.runtimes.first() {
        left = left.push(
            text(&rt.name)
                .size(app.fs(11))
                .color(runtime_color(p, app.data.chat.status)),
        );
        if !app.data.chat.status_line.is_empty() {
            left = left.push(
                text(&app.data.chat.status_line)
                    .size(app.fs(10))
                    .color(p.label_caption),
            );
        }
    }
    container(row![left, Space::new().width(Length::Fill),])
        .padding([3, 8])
        .width(Length::Fill)
        .style(theme::surface(p, p.statusbar_bg, 0.0))
        .into()
}

/// runtime 名颜色：统一走 design 系统（theme.rs Palette::status_color）。
///
/// 为什么需要：状态色在三处共用，这里只做转发——避免底部条自己 match 状态取色而与别处漂移。
/// 入参/出参：`p` 调色板、`status` 当前会话状态；返回状态色。
fn runtime_color(p: theme::Palette, status: ChatStatus) -> iced::Color {
    p.status_color(status)
}
