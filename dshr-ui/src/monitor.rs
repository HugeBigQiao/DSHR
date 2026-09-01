//! 监控页：数据看板（M3 填充；先占位，样式走官方 token）。
use iced::widget::{column, container, text};
use iced::{Element, Length};

use crate::app::{App, Message};

/// 渲染监控页占位。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    container(column![
        text("监控（数据看板）").size(16).color(p.label_primary),
        text("token 算账 / 工具审计 / 会话树 — M3 填充")
            .size(12)
            .color(p.label_tertiary),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}
