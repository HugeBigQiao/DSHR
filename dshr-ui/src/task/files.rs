//! 任务页右栏：工作区文件树（点目录进入，点".."返回上级）。

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length};

use dshr_state::UiFileEntry;

use crate::app::{App, MUTED, card, fs};
use crate::message::Message;

/// 右栏：当前工作区目录的内容列表。
pub fn file_tree(app: &App) -> Element<'_, Message> {
    let mut col = column![
        text("文件").size(fs(app, 14)),
        // 面包屑：当前相对路径
        text(if app.task.file_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", app.task.file_path)
        })
        .size(fs(app, 11))
        .color(MUTED),
        Space::new().height(4),
    ]
    .spacing(4)
    .padding([8, 10]);

    // 返回上级（非根目录时）
    if !app.task.file_path.is_empty() {
        col = col.push(
            button(text(".. 上级目录").size(fs(app, 12)))
                .on_press(Message::FileUp)
                .padding([2, 4]),
        );
    }

    for e in &app.task.files {
        col = col.push(entry_row(app, e));
    }

    if app.task.files.is_empty() && app.task.file_path.is_empty() {
        col = col.push(text("（空目录）").size(fs(app, 11)).color(MUTED));
    }

    container(scrollable(col).width(220))
        .padding(6)
        .style(card(None, 8.0))
        .into()
}

/// 一个条目：目录可点进入，文件只显示。
fn entry_row<'a>(app: &'a App, e: &'a UiFileEntry) -> Element<'a, Message> {
    let icon = if e.is_dir { "📁" } else { "📄" };
    let label = format!("{icon} {}", e.name);
    if e.is_dir {
        let path = join_path(&app.task.file_path, &e.name);
        button(text(label).size(fs(app, 12)))
            .on_press(Message::FileOpen(path))
            .padding([2, 4])
            .into()
    } else {
        row![
            text(label).size(fs(app, 12)),
            Space::new().width(Length::Fill),
        ]
        .padding([2, 4])
        .into()
    }
}

/// 拼接相对路径（"a" + "b" → "a/b"）。
fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}
