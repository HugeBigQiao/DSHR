//! 覆盖式下拉菜单 widget（官方形态：菜单悬浮覆盖在标签上，不挤占布局）。
//!
//! iced 0.14 无内置 popover；此为实现：
//! - 菜单数据（label + Message）存于 widget（视图每帧重建，数据便宜）；
//! - 菜单按钮的 [`Tree`]（hover/press 状态）存于 widget 的 tree `state`，跨帧保留；
//! - `Widget::overlay` 现建菜单 `Element<'b>`（只引用 'static label + 克隆消息），
//!   定位在宿主右下（右对齐、向下展开），视觉上覆盖下方行。

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{self, Clipboard, Shell};
use iced::widget::{button, column, container, text};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Vector};

use crate::theme;

/// 菜单条目：标签 + 触发消息（Message 需 Clone）。
pub type MenuItem<Message> = (&'static str, Message);

/// 宿主（⋯ 按钮或空槽）+ 可选菜单数据 + 菜单样式参数。
pub struct Popover<'a, Message> {
    host: Element<'a, Message>,
    menu: Option<Vec<MenuItem<Message>>>,
    palette: theme::Palette,
    font_size: f32,
}

impl<'a, Message: Clone + 'static> Popover<'a, Message> {
    /// 宿主 + 菜单数据（None = 不弹）。
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
    fn from(popover: Popover<'a, Message>) -> Self {
        Element::new(popover)
    }
}

/// widget 自身的 tree state：菜单按钮树（跨帧保留 hover/press）。
struct MenuState {
    menu_tree: Tree,
}

impl Default for MenuState {
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
    fn size(&self) -> Size<Length> {
        self.host.as_widget().size()
    }

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
        self.host
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport)
    }

    fn state(&self) -> tree::State {
        tree::State::new(MenuState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.host.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        if tree.children.is_empty() {
            tree.children.push(Tree::empty());
        }
        self.host.as_widget().diff(&mut tree.children[0]);
    }

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
struct MenuOverlay<'a, Message> {
    menu: Element<'a, Message>,
    menu_tree: &'a mut Tree,
    host_pos: Point,
    host_size: Size,
}

impl<'a, Message> advanced::Overlay<Message, iced::Theme, Renderer> for MenuOverlay<'a, Message>
where
    Message: 'static,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);
        let mut node = self.menu.as_widget_mut().layout(self.menu_tree, renderer, &limits);
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

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let viewport = layout.bounds();
        self.menu.as_widget().mouse_interaction(
            self.menu_tree,
            layout,
            cursor,
            &viewport,
            renderer,
        )
    }
}

/// 由菜单数据构建菜单 Element（边框圆角卡片 + 幽灵项）。
/// 实验：实心背景（bg_layer3），定位可见性 vs 样式问题。
fn build_menu<'a, Message: Clone + 'a>(
    items: &[MenuItem<Message>],
    p: theme::Palette,
    fs: f32,
) -> Element<'a, Message> {
    let col = items.iter().fold(column![].spacing(1), |col, (label, msg)| {
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
