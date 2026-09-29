//! 覆盖式下拉菜单 widget（官方形态：菜单悬浮覆盖在标签上，不挤占布局）。
//!
//! iced 0.14 无内置 popover；此为实现：
//! - 菜单数据（label + Message）存于 widget（视图每帧重建，数据便宜）；
//! - 菜单按钮的 [`Tree`]（hover/press 状态）存于 widget 的 tree `state`，跨帧保留；
//! - `Widget::overlay` 现建菜单 `Element<'b>`（只引用 'static label + 克隆消息），
//!   定位在宿主右下（右对齐、向下展开），视觉上覆盖下方行。
//!
//! 主要用途：给任意宿主控件挂一个「覆盖式菜单」——宿主自己在 `menu` 为 Some 时弹出菜单层，
//! 菜单项点击直接发出调用方给的消息。
//! 为什么需要：iced 0.14 没有内置 popover，而侧边栏行尾的 ⋯ 菜单必须覆盖在列表上（不能挤占
//! 布局，否则打开菜单会推走下面的行）。实现成 `Widget` 而不是「在页面里叠一层」是因为只有
//! `overlay` 才能拿到**绝对坐标**并脱离父容器的裁剪与流式布局；三个硬教训（viewport 必须传
//! 绝对坐标、锚点 = `layout.position() + translation`、偏移 +8 避开宿主）都记录在上面，
//! 它们正是本文件存在的主要理由——换任何别的写法都会重踩。
//! 它约束：菜单数据每帧重建（便宜），但菜单的 `Tree` 存在 widget state 里跨帧保留，
//! 因此 `items` 的顺序/数量变化要经 `diff` 同步（当前只在首次打开时建树）。
//! 上接：`task::sidebar::slot`（唯一使用者，宿主 = ⋯/+ 按钮）。
//! 下接：`iced::advanced::{Widget, Overlay, Tree, layout, mouse}`、`crate::theme`。
//! 官方对应：`packages/client/ui-primitives/src/Menu.tsx` 的 `Menu`（含 `anchor`/`align`/`side`/
//! `portal`）与 `MenuSurface.tsx`；定位思路另参考 iced 官方 `iced_widget::menu`（本文件注释里
//! 所说的 "官方 menu.rs" 指 iced 的 Rust 实现，非 deepseek-harness）。

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{self, Clipboard, Shell};
use iced::widget::{button, column, container, text};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Vector};

use crate::theme;

/// 菜单条目：标签 + 触发消息（Message 需 Clone）。
///
/// 为什么需要：菜单数据要能被「每帧重建」地持有，因此标签用 `'static` 借用（文案是常量）、
/// 消息按值克隆，避免把宿主的生命周期带进 overlay 的 `'b`。
pub type MenuItem<Message> = (&'static str, Message);

/// 宿主（⋯ 按钮或空槽）+ 可选菜单数据 + 菜单样式参数。
///
/// 为什么需要：它把「被覆盖的控件」与「覆盖层内容」绑成一个 Element，页面只写一行
/// （`container(Popover::new(host, menu, p, fs))`）就能同时拿到布局占位与弹出行为。
/// `palette`/`font_size` 由构造时显式传入，是因为 overlay 里拿不到宿主的外层样式上下文。
pub struct Popover<'a, Message> {
    host: Element<'a, Message>,
    menu: Option<Vec<MenuItem<Message>>>,
    palette: theme::Palette,
    font_size: f32,
}

impl<'a, Message: Clone + 'static> Popover<'a, Message> {
    /// 宿主 + 菜单数据（None = 不弹）。
    ///
    /// 入参/出参：`host` 为被覆盖的控件、`menu` 为 Some(条目表) 时弹出（None 时行为与直接渲染
    /// 宿主完全一致）、`palette`/`font_size` 供菜单样式使用；返回 `Popover` 值
    /// （经 `From` 转成 `Element` 后才参与布局）。
    pub fn new(
        host: Element<'a, Message>,
        menu: Option<Vec<MenuItem<Message>>>,
        palette: theme::Palette,
        font_size: f32,
    ) -> Self {
        Self {
            host,
            menu,
            palette,
            font_size,
        }
    }
}

impl<'a, Message: Clone + 'static> From<Popover<'a, Message>> for Element<'a, Message> {
    /// 让 `Popover` 能直接塞进 `container(..)`/`row![..]` 等期望 `Element` 的位置。
    /// 入参/出参：消费自身，返回包好的 `Element`（内部即 `Element::new(popover)`）。
    fn from(popover: Popover<'a, Message>) -> Self {
        Element::new(popover)
    }
}

/// widget 自身的 tree state：菜单按钮树（跨帧保留 hover/press）。
///
/// 为什么需要：菜单是在 `overlay()` 里**每帧新建**的 Element，如果连它的 `Tree` 也每帧新建，
/// 菜单项就永远处于「刚初始化」状态（hover 高亮/按压反馈全丢）；所以把菜单树存在宿主的 widget
/// state 里，跨帧复用。
struct MenuState {
    menu_tree: Tree,
}

impl Default for MenuState {
    /// 初始为空树（首次 `overlay()` 时用菜单 Element 建树，见那里的说明）。
    fn default() -> Self {
        Self {
            menu_tree: Tree::empty(),
        }
    }
}

impl<Message> Widget<Message, iced::Theme, Renderer> for Popover<'_, Message>
where
    Message: Clone + 'static,
{
    /// 尺寸 = 宿主尺寸（覆盖层不参与布局，故不影响 widget 占地）。
    fn size(&self) -> Size<Length> {
        self.host.as_widget().size()
    }

    /// 布局完全交给宿主（本 widget 不改变布局，只额外提供 overlay）。
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.host
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    /// 只画宿主内容；菜单在 `overlay` 里画（保证它画在所有常规内容之上）。
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.host.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        )
    }

    /// 为每个 Popover 实例建一份 `MenuState`（菜单树跨帧保留的载体，见其文档）。
    fn state(&self) -> tree::State {
        tree::State::new(MenuState::default())
    }

    /// 子节点只有宿主一棵树（菜单树不在这里，它挂在 `MenuState` 内）。
    ///
    /// 为什么需要：`tree.children[0]` 是本 widget 唯一直接布局/绘制的子节点，索引写死依赖这个
    /// 顺序；把菜单树放进 state 而不是 children，正是为了避免它与宿主的索引混淆。
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.host.as_widget())]
    }

    /// 把宿主 Element 的变化同步进它的 Tree。
    ///
    /// 为什么需要：宿主（⋯ 按钮）每帧会被重建为新 Element，`diff` 是 iced 用来在保留 Tree
    /// （即保留 hover/press 状态）的前提下对齐结构的唯一时机；`children` 为空时补一棵空树，
    /// 是为了容忍 iced 在首次 `state()`/`diff` 调用顺序上的差异（少了它会索引越界 panic）。
    fn diff(&self, tree: &mut Tree) {
        if tree.children.is_empty() {
            tree.children.push(Tree::empty());
        }
        self.host.as_widget().diff(&mut tree.children[0]);
    }

    /// 把 operation 透传给宿主（菜单不在常规层级，故不参与）。
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.host
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    /// 事件先给宿主（⋯ 按钮的点击/按压由宿主自己处理）。
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.host.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    /// 鼠标指针形状随宿主（菜单是独立 overlay，自己算自己的）。
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.host.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    /// 弹出菜单层（`menu` 为 None 时返回 None，行为与普通容器无异）。
    ///
    /// 为什么需要：这是整个 widget 的目标——只有 overlay 能脱离父容器的布局与裁剪，拿到
    /// 宿主的**绝对坐标**并画在其下方（三个硬教训见文件头与下方注释）。
    /// 入参/出参：`layout` 为宿主布局（相对父容器）、`translation` 为累积平移量（必须叠加，
    /// 否则坐标错）；返回 `Some(MenuOverlay)` 或 None。
    /// 副作用：首次弹出时用菜单 Element 初始化 `state.menu_tree`（之后跨帧复用，保留 hover 态）。
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<advanced::overlay::Element<'b, Message, iced::Theme, Renderer>> {
        let items = self.menu.as_ref()?;
        let state = tree.state.downcast_mut::<MenuState>();
        let bounds = layout.bounds();

        // 现建菜单 Element<'b>：只引用 'static label + 克隆消息，无 'a 依赖。
        let menu = build_menu(items, self.palette, self.font_size);
        // 菜单树：首次打开初始化结构（items 同构 → 状态跨帧保留）。
        if state.menu_tree.children.is_empty() {
            state.menu_tree = Tree::new(menu.as_widget());
            menu.as_widget().diff(&mut state.menu_tree);
        }

        // 锚点 = 宿主在**视口绝对坐标**的位置：layout.bounds() 是容器内相对坐标，
        // 必须叠加 translation（iced 的 pick_list 同款：layout.position() + translation）。
        let anchor = Point::new(bounds.x, bounds.y) + translation;
        Some(advanced::overlay::Element::new(Box::new(MenuOverlay {
            menu,
            menu_tree: &mut state.menu_tree,
            host_pos: anchor,
            host_size: Size::new(bounds.width, bounds.height),
        })))
    }
}

/// 悬浮菜单本体：布局在宿主右下方（右对齐），覆盖下方行。
///
/// 为什么需要：iced 的 `overlay::Element` 需要一个 `Overlay` 实现；本结构持有菜单 Element 与
/// 它的 Tree（来自宿主 state）以及宿主的位置/尺寸，是「定位」这件事的全部输入。
struct MenuOverlay<'a, Message> {
    /// 菜单内容（在 `Popover::overlay` 里现建）。
    menu: Element<'a, Message>,
    /// 菜单的 widget 树（借自宿主 state，跨帧保留 hover/press）。
    menu_tree: &'a mut Tree,
    /// 宿主左上角在**视口绝对坐标**下的位置（已叠加 translation）。
    host_pos: Point,
    /// 宿主尺寸（决定右对齐的基准与向下展开的起点）。
    host_size: Size,
}

impl<'a, Message> advanced::Overlay<Message, iced::Theme, Renderer> for MenuOverlay<'a, Message>
where
    Message: 'static,
{
    /// 求菜单布局：右对齐到宿主右下、向下偏移 8，右缘超出视口时左移钳制。
    ///
    /// 为什么需要：这是三个硬教训里的定位规则（0 偏移会让第一项压在 ⋯ 正下方 → 点击 ⋯ 误触
    /// 第一项；不钳制则菜单在窄列里被推出视口）。
    /// 入参/出参：`bounds` 为视口尺寸；返回已 `move_to` 的布局节点。
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);
        let mut node = self
            .menu
            .as_widget_mut()
            .layout(self.menu_tree, renderer, &limits);
        let size = node.size();
        // 右对齐到宿主右下、向下展开（官方 menu.rs：position + target_height）。
        // 偏移 +8 避开 ⋯ 底部；右缘超出视口时左移收进视口。
        let mut x = self.host_pos.x + self.host_size.width - size.width;
        if x + size.width > bounds.width {
            x = (bounds.width - size.width - 4.0).max(4.0);
        }
        let y = self.host_pos.y + self.host_size.height + 8.0;
        node.move_to_mut(Point::new(x, y));
        node
    }

    /// 画菜单内容；viewport 必须传菜单自身的绝对矩形（见下方教训注释）。
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        // viewport 必须是**绝对坐标的菜单矩形**（官方 menu.rs 传 &bounds），
        // 否则内容被渲染器按 (0,0) 视口裁剪掉 → 菜单不可见。
        let viewport = layout.bounds();
        self.menu.as_widget().draw(
            self.menu_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &viewport,
        );
    }

    /// 菜单内的事件处理（同样以菜单自身为 viewport——与 `draw` 保持一致，否则点击命中区
    /// 与可见区会错位）。
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let viewport = layout.bounds();
        self.menu.as_widget_mut().update(
            self.menu_tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &viewport,
        );
    }

    /// 菜单项的鼠标指针形状（让按钮显示手型/默认指针）。
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        self.menu
            .as_widget()
            .mouse_interaction(self.menu_tree, layout, cursor, &viewport, renderer)
    }
}

/// 由菜单数据构建菜单 Element（边框圆角卡片 + 幽灵项）。
/// 实验：实心背景（bg_layer3），定位可见性 vs 样式问题。
///
/// 为什么需要：菜单是在 overlay 里现建的，没有现成 Element 可复用；条目用幽灵按钮
/// （hover 才有填充）以匹配官方菜单的观感，外表包一层 layer3 底 + 圆角卡片使菜单与下方内容
/// 分离（否则菜单看起来像列表的一部分）。
/// 入参/出参：`items` 为条目（label + 消息）、`p` 调色板、`fs` 字号；返回菜单 `Element`。
fn build_menu<'a, Message: Clone + 'a>(
    items: &[MenuItem<Message>],
    p: theme::Palette,
    fs: f32,
) -> Element<'a, Message> {
    let col = items
        .iter()
        .fold(column![].spacing(1), |col, (label, msg)| {
            col.push(
                button(text(*label).size(fs))
                    .on_press(msg.clone())
                    .style(theme::ghost_button(p))
                    .padding([5, 12])
                    .width(Length::Shrink),
            )
        });
    container(col)
        .padding([4, 8])
        .style(theme::surface(p, p.bg_layer3, 8.0))
        .into()
}
