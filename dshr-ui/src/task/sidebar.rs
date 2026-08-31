//! 任务页左栏：任务树（runtime + 会话 + "..."操作菜单）。

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length};

use dshr_state::UiStatus;

use crate::app::{ACCENT, App, MUTED, card, fs};
use crate::message::{MenuTarget, Message};

/// 左栏：顶部"开启新任务 +"（主色按钮）+ runtime/会话树。
pub fn sidebar(app: &App) -> Element<'_, Message> {
    let mut col = column![
        row![
            // 开启新任务（无工作区也行，决策 21）
            button(row![
                text("＋").size(fs(app, 15)),
                Space::new().width(6),
                text("开启新的任务").size(fs(app, 14)),
            ])
            .on_press(Message::AddPressed)
            .style(iced::widget::button::primary)
            .padding([8, 12])
            .width(Length::Fill),
        ],
        Space::new().height(6),
    ];

    for rt in &app.task.runtimes {
        col = col.push(runtime_block(app, rt));
        col = col.push(Space::new().height(6));
    }

    // 添加 runtime 弹窗
    if app.task.add_mode {
        col = col.push(
            container(
                column![
                    text("新任务").size(fs(app, 13)).color(MUTED),
                    text_input("名称（留空=新任务）", &app.task.name_input)
                        .on_input(Message::NameChanged)
                        .width(Length::Fill),
                    text_input("工作区路径（可选，设置后锁死）", &app.task.path_input)
                        .on_input(Message::PathChanged)
                        .width(Length::Fill),
                    row![
                        button("取消").on_press(Message::CancelAdd),
                        button("确定").on_press(Message::ConfirmAdd),
                    ]
                    .spacing(8),
                ]
                .spacing(6)
                .padding(10),
            )
            .style(card(None, 8.0)),
        );
    }

    // 补设工作区弹窗（对话页右上角 + 触发，这里渲染在树底部也行）
    if app.task.workspace_add {
        col = col.push(
            container(
                column![
                    text("设置工作区（设置后锁死）")
                        .size(fs(app, 13))
                        .color(MUTED),
                    text_input("工作区路径", &app.task.workspace_input)
                        .on_input(Message::WorkspaceChanged)
                        .width(Length::Fill),
                    row![
                        button("取消").on_press(Message::CancelWorkspace),
                        button("确定").on_press(Message::ConfirmWorkspace),
                    ]
                    .spacing(8),
                ]
                .spacing(6)
                .padding(10),
            )
            .style(card(None, 8.0)),
        );
    }

    col = col.push(Space::new().height(Length::Fill));
    scrollable(container(col).padding(10).width(250)).into()
}

/// 一个 runtime 块：标题行 + （可选展开菜单/改名）+ 会话列表。
fn runtime_block<'a>(app: &'a App, rt: &'a crate::task::RtView) -> Element<'a, Message> {
    let status_icon = match rt.status {
        UiStatus::Connecting => "⏳",
        UiStatus::Ready => "▶",
        UiStatus::Closed => "⏹",
    };
    // 点 runtime 标题：无会话 → 开会话；有会话 → 选中第一个。
    let header_action = if let Some(first) = rt.sessions.first() {
        Message::SelectSession(first.id.clone())
    } else {
        Message::NewSession(rt.id.clone())
    };

    let mut block = column![
        row![
            button(row![
                text(format!("{status_icon} {}", rt.name)).size(fs(app, 14)),
                Space::new().width(4),
                text(format!("（{}）", rt.sessions.len()))
                    .size(fs(app, 11))
                    .color(MUTED),
            ])
            .on_press(header_action)
            .padding(0),
            Space::new().width(Length::Fill),
            button("⋯").on_press(Message::ToggleMenu(MenuTarget::Runtime(rt.id.clone()))),
        ],
        text(
            rt.workspace
                .clone()
                .unwrap_or_else(|| "（未设置工作区）".to_string()),
        )
        .size(fs(app, 10))
        .color(MUTED),
    ]
    .spacing(2);

    // 展开的 runtime 菜单
    if app.task.menu.as_ref() == Some(&MenuTarget::Runtime(rt.id.clone())) {
        block = block.push(runtime_menu(app, rt));
    }
    // 内联改名（runtime）
    if let Some(r) = &app.task.renaming {
        if r.target == MenuTarget::Runtime(rt.id.clone()) {
            block = block.push(rename_row(&r.input));
        }
    }

    // 会话列表（缩进，选中项高亮背景 + 主题色文字）
    for s in &rt.sessions {
        let selected = app.task.selected.as_deref() == Some(s.id.as_str());
        let label = text(format!("• {}", s.title)).size(fs(app, 13));
        let label = if selected {
            label.color(ACCENT)
        } else {
            label.color(iced::Color::from_rgb(0.8, 0.82, 0.88))
        };
        let session_row = row![
            button(label)
                .on_press(Message::SelectSession(s.id.clone()))
                .padding([2, 4]),
            Space::new().width(Length::Fill),
            button("⋯").on_press(Message::ToggleMenu(MenuTarget::Session(s.id.clone()))),
        ];
        let mut s_col = column![session_row].spacing(2);
        if app.task.menu.as_ref() == Some(&MenuTarget::Session(s.id.clone())) {
            s_col = s_col.push(session_menu(app, s));
        }
        if let Some(r) = &app.task.renaming {
            if r.target == MenuTarget::Session(s.id.clone()) {
                s_col = s_col.push(rename_row(&r.input));
            }
        }
        // 选中项：给整行套一个淡色圆角背景。
        if selected {
            block = block.push(container(s_col).style(card(
                Some(iced::Color::from_rgba(0.36, 0.66, 0.99, 0.12)),
                6.0,
            )));
        } else {
            block = block.push(s_col);
        }
    }

    container(block)
        .padding(8)
        .width(Length::Fill)
        .style(card(None, 8.0))
        .into()
}

/// runtime 的操作菜单（改名/开会话/归档/删除）。
fn runtime_menu<'a>(app: &'a App, rt: &'a crate::task::RtView) -> Element<'a, Message> {
    menu_row(
        app,
        &[
            (
                "改名",
                Message::StartRename(MenuTarget::Runtime(rt.id.clone())),
            ),
            ("＋会话", Message::NewSession(rt.id.clone())),
            ("归档", Message::ArchiveRuntime(rt.id.clone())),
            ("删除", Message::DeleteRuntime(rt.id.clone())),
        ],
    )
}

/// 会话的操作菜单（改名/归档/删除）。
fn session_menu<'a>(app: &'a App, s: &'a crate::task::SessionView) -> Element<'a, Message> {
    menu_row(
        app,
        &[
            (
                "改名",
                Message::StartRename(MenuTarget::Session(s.id.clone())),
            ),
            ("归档", Message::ArchiveSession(s.id.clone())),
            ("删除", Message::DeleteSession(s.id.clone())),
        ],
    )
}

/// 一排小按钮菜单。
fn menu_row<'a>(app: &'a App, items: &[(&'static str, Message)]) -> Element<'a, Message> {
    let mut row = iced::widget::row![].spacing(4);
    for (label, msg) in items {
        row = row.push(
            button(text(*label).size(fs(app, 11)))
                .on_press(msg.clone())
                .padding([2, 8]),
        );
    }
    row.into()
}

/// 内联改名行（输入框 + 确定/取消）。
fn rename_row(input: &str) -> Element<'_, Message> {
    row![
        text_input("新名字", input)
            .on_input(Message::RenameChanged)
            .width(Length::Fill),
        button("确定").on_press(Message::ConfirmRename),
        button("取消").on_press(Message::CancelRename),
    ]
    .spacing(4)
    .into()
}
