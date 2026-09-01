//! 中间对话：消息流 + 统计行 + composer（对标官方 ChatView + StatsLine + composer dock）。
//! 消息形态：assistant 气泡（specific-bubble）、代码块底色（markdown-code-block）、
//! 工具卡片（border-l1 圆角）。composer 参考官方输入框：多行自动扩展高度、
//! 外面一个框、右下角圆形发送箭头、同行为模型/状态信息。
use iced::widget::{button, column, container, row, scrollable, text, text_editor, Space};
use iced::{Element, Length};

use crate::app::App;
use crate::model::{ChatState, MsgKind, MsgView, ToolView};
use crate::task::Message;
use crate::theme;

/// 渲染对话区。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let chat = &app.data.chat;
    let messages = chat.messages.iter().enumerate().fold(column![].spacing(8), |col, (i, m)| {
        col.push(render_message(app, chat, i, m))
    });
    let composer = container(column![
        // 多行编辑器：透明内层（无框），高度随内容扩展（min_height 兜底初始高度）；
        // 整个输入区只有外层 input_box 一个框。
        // Enter 发送：iced 0.14 的 text_editor 把 Enter 发布为 Action::Edit(Edit::Enter)，
        // 插入换行是在 App 收到 action 后 content.perform() 才执行的——拦截它转 Send，
        // 不执行 perform → 不插入换行。注：Edit::Enter 不携带 shift 信息（0.14 限制），
        // 所以 Shift+Enter 也会发送，多行文本请用中间换行。
        text_editor::TextEditor::new(&app.composer)
            .placeholder("输入消息…")
            .on_action(|action| match action {
                text_editor::Action::Edit(text_editor::Edit::Enter) => Message::Send,
                other => Message::ComposerEdit(other),
            })
            .style(theme::editor_flat(p))
            .height(Length::Shrink)
            .min_height(56.0)
            .padding(8),
        // 同一行：状态 / 模型信息 + 右下角圆形发送箭头。
        row![
            text(&chat.status).size(app.fs(11)).color(status_color(p, &chat.status)),
            text(&chat.stats).size(app.fs(11)).color(p.label_caption),
            Space::new().width(Length::Fill),
            button(text("↑").size(app.fs(15)).color(p.primary_btn_text))
                .on_press(Message::Send)
                .style(theme::circle_button(p))
                .padding([6, 7]),
        ]
        .align_y(iced::alignment::Vertical::Center),
    ])
    .padding(8)
    .style(theme::input_box(p, 12.0));
    let status = row![
        text(&chat.session_id).size(app.fs(10)).color(p.label_caption),
        text(&chat.status).size(app.fs(12)).color(status_color(p, &chat.status)),
        text(&chat.stats).size(app.fs(11)).color(p.label_caption),
    ]
    .spacing(10);
    container(column![
        scrollable(messages).width(Length::Fill).height(Length::Fill),
        status,
        composer,
    ])
    .width(Length::FillPortion(3))
    .padding(12)
    .into()
}

/// 状态文字颜色：running 强调蓝，idle 次要。
fn status_color(p: theme::Palette, status: &str) -> iced::Color {
    if status == "running" {
        p.accent
    } else {
        p.label_caption
    }
}

/// 按消息种类渲染（官方形态：名字行 + 内容/气泡/卡片）。
fn render_message<'a>(
    app: &'a App,
    _chat: &'a ChatState,
    index: usize,
    msg: &'a MsgView,
) -> Element<'a, Message> {
    let p = app.palette();
    match msg.kind {
        MsgKind::User => column![
            text("你").size(app.fs(11)).color(p.label_tertiary),
            text(&msg.text).size(app.fs(14)).color(p.label_primary),
            time(app, msg),
        ]
        .into(),
        MsgKind::Assistant => column![
            text("dsh").size(app.fs(11)).color(p.accent),
            container(text(&msg.text).size(app.fs(14)).color(p.label_primary))
                .width(Length::Fill)
                .padding(10)
                .style(theme::surface(p, p.bubble, 10.0)),
            time(app, msg),
        ]
        .into(),
        MsgKind::Reasoning => container(
            text(msg.reasoning.clone().unwrap_or_default())
                .size(app.fs(12))
                .color(p.label_tertiary),
        )
        .padding([2, 4])
        .into(),
        MsgKind::Tool => match &msg.tool {
            Some(tool) => tool_card(app, index, tool),
            None => container(text("(工具)")).into(),
        },
        MsgKind::Notice => container(text(&msg.text).size(app.fs(11)).color(p.label_caption))
            .padding([2, 4])
            .into(),
    }
}

/// 时间标签（caption 小字）。
fn time<'a>(app: &'a App, msg: &'a MsgView) -> Element<'a, Message> {
    text(&msg.time_label)
        .size(app.fs(10))
        .color(app.palette().label_caption)
        .into()
}

/// 工具摘要卡片（官方工具节点：l1 边框圆角 + 名称着色 + 展开）。
fn tool_card<'a>(app: &'a App, index: usize, tool: &'a ToolView) -> Element<'a, Message> {
    let p = app.palette();
    let color = if tool.is_error { p.error } else { p.success };
    let head = row![
        text(format!("🔧 {}", tool.name)).size(app.fs(13)).color(color),
        text(format!("{}ms", tool.duration_ms)).size(app.fs(11)).color(p.label_caption),
        button(text(if tool.expanded { "收起" } else { "展开" }).size(app.fs(11)))
            .on_press(Message::ToggleTool(index))
            .style(theme::ghost_button(p))
            .padding([2, 8]),
    ]
    .spacing(10);
    let mut col = column![head];
    if tool.expanded {
        col = col.push(text(&tool.summary).size(app.fs(12)).color(p.label_secondary));
    }
    container(col)
        .width(Length::Fill)
        .padding(8)
        .style(theme::bordered(p, 8.0))
        .into()
}
