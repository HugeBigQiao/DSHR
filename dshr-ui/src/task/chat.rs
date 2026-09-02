//! 中间对话：消息流 + 统计行 + composer（对标官方 ChatView + StatsLine + composer dock）。
//! s3：消息/统计全部来自真实快照（model.rs 映射）；工具卡展开状态在 App 按 seq 持有，
//! 快照整体刷新不丢。流式 token 渲染不在本步（folder 按 DESIGN §11.3 忽略 chunk）——
//! running 期间状态行以 "…" 示意。
use iced::widget::{Space, button, column, container, row, scrollable, text, text_editor};
use iced::{Element, Length};

use crate::app::App;
use crate::model::{ChatStatus, MsgKind, MsgView, short_id, stats_line};
use crate::task::Message;
use crate::theme;

/// 渲染对话区。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let chat = &app.data.chat;
    let messages = chat.messages.iter().fold(column![].spacing(8), |col, m| {
        col.push(render_message(app, m))
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
        // 同一行：状态 / 统计 + 右下角圆形发送箭头。
        row![
            text(status_text(chat.status))
                .size(app.fs(11))
                .color(status_color(p, chat.status)),
            text(stats_line(&chat.stats))
                .size(app.fs(11))
                .color(p.label_caption),
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
    // 底部一行小字：会话（短 id）+ 状态 + 补充说明（未启动提示/停止原因/失败原因）。
    let sid_label = if chat.session_id.is_empty() {
        "（无会话）".to_string()
    } else {
        format!("会话 {}", short_id(&chat.session_id))
    };
    let bottom = row![
        text(sid_label).size(app.fs(10)).color(p.label_caption),
        text(status_text(chat.status))
            .size(app.fs(12))
            .color(status_color(p, chat.status)),
        text(&chat.status_line)
            .size(app.fs(10))
            .color(p.label_caption),
    ]
    .spacing(10);
    container(column![
        scrollable(messages)
            .width(Length::Fill)
            .height(Length::Fill),
        bottom,
        composer,
    ])
    .width(Length::FillPortion(3))
    .padding(12)
    .into()
}

/// 状态文字：running 带 "…"（流式期间示意；token 级渲染不在本步）。
fn status_text(status: ChatStatus) -> String {
    match status {
        ChatStatus::Running => "running…".to_string(),
        other => other.label().to_string(),
    }
}

/// 状态颜色：统一走 design 系统（theme.rs Palette::status_color）。
fn status_color(p: theme::Palette, status: ChatStatus) -> iced::Color {
    p.status_color(status)
}

/// 按消息种类渲染（官方形态：名字行 + 内容/气泡/卡片）。
fn render_message<'a>(app: &'a App, msg: &'a MsgView) -> Element<'a, Message> {
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
            Some(tool) => tool_card(app, msg.seq, tool),
            None => container(text("(工具)")).into(),
        },
        MsgKind::Notice => container(text(&msg.text).size(app.fs(11)).color(p.label_caption))
            .padding([2, 4])
            .into(),
    }
}

/// 时间标签（caption 小字；快照映射时已格式化为 UTC HH:mm）。
fn time<'a>(app: &'a App, msg: &'a MsgView) -> Element<'a, Message> {
    text(&msg.time_label)
        .size(app.fs(10))
        .color(app.palette().label_caption)
        .into()
}

/// 工具摘要卡片（官方工具节点：l1 边框圆角 + 名称着色 + 展开；内容 = 快照 ToolItem：
/// result 摘要 + diffs 行数；展开态在 App 按 seq 持有，快照刷新不丢）。
fn tool_card<'a>(
    app: &'a App,
    seq: u64,
    tool: &'a dshr_state::snapshot::ToolItem,
) -> Element<'a, Message> {
    let p = app.palette();
    let color = if tool.is_error { p.error } else { p.success };
    let expanded = app.expanded_tools.contains(&seq);
    // 摘要信息：错误标 / 时长 / 文件变更行数。
    let mut meta = String::new();
    if tool.is_error {
        meta.push_str("失败 ");
    }
    if tool.duration_ms > 0 {
        meta.push_str(&format!("{}ms ", tool.duration_ms));
    }
    if !tool.diffs.is_empty() {
        let files = tool.diffs.len();
        let added: u64 = tool.diffs.iter().map(|d| d.added).sum();
        let removed: u64 = tool.diffs.iter().map(|d| d.removed).sum();
        meta.push_str(&format!("Δ+{added} −{removed} {files}文件"));
    }
    let head = row![
        text(format!("🔧 {}", tool.name))
            .size(app.fs(13))
            .color(color),
        text(meta).size(app.fs(11)).color(p.label_caption),
        Space::new().width(Length::Fill),
        button(text(if expanded { "收起" } else { "展开" }).size(app.fs(11)))
            .on_press(Message::ToggleTool(seq))
            .style(theme::ghost_button(p))
            .padding([2, 8]),
    ]
    .spacing(10)
    .align_y(iced::alignment::Vertical::Center);
    let mut col = column![head];
    if expanded {
        // 展开内容：调用参数摘要 + 结果摘要 + 逐文件 diff 行数（不求美，求信息可见）。
        if !tool.arguments.is_empty() {
            col = col.push(
                text(format!("参数 {}", tool.arguments))
                    .size(app.fs(11))
                    .color(p.label_caption),
            );
        }
        match &tool.result {
            Some(r) => col = col.push(text(r).size(app.fs(12)).color(p.label_secondary)),
            None => col = col.push(text("（运行中…）").size(app.fs(11)).color(p.label_caption)),
        }
        for d in &tool.diffs {
            col = col.push(
                text(format!("  +{} −{}  {}", d.added, d.removed, d.path))
                    .size(app.fs(11))
                    .color(p.label_caption),
            );
        }
    }
    container(col)
        .width(Length::Fill)
        .padding(8)
        .style(theme::bordered(p, 8.0))
        .into()
}
