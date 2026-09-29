//! 右侧预留：未来 turn rail / 详情（对标官方 details 栏：layer1 底；首版占位）。
//!
//! 主要用途：占住三栏布局的右列（官方 rightbar 的位置），显示一句「后续」说明。
//! 为什么需要：三区的列宽分配（`FillPortion(1)`）必须有一个真实的 Element 参与，否则任务页
//! 右侧会被对话区吃掉；先放占位能固定布局骨架，将来换成 turn rail 只需替换本文件的 `view`。
//! 上接：`task::view`（三区横向拼装）。
//! 下接：`app::App`（只读调色板/字号）、`theme::surface`。
//! 官方对应：`packages/client/ui-sidebar-right/src/client/shell/SidebarRight.tsx` 与
//! `.../shell/RightbarRoot.tsx`（官方右侧栏轨道；dshr 目前只有空壳）。
use iced::widget::{container, text};
use iced::{Element, Length};

use crate::app::App;
use crate::task::Message;
use crate::theme;

/// 渲染右侧占位。
///
/// 入参/出参：`app` 提供调色板/字号基准；返回占一栏宽的 `Element<Message>`（无交互，不产生消息）。
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
