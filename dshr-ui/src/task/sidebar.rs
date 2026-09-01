//! 左侧边栏：runtime 树（每个 runtime = 一个 dsh 子进程）+ 会话缩进层级。
//!
//! 交互（Zed 风格）：
//! - 行尾 ⋯ / runtime 行 +：**常显**（纯文字，和背景同色）；
//! - 点击 ⋯ → 覆盖式菜单（Popover 悬浮在标签上，右对齐向下展开，官方形态）；
//! - hover 或选中时整行显示灰色框；当前选中会话常显灰框；
//! - runtime 行可 + / − 收起会话列表；顶部「新建 runtime」居中。
use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Length};

use crate::app::App;
use crate::model::RuntimeView;
use crate::task::Message;
use crate::theme;
use crate::widgets::popover::Popover;

/// ⋯/+ 按钮位宽度（对齐占位）。
const SLOT: f32 = 30.0;

/// 渲染 runtime 树。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    // 强制居中：按钮内容 = 左右对称 Space。
    let new_rt = button(
        row![
            Space::new().width(Length::Fill),
            text("＋ 新建 runtime").size(app.fs(13)),
            Space::new().width(Length::Fill),
        ],
    )
    .on_press(Message::NewRuntime)
    .style(theme::primary_button(p))
    .padding([8, 12])
    .width(Length::Fill);
    let tree = app
        .data
        .runtimes
        .iter()
        .fold(column![].spacing(4), |col, rt| col.push(runtime_block(app, rt)));
    container(column![
        new_rt,
        // 新建按钮与下方列表的间隔（上下）。
        Space::new().height(8),
        scrollable(tree).width(Length::Fill).height(Length::Fill),
    ])
    .width(Length::FillPortion(1))
    .padding(10)
    .style(theme::surface(p, p.sidebar_fill, 0.0))
    .into()
}

/// 一个 runtime 块：行（名称 + 展开 + ⋯ 覆盖菜单）+ 缩进会话。
fn runtime_block<'a>(app: &'a App, rt: &'a RuntimeView) -> Element<'a, Message> {
    let p = app.palette();
    let hovered = app.hover.as_ref() == Some(&(rt.id.clone(), None));
    let menu_open = app.menu.as_ref() == Some(&(rt.id.clone(), None));

    let rt_row = themed_row(
        if hovered { Some(p.interactive_hover) } else { None },
        row![
            text(&rt.name).size(app.fs(13)).color(p.label_primary),
            Space::new().width(Length::Fill),
            slot(
                app,
                Message::ToggleRuntimeExpand(rt.id.clone()),
                if rt.expanded { "−" } else { "+" },
                None,
            ),
            slot(
                app,
                Message::MenuToggle(rt.id.clone(), None),
                "⋯",
                menu_open.then(|| {
                    menu_items(vec![
                        ("＋ 新建 runtime", Message::NewRuntime),
                        ("删除 runtime", Message::DeleteRuntime(rt.id.clone())),
                        ("归档 runtime", Message::ArchiveRuntime(rt.id.clone())),
                    ])
                }),
            ),
        ]
        .align_y(iced::alignment::Vertical::Center)
        .into(),
        Message::Hover(Some((rt.id.clone(), None))),
        Message::Hover(None),
    );

    let sessions = if rt.expanded {
        rt.sessions.iter().fold(column![].spacing(1), |scol, s| {
            scol.push(session_row(app, rt, s))
        })
    } else {
        column![]
    };

    column![rt_row, sessions].spacing(8).into()
}

/// 一个会话行（缩进；hover 或选中时灰框；⋯ 覆盖菜单）。
fn session_row<'a>(
    app: &'a App,
    rt: &'a RuntimeView,
    s: &'a crate::model::SessionView,
) -> Element<'a, Message> {
    let p = app.palette();
    let selected = rt.selected_session.as_deref() == Some(s.id.as_str());
    let hovered = app.hover.as_ref() == Some(&(rt.id.clone(), Some(s.id.clone())));
    let menu_open = app.menu.as_ref() == Some(&(rt.id.clone(), Some(s.id.clone())));

    let srow = themed_row(
        if selected || hovered {
            Some(p.interactive_hover)
        } else {
            None
        },
        row![
            button(text(&s.title).size(app.fs(13)))
                .on_press(Message::SelectSession(rt.id.clone(), s.id.clone()))
                .style(theme::ghost_button(p))
                .padding([5, 10])
                .width(Length::Fill),
            slot(
                app,
                Message::MenuToggle(rt.id.clone(), Some(s.id.clone())),
                "⋯",
                menu_open.then(|| {
                    menu_items(vec![
                        ("＋ 新建会话", Message::NewSession(rt.id.clone())),
                        (
                            "删除会话",
                            Message::DeleteSession(rt.id.clone(), s.id.clone()),
                        ),
                        (
                            "归档会话",
                            Message::ArchiveSession(rt.id.clone(), s.id.clone()),
                        ),
                    ])
                }),
            ),
        ]
        .align_y(iced::alignment::Vertical::Center)
        .into(),
        Message::Hover(Some((rt.id.clone(), Some(s.id.clone())))),
        Message::Hover(None),
    );
    container(srow)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 12.0,
        })
        .into()
}

/// 行容器：hover/选中灰框 + mouse_area（enter/exit 上报 hover 行）。
fn themed_row<'a>(
    bg: Option<Color>,
    content: Element<'a, Message>,
    on_enter: Message,
    on_exit: Message,
) -> Element<'a, Message> {
    let inner = container(content)
        .width(Length::Fill)
        .padding([4, 4])
        .style(move |_| iced::widget::container::Style {
            background: bg.map(Background::Color),
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..iced::widget::container::Style::default()
        });
    mouse_area(inner).on_enter(on_enter).on_exit(on_exit).into()
}

/// ⋯/+ 槽位：常显纯文字按钮；menu 为 Some 时宿主挂覆盖式 Popover。
fn slot<'a>(
    app: &'a App,
    msg: Message,
    label: &'static str,
    menu: Option<Vec<(&'static str, Message)>>,
) -> Element<'a, Message> {
    let p = app.palette();
    let host: Element<'a, Message> = button(text(label).size(app.fs(13)).color(p.label_tertiary))
        .on_press(msg)
        .style(theme::plain_button(p))
        .padding([2, 7])
        .into();
    container(Popover::new(host, menu, p, app.fs(12)))
        .width(Length::Fixed(SLOT))
        .into()
}

/// 菜单内容（数据形式，Popover 负责悬浮定位与渲染）。
fn menu_items(items: Vec<(&'static str, Message)>) -> Vec<(&'static str, Message)> {
    items
}
