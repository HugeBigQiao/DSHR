//! 监控页：数据看板（M3 填充；先占位，样式走官方 token）。
//!
//! 主要用途：占住第四个页面，显示一句「待填充」说明。
//! 为什么需要：顶栏已有「监控」标签（`Page::Monitor`），页面必须存在才能切过去不空白；
//! 它把目标写清楚（token 算账 / 工具审计 / 会话树，对应 DESIGN.md §8.3 的 read 聚合与 §12.1 的
//! M3.8），使实现时不必再回查设计文档。
//! 上接：`app::App::view`（`Page::Monitor` 分支）。
//! 下接：`app::App`（只读调色板/字号）。
//! 官方对应：无整页对应（官方无此页）——数据来源将来对齐
//! `packages/client/ui-chat/src/client/chat/TurnUsagePanel.tsx` 与 `.../chat/stat-dialog.ts`
//! 的 token/轮次统计口径。
use iced::widget::{column, container, text};
use iced::{Element, Length};

use crate::app::{App, Message};

/// 渲染监控页占位。
///
/// 入参/出参：`app` 提供调色板/字号；返回整页 `Element<Message>`（无交互，不产生消息）。
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
