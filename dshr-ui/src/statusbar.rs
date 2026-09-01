//! 底部图标栏（Zed 风格 status bar）：矮行、全图标。底色与页面区分（statusbar_bg）。
//! 当前一个按钮：收起/展开侧边栏。
use iced::widget::{button, container, row, text, Space};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::theme;

/// 渲染底部图标栏。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    container(row![
        button(text("≡").size(app.fs(14)).color(p.label_secondary))
            .on_press(Message::ToggleSidebar)
            .style(theme::ghost_button(p))
            .padding([3, 10]),
        Space::new().width(Length::Fill),
    ])
    .padding([3, 8])
    .width(Length::Fill)
    .style(theme::surface(p, p.statusbar_bg, 0.0))
    .into()
}
