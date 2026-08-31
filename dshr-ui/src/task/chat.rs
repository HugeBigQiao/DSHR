//! 任务页中栏：对话（标题条 + 消息流 + 椭圆伸缩输入框）。
//!
//! 键盘：Enter 发送 / Shift+Enter 换行（iced 的 key_binding 闭包完全接管按键，
//! 非 Enter 键委托默认 Binding::from_key_press 保持原有行为）。

use iced::widget::{Space, button, column, container, row, scrollable, text, text_editor};
use iced::{Color, Element, Length};

use crate::app::{ASSIST_BUBBLE, App, MUTED, TOOL_GREEN, USER_BUBBLE, WARN, card, fs};
use crate::message::Message;
use crate::task::MsgView;

/// 中栏：标题条（当前会话名 + 工作区入口）+ 消息流 + 输入框。
pub fn chat_area(app: &App) -> Element<'_, Message> {
    let mut messages = column![].spacing(8).padding(14);

    if app.task.selected.is_none() {
        messages = messages.push(
            text("点「开启新的任务」添加 runtime，会自动开会话，输入消息开始。")
                .size(fs(app, 13))
                .color(MUTED),
        );
    }

    for m in &app.task.messages {
        messages = messages.push(message_row(app, m));
    }

    column![
        chat_header(app),
        container(scrollable(messages).height(Length::Fill)).width(Length::Fill),
        input_box(app),
    ]
    .into()
}

/// 标题条：左侧 = 当前会话标题（跟随 session/title 自动更新），
/// 右侧 = 无工作区时「＋ 添加工作区」，有工作区时显示路径（只读）。
fn chat_header(app: &App) -> Element<'_, Message> {
    let selected = app.task.selected.as_ref().and_then(|sid| {
        app.task
            .runtimes
            .iter()
            .flat_map(|rt| &rt.sessions)
            .find(|s| &s.id == sid)
    });
    let title = selected
        .map(|s| s.title.clone())
        .unwrap_or_else(|| "对话".to_string());

    // 选中会话的归属 runtime（工作区操作定位用）。
    let active = app
        .task
        .active_runtime
        .as_ref()
        .and_then(|id| app.task.runtimes.iter().find(|rt| &rt.id == id));
    let has_workspace = active.and_then(|rt| rt.workspace.as_ref()).is_some();

    let right: Element<'_, Message> = if !has_workspace && active.is_some() {
        button("＋ 添加工作区")
            .on_press(Message::WorkspaceAdd)
            .padding([4, 10])
            .into()
    } else if has_workspace {
        let ws = active
            .and_then(|rt| rt.workspace.clone())
            .unwrap_or_default();
        text(ws).size(fs(app, 11)).color(MUTED).into()
    } else {
        Space::new().width(0).into()
    };

    row![
        text(title).size(fs(app, 15)),
        Space::new().width(Length::Fill),
        right,
    ]
    .padding([10, 16])
    .into()
}

/// 输入框：多行编辑器（text_editor）+ 右上角伸缩箭头 + 右侧发送按钮。
fn input_box(app: &App) -> Element<'_, Message> {
    let height = if app.task.input_expanded { 160.0 } else { 48.0 };
    let editor = text_editor(&app.task.input)
        .on_action(Message::InputAction)
        .key_binding(|press: iced::widget::text_editor::KeyPress| {
            use iced::keyboard::{self, key};
            use iced::widget::text_editor::Binding;
            // Enter：Shift+Enter 换行，裸 Enter 发送。
            if press.key == keyboard::Key::Named(key::Named::Enter) {
                if press.modifiers.shift() {
                    return Some(Binding::Enter);
                }
                return Some(Binding::Custom(Message::SendPressed));
            }
            // 其余按键交给默认绑定（光标移动/删除/粘贴等）。
            Binding::from_key_press(press)
        })
        .height(height)
        .padding(10)
        .placeholder("输入消息…（Enter 发送，Shift+Enter 换行）");

    // 伸缩箭头：普通态 ↗（拉高），展开态 ↘（复原）。
    let expand = button(if app.task.input_expanded {
        "↘"
    } else {
        "↗"
    })
    .on_press(Message::ToggleInputExpand)
    .padding([2, 8]);

    row![
        container(editor).style(card(None, 18.0)),
        column![
            expand,
            Space::new().height(Length::Fill),
            button("发送")
                .on_press(Message::SendPressed)
                .padding([6, 16]),
        ]
        .spacing(4),
    ]
    .spacing(8)
    .padding([10, 16])
    .into()
}

/// 渲染一行消息（气泡样式，去掉 [assistant]/[thinking] 前缀，靠对齐 + 颜色区分角色）。
fn message_row<'a>(app: &App, m: &'a MsgView) -> Element<'a, Message> {
    match m {
        MsgView::User(t) => {
            // 右对齐蓝色气泡（白字）。
            let bubble = container(text(t.clone()).size(fs(app, 14)).color(Color::WHITE))
                .padding([8, 14])
                .style(card(Some(USER_BUBBLE), 14.0));
            row![Space::new().width(Length::Fill), bubble].into()
        }
        MsgView::Assistant { text: t, reasoning } => {
            // 左对齐深色气泡（浅字），思考内容折叠显示在下方（小字灰色）。
            let mut col = column![
                container(
                    text(t.clone())
                        .size(fs(app, 14))
                        .color(Color::from_rgb(0.9, 0.92, 0.96))
                )
                .padding([8, 14])
                .style(card(Some(ASSIST_BUBBLE), 14.0))
            ]
            .spacing(4)
            .align_x(iced::alignment::Horizontal::Left);
            if let Some(r) = reasoning {
                col = col.push(text(format!("💭 {r}")).size(fs(app, 11)).color(MUTED));
            }
            col.into()
        }
        MsgView::Tool(t) => text(t.clone()).size(fs(app, 12)).color(TOOL_GREEN).into(),
        MsgView::TurnEnd(t) => {
            // 轮结束：居中分割线样式（小字灰色）。
            row![
                Space::new().width(Length::Fill),
                text(t.clone()).size(fs(app, 11)).color(MUTED),
                Space::new().width(Length::Fill),
            ]
            .into()
        }
        MsgView::Info(t) => text(t.clone()).size(fs(app, 13)).color(WARN).into(),
        // 对话内错误：红色字体。
        MsgView::Error(t) => text(t.clone())
            .size(fs(app, 13))
            .color(Color::from_rgb(0.92, 0.35, 0.35))
            .into(),
    }
}
