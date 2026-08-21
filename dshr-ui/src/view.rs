//! 渲染层：把 App 状态画成 Element（纯渲染，不修改状态）。
//!
//! 依赖 app.rs 的 App（读字段），更新逻辑在 app.rs，这里只"画"。

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length};

use dshr_state::ui::UiStatus;

use crate::app::App;
use crate::message::Message;
use crate::model::MsgView;

/// 根视图：boot_error 全屏提示，否则左右分栏。
pub fn view(app: &App) -> Element<'_, Message> {
    if let Some(err) = &app.boot_error {
        return container(text(format!("启动失败：{err}")).size(18))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    }

    row![sidebar(app), chat_area(app)].into()
}

/// 左侧边栏：runtime 树 + 添加按钮。
fn sidebar(app: &App) -> Element<'_, Message> {
    let mut col = column![
        text("Runtimes").size(16),
        button("＋ 添加 Runtime").on_press(Message::AddPressed),
        Space::new().height(8),
    ];

    for rt in &app.runtimes {
        let status_icon = match rt.status {
            UiStatus::Connecting => "⏳",
            UiStatus::Ready => "▶",
            UiStatus::Closed => "⏹",
        };
        let mut rt_col = column![
            row![
                text(format!("{status_icon} {}", rt.name)).size(14),
                Space::new().width(Length::Fill),
                button("＋会话").on_press(Message::NewSession(rt.id.clone())),
                button("🗑").on_press(Message::ArchiveRuntime(rt.id.clone())),
            ],
            text(rt.workspace.clone())
                .size(11)
                .color(iced::Color::from([0.5, 0.5, 0.5])),
        ]
        .spacing(2);

        for s in &rt.sessions {
            let selected = app.selected.as_deref() == Some(s.id.as_str());
            let mut label = text(format!("• {}", s.title)).size(13);
            if selected {
                label = label.color(iced::Color::from([0.2, 0.6, 1.0]));
            }
            rt_col = rt_col.push(
                button(label)
                    .on_press(Message::SelectSession(s.id.clone()))
                    .padding([2, 8]),
            );
        }
        col = col.push(container(rt_col).padding(8));
    }

    // 底部：退出（正常收尾：shutdown 全部 runtime）。
    col = col.push(Space::new().height(Length::Fill));
    col = col.push(button("退出").on_press(Message::Close));

    scrollable(container(col).padding(10).width(260)).into()
}

/// 右侧聊天区：消息流 + 输入框。
fn chat_area(app: &App) -> Element<'_, Message> {
    let mut messages = column![].spacing(6).padding(10);

    if app.selected.is_none() {
        messages = messages.push(
            text("添加 runtime 后会自动开会话，输入消息开始。")
                .color(iced::Color::from([0.5, 0.5, 0.5])),
        );
    }

    for m in &app.messages {
        let line: Element<'_, Message> = match m {
            MsgView::User(t) => text(format!("[user] {t}")).into(),
            MsgView::Assistant { text: t, reasoning } => {
                let think: Element<'_, Message> = if let Some(r) = reasoning {
                    text(format!("[thinking] {r}"))
                        .size(11)
                        .color(iced::Color::from([0.5, 0.5, 0.6]))
                        .into()
                } else {
                    Space::new().into()
                };
                column![think, text(format!("[assistant] {t}"))].into()
            }
            MsgView::Tool(t) => text(t.clone())
                .size(12)
                .color(iced::Color::from([0.4, 0.6, 0.4]))
                .into(),
            MsgView::TurnEnd(t) => text(t.clone())
                .size(11)
                .color(iced::Color::from([0.5, 0.5, 0.5]))
                .into(),
            MsgView::Info(t) => text(t.clone())
                .color(iced::Color::from([0.9, 0.5, 0.3]))
                .into(),
        };
        messages = messages.push(line);
    }

    let input_row = row![
        text_input("输入消息…", &app.input)
            .on_input(Message::InputChanged)
            .width(Length::Fill),
        button("发送").on_press(Message::SendPressed),
    ]
    .spacing(8)
    .padding(10);

    let add_row = if app.add_mode {
        let r: Element<'_, Message> = row![
            text_input("名称", &app.name_input)
                .on_input(Message::NameChanged)
                .width(Length::FillPortion(1)),
            text_input("工作区路径", &app.path_input)
                .on_input(Message::PathChanged)
                .width(Length::FillPortion(2)),
            button("确定").on_press(Message::ConfirmAdd),
            button("取消").on_press(Message::CancelAdd),
        ]
        .spacing(8)
        .padding(10)
        .into();
        Some(r)
    } else {
        None
    };

    let mut chat = column![scrollable(messages).height(Length::Fill), input_row,];
    if let Some(r) = add_row {
        chat = chat.push(r);
    }
    chat.into()
}
