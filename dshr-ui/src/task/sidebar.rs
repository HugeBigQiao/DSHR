//! 左侧边栏：runtime 树 + 会话缩进层级。
//!
//! s3 收敛（DESIGN.md §3）：单 runtime（= 当前 dsh/fake 进程）+ 单当前会话；
//! 原多 runtime 假树逻辑删除，多 runtime/多会话管理留 s4。行内交互保留 Zed 风格：
//! - 行尾 ⋯ / runtime 行 +：**常显**（纯文字，和背景同色）；
//! - 点击 ⋯ → 覆盖式菜单（Popover 悬浮在标签上，右对齐向下展开，官方形态）；
//! - hover 或选中时整行显示灰色框；当前选中会话常显灰框；
//! - 会话行带状态点：running 高亮、idle 正常灰、stopped/failed 错误红；
//! - 顶部「＋ 新建 runtime」= 启动 runtime（Fake/Real 判定在 dshr_state::raw）。
//!
//! 主要用途：画 runtime 树（runtime 行 + 缩进会话行）、行尾常显的 ⋯/+ 槽位、以及挂在槽位上的
//! 覆盖式菜单；行 hover/选中背景与状态点也在这里。
//! 为什么需要：它是本项目唯一使用自研 `Popover` 的地方，且「行 hover 状态放在 App、
//! 菜单开关放在 App」这套跨行交互需要一处集中协调（hover 进入/离开 → `Message::Hover`，
//! 点 ⋯ → `Message::MenuToggle`）；会话行与 runtime 行的层级/缩进规则也只在树里成立。
//! 它约束：所有行内操作必须经 `task::Message` 回到 App，本文件不改任何状态；菜单项文案属于产品文案。
//! 上接：`task::view`（左列）。
//! 下接：`app::App`（`data.runtimes`/`hover`/`menu`/`data.chat.status`）、`model::{RuntimeView,
//! SessionView}`、`widgets::popover::Popover`、`theme`。
//! 官方对应：`packages/client/ui-sidebar/src/client/SidebarRoot.tsx` 的 `SidebarRoot`
//! （工作区/会话浏览区 + 行内操作）；"⋯ 菜单"对应 `packages/client/ui-primitives/src/Menu.tsx`
//! 的 `Menu`（含 `MenuItemButton`）。
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text};
use iced::{Background, Border, Color, Element, Length};

use crate::app::App;
use crate::model::RuntimeView;
use crate::task::Message;
use crate::theme;
use crate::widgets::popover::Popover;

/// ⋯/+ 按钮位宽度（对齐占位）。
///
/// 为什么需要：槽位常显但只有图标宽度，固定宽度才能让「名称左对齐 + 操作右对齐」在 hover
/// 显示/隐藏 ⋯ 时不左右抖动。
const SLOT: f32 = 30.0;

/// 渲染 runtime 树。
///
/// 为什么需要：树的空/非空两态都必须在同一列里给出（未启动时给操作指引，启动后给树），
/// 且「新建 runtime」按钮固定在最上方不随树滚动。
/// 入参/出参：`app` 提供 `data.runtimes` 等；返回左列 `Element<Message>`。
pub fn view<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    // 强制居中：按钮内容 = 左右对称 Space。
    let new_rt = button(row![
        Space::new().width(Length::Fill),
        text("＋ 新建 runtime").size(app.fs(13)),
        Space::new().width(Length::Fill),
    ])
    .on_press(Message::NewRuntime)
    .style(theme::primary_button(p))
    .padding([8, 12])
    .width(Length::Fill);
    let mut tree = column![].spacing(4);
    if app.data.runtimes.is_empty() {
        // 未启动提示（runtime 启动后由 Started 事件插入单槽）。
        tree = tree.push(
            container(
                text("未启动。\nFake：无需配置，随 prompt 回显测试事件；\n真实 dsh：需 workspace 根 config.json 配 api-key。")
                    .size(app.fs(11))
                    .color(p.label_caption),
            )
            .padding([6, 4]),
        );
    } else {
        tree = app
            .data
            .runtimes
            .iter()
            .fold(tree, |col, rt| col.push(runtime_block(app, rt)));
    }
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
///
/// 为什么需要：runtime 行同时承载三件事（展开/收起、⋯ 菜单、hover 背景），且菜单是「覆盖式」——
/// 打开时挂在 ⋯ 槽位上而不是插进列表，所以这一层必须知道 `hover`/`menu` 是否指向本行。
/// 入参/出参：`app` 提供交互态、`rt` 为该 runtime 的视图模型；返回该块（行 + 会话列）。
/// 主要功能：按 `rt.expanded` 决定是否渲染会话列；行背景与菜单开合都由 `app` 的全局态决定。
fn runtime_block<'a>(app: &'a App, rt: &'a RuntimeView) -> Element<'a, Message> {
    let p = app.palette();
    let hovered = app.hover.as_ref() == Some(&(rt.id.clone(), None));
    let menu_open = app.menu.as_ref() == Some(&(rt.id.clone(), None));

    let rt_row = themed_row(
        if hovered {
            Some(p.interactive_hover)
        } else {
            None
        },
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
///
/// 为什么需要：会话行的选中态（`selected_session`）与 hover 态共用一种背景，且状态点要显示
/// **当前会话**的生命周期（s3 单会话：所有行都用 `data.chat.status`）；把这些判定集中在此
/// 才能让「选中/hover/状态点/菜单」四件事用同一份比较。
/// 入参/出参：`rt` 提供选中项与 id、`s` 为该会话行；返回会话行（左侧缩进 12px）。
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
            // 状态点：会话行反映当前会话生命周期（running 高亮 / idle 灰 / 异常红）。
            text("●")
                .size(app.fs(8))
                .color(p.status_color(app.data.chat.status)),
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
///
/// 为什么需要：iced 里按钮只报点击，行级 hover 必须用 `mouse_area` 的 enter/exit 事件；
/// 把「背景 + 圆角 + hover 上报」合成一层，runtime 行与会话行才不会各写一份（也避免
/// 在子控件上加 hover 判定导致鼠标移到 ⋯ 上就丢高亮）。
/// 入参/出参：`bg` 为 Some 时的行背景（None = 无背景）、`content` 为行内容、
/// `on_enter`/`on_exit` 为进入/离开该行要发的消息；返回可交互行 `Element<Message>`。
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
///
/// 为什么需要：⋯ 与 + 是两个语义不同的按钮，但视觉上是同一个固定宽槽位；把「按钮 + 可选菜单」
/// 合成一个函数，是因为菜单必须**挂在宿主自己身上**（`Popover` 以宿主位置为锚点），
/// 若拆开就容易把菜单挂到别的行上。
/// 入参/出参：`msg` 为点击消息、`label` 为字形（"⋯" / "+" / "−"）、`menu` 为 Some(条目表) 时
/// 弹出菜单；返回固定宽 `SLOT` 的槽位 Element。
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
///
/// 为什么需要：把调用点的 `vec![...]` 与 Popover 的入参形状解耦——将来要加图标/禁用态时
/// 只改本函数与 Popover，不必动每个调用点。
/// 入参/出参：`items` 为 `(标签, 消息)` 列表；原样返回（当前是纯透传，无副作用）。
fn menu_items(items: Vec<(&'static str, Message)>) -> Vec<(&'static str, Message)> {
    items
}
