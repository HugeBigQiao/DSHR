//! 中间对话：消息流 + 统计行 + composer（对标官方 ChatView + StatsLine + composer dock）。
//! s3：消息/统计全部来自真实快照（model.rs 映射）；工具卡展开状态在 App 按 seq 持有，
//! 快照整体刷新不丢。流式 token 渲染不在本步（folder 按 DESIGN.md §8.3 忽略 chunk）——
//! running 期间状态行以 "…" 示意。
//!
//! 主要用途：渲染对话区三块——可滚动的消息流（按 `MsgKind` 分形态）、composer（多行编辑器 +
//! 状态/统计 + 圆形发送箭头）、以及底部一行「会话短 id + 状态 + 状态行」。
//! 为什么需要：它是唯一把「消息种类 → 视觉形态」定下来的地方（用户行/assistant 气泡/
//! reasoning 灰字/工具卡/notice 小字），也是唯一处理「Enter 即发送」这条交互契约的地方
//! （见下方 composer 处的详细说明）；这些形态与 iced `text_editor` 的行为细节耦合，集中在一个
//! 文件里便于与官方 ChatView/StatsLine 逐条对齐。
//! 它约束：本文件不持有任何状态（全部读 `App`），交互靠发 `task::Message`；消息正文一个字都
//! 不改（快照里的文本直接上屏）。
//! 上接：`task::view`（中间列）。
//! 下接：`app::App`（`data.chat`/`composer`/`expanded_tools`/字号）、`model`（MsgView/ChatStatus/
//! stats_line/short_id）、`theme`、`dshr_state::snapshot::StreamSummary`。
//! 官方对应：`packages/client/ui-chat/src/client/chat/ChatView.tsx` 的 `ChatView`；
//! 消息行对应 `.../chat/MessageItem.tsx`、`.../chat/ReasoningRow.tsx`、工具节点
//! `.../conversation-nodes/tool.ts`；统计行对应 `.../chat/StatsPills.tsx`。
use iced::widget::{Space, button, column, container, row, scrollable, text, text_editor};
use iced::{Element, Length};

use crate::app::App;
use crate::model::{ChatStatus, MsgKind, MsgView, short_id, stats_line};
use crate::task::Message;
use crate::theme;
use dshr_state::snapshot::StreamSummary;

/// 渲染对话区。
///
/// 为什么需要：对话区自上而下的顺序（消息流占满剩余高度 → 底部状态行 → composer 固定在下）
/// 与「消息流可滚动、其余不滚」的分工只在这里表达；composer 的 Enter 拦截也在这里（见下方注释）。
/// 入参/出参：`app` 提供 `data.chat`（消息/统计/状态/标题）、`composer`（草稿）与字号/调色板；
/// 返回中间列 `Element<Message>`。
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
///
/// 为什么需要：`ChatStatus::label()` 是各处共用的短标签，但对话区需要额外表达「正在产出」，
/// 所以在此加后缀而不改 `label()`（底部图标栏等处应保持干净短标签）。
/// 入参/出参：`status` 为当前状态；返回展示文案。
fn status_text(status: ChatStatus) -> String {
    match status {
        ChatStatus::Running => "running…".to_string(),
        other => other.label().to_string(),
    }
}

/// 状态颜色：统一走 design 系统（theme.rs Palette::status_color）。
///
/// 为什么需要：状态色要在对话区/侧边栏状态点/底部图标栏三处一致，所以只做一次转发，
/// 避免各页各自 match 状态取色（那种分散迟早会不一致）。
/// 入参/出参：`p` 为调色板、`status` 为当前状态；返回状态色。
fn status_color(p: theme::Palette, status: ChatStatus) -> iced::Color {
    p.status_color(status)
}

/// 按消息种类渲染（官方形态：名字行 + 内容/气泡/卡片）。
///
/// 为什么需要：`MsgKind` 是快照的判别式，视觉形态必须与它一一对应；把这个 match 写在唯一处，
/// 消息流才不会有「同一个 kind 两种长相」。
/// 入参/出参：`app` 提供调色板/字号/工具卡展开集，`msg` 为该行视图模型；返回该行
/// `Element<Message>`。Tool 行缺 `tool` 内容时退化为「(工具)」占位（不会 panic）。
fn render_message<'a>(app: &'a App, msg: &'a MsgView) -> Element<'a, Message> {
    let p = app.palette();
    match msg.kind {
        MsgKind::User => column![
            text("你").size(app.fs(11)).color(p.label_tertiary),
            text(&msg.text).size(app.fs(14)).color(p.label_primary),
            time(app, msg),
        ]
        .into(),
        MsgKind::Assistant => {
            let mut col = column![
                text("dsh").size(app.fs(11)).color(p.accent),
                container(text(&msg.text).size(app.fs(14)).color(p.label_primary))
                    .width(Length::Fill)
                    .padding(10)
                    .style(theme::surface(p, p.bubble, 10.0)),
            ];
            if let Some(stream) = &msg.stream {
                col = col.push(stream_caption(app, stream));
            }
            col.push(time(app, msg)).into()
        }
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
        // 下面两种**默认不出现在视图里**（`model.rs::apply_snapshot` 已过滤掉；数据仍在快照/库/导出里）。
        // 这里仍给出合理长相，而不是 `unreachable!()`：一旦将来加了「显示注入/尝试」开关，
        // 或过滤被误删，它们会以折叠小字出现，而不是让界面 panic。
        MsgKind::Injected => container(
            text(format!("[{}] {}", msg.source, msg.text))
                .size(app.fs(10))
                .color(p.label_tertiary),
        )
        .padding([2, 4])
        .into(),
        MsgKind::Attempt => container(
            text("[模型尝试（未提交）]")
                .size(app.fs(10))
                .color(p.label_tertiary),
        )
        .padding([2, 4])
        .into(),
    }
}

/// 时间标签（caption 小字；快照映射时已格式化为 UTC HH:mm）。
///
/// 入参/出参：`app` 只提供字号/调色板，`msg` 提供已格式化好的 `time_label`；返回小字 Element。
fn time<'a>(app: &'a App, msg: &'a MsgView) -> Element<'a, Message> {
    text(&msg.time_label)
        .size(app.fs(10))
        .color(app.palette().label_caption)
        .into()
}

/// v3 流记录摘要（首 token 延迟 / 时长 / 文本量）；真正的逐 token 直播需要上游 live 通知。
///
/// 为什么需要：逐 chunk 内容按 DESIGN.md §8.3 不保留，但「这次流有多长、首 token 等多久」
/// 是排障/体感的关键信息，所以用一行摘要替代逐 token 渲染。
/// 入参/出参：`stream` 为快照里的流摘要；返回由 ` · ` 连接的单行小字
/// （各段只在数据存在时出现——缺时间戳就只显示 chunk 数）。
fn stream_caption<'a>(app: &'a App, stream: &StreamSummary) -> Element<'a, Message> {
    let p = app.palette();
    let mut parts = vec![format!("流记录 {} chunks", stream.chunks)];
    if let (Some(first), Some(token)) = (stream.first_time, stream.first_token_time) {
        parts.push(format!("首 token +{}ms", token.saturating_sub(first)));
    }
    if let (Some(first), Some(last)) = (stream.first_time, stream.last_time) {
        parts.push(format!("时长 {}ms", last.saturating_sub(first)));
    }
    if stream.text_chars > 0 {
        parts.push(format!("text {} chars", stream.text_chars));
    }
    if stream.reasoning_chars > 0 {
        parts.push(format!("reasoning {} chars", stream.reasoning_chars));
    }
    if stream.tool_args_chars > 0 {
        parts.push(format!("tool args {} chars", stream.tool_args_chars));
    }
    text(parts.join(" · "))
        .size(app.fs(10))
        .color(p.label_caption)
        .into()
}

/// 工具摘要卡片（官方工具节点：l1 边框圆角 + 名称着色 + 展开；内容 = 快照 ToolItem：
/// result 摘要 + diffs 行数；展开态在 App 按 seq 持有，快照刷新不丢）。
///
/// 为什么需要：工具调用是长对话里最多的行，默认收起只留「名称 + 失败标 + 耗时 + Δ 行数」，
/// 既能让用户扫读，也能在出错时一眼看到红色；展开态存 App（按 seq 索引）而不是本地，是因为
/// 快照每事件整份替换、行会被重建。
/// 入参/出参：`seq` 为该行稳定序号（展开集索引）、`tool` 为快照里配对好的工具项
/// （result 为 None = 仍在运行）；返回工具卡 `Element<Message>`（按钮发 `ToggleTool(seq)`）。
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
