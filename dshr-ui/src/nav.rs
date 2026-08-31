//! 顶部导航栏（任务 / 监控 / 配置 + 右侧当前 runtime 信息与退出）。

use iced::widget::{Space, button, row, text};
use iced::{Element, Length};

use crate::app::{App, MUTED, Page, fs};
use crate::message::Message;

/// 顶部导航栏（任务 / 监控 / 配置 + 右侧标题与退出）。
pub fn top_nav(app: &App) -> Element<'static, Message> {
    let current = app.page;
    let item = |icon: &'static str, label: &'static str, page: Page| {
        let active = current == page;
        let content = row![
            text(icon).size(fs(app, 14)),
            Space::new().width(6),
            text(label).size(fs(app, 13))
        ]
        .padding([6, 14]);
        button(content)
            .on_press(Message::Navigate(page))
            .style(if active {
                iced::widget::button::primary
            } else {
                iced::widget::button::secondary
            })
            .padding(0)
    };

    row![
        text("dshr").size(fs(app, 17)),
        Space::new().width(16),
        item("💬", "任务", Page::Task),
        item("📊", "监控", Page::Monitor),
        item("⚙", "配置", Page::Config),
        Space::new().width(Length::Fill),
        // 右侧：当前活跃 runtime 摘要（名字 · 工作区）。
        text(
            app.task
                .active_runtime
                .as_ref()
                .and_then(|id| app.task.runtimes.iter().find(|rt| &rt.id == id))
                .map(|rt| {
                    format!(
                        "{} · {}",
                        rt.name,
                        rt.workspace.clone().unwrap_or("无工作区".to_string())
                    )
                })
                .unwrap_or_default(),
        )
        .size(fs(app, 11))
        .color(MUTED),
        Space::new().width(12),
        button("退出").on_press(Message::Close),
    ]
    .padding([8, 12])
    .spacing(4)
    .width(Length::Fill)
    .into()
}
