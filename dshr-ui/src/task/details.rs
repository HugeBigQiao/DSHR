//! 右侧预留：未来 turn rail / 详情（对标官方 details 栏：layer1 底；首版占位）。
use iced::widget::{container, text};
use iced::{Element, Length};

use crate::app::App;
use crate::task::Message;
use crate::theme;

/// 渲染右侧占位。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    container(
        text("右侧预留：turn rail / 详情（后续）")
            .size(app.fs(12))
            .color(p.label_caption),
    )
    .width(Length::FillPortion(1))
    .padding(10)
    .style(theme::surface(p, p.bg_layer1, 0.0))
    .into()
}
