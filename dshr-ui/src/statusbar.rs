//! 底部图标栏（Zed 风格 status bar）：矮行、全图标。底色与页面区分（statusbar_bg）。
//! 左侧：收起/展开侧边栏；随后是当前 runtime 名 + 状态（未启动时为空）。
//! 状态栏提示走这里：如 "Fake runtime（无 config.json）"（自动回落说明）。
use iced::widget::{Space, button, container, row, text};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::model::ChatStatus;
use crate::theme;

/// 渲染底部图标栏。
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
fn runtime_color(p: theme::Palette, status: ChatStatus) -> iced::Color {
    p.status_color(status)
}
