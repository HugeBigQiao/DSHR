//! 顶栏（Zed 风格）：左侧页面标签（任务/监控/配置），右侧窗口控制（— □ ✕）。
//! 布局对齐官方设计系统：layer1 底 + 标签 active 填充 + 幽灵窗口按钮。
//! 窗口图标用 canvas 自绘：unicode 字符（— □ ✕）在不同字体 fallback 下
//! 大小不一（尤其 U+25A1 在 Segoe UI 渲染偏小），自绘保证三个图标视觉统一。
//! 热区按桌面惯例（~44×30，KDE/Windows 同档），图标统一 16×16。
use iced::mouse;
use iced::widget::canvas::{Canvas, Frame, Geometry, Path, Program, Stroke};
use iced::widget::{Space, button, container, row, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::app::{App, Message, Page, WindowCmd};
use crate::theme;

/// 窗口图标种类（canvas 程序按此绘制线条/方框/叉）。
#[derive(Debug, Clone, Copy)]
enum WinIconKind {
    Minimize,
    Maximize,
    Close,
}

/// 图标程序：kind + 颜色（视图每帧重建，颜色来自当前 palette）。
#[derive(Debug, Clone, Copy)]
struct WinIcon {
    kind: WinIconKind,
    color: Color,
}

impl<Message> Program<Message> for WinIcon {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let stroke = Stroke::default().with_width(1.0).with_color(self.color);
        match self.kind {
            WinIconKind::Minimize => {
                frame.stroke(
                    &Path::line(Point::new(w * 0.15, h * 0.5), Point::new(w * 0.85, h * 0.5)),
                    stroke,
                );
            }
            WinIconKind::Maximize => {
                frame.stroke(
                    &Path::rectangle(
                        Point::new(w * 0.18, h * 0.24),
                        Size::new(w * 0.64, h * 0.52),
                    ),
                    stroke,
                );
            }
            WinIconKind::Close => {
                frame.stroke(
                    &Path::line(
                        Point::new(w * 0.24, h * 0.24),
                        Point::new(w * 0.76, h * 0.76),
                    ),
                    stroke,
                );
                frame.stroke(
                    &Path::line(
                        Point::new(w * 0.76, h * 0.24),
                        Point::new(w * 0.24, h * 0.76),
                    ),
                    stroke,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// 渲染顶栏。
pub fn nav<'a>(app: &'a App) -> Element<'a, Message> {
    let p = app.palette();
    let tab = |label: &'static str, target: Page| {
        button(text(label).size(app.fs(13)))
            .on_press(Message::Nav(target))
            .style(theme::nav_button(p, app.page == target))
            .padding([6, 14])
    };
    // 窗口控制按钮：热区 22×16、图标 9×9（当前档的 2/3；线宽同步减到 1.0）。
    let win = |kind: WinIconKind, cmd: WindowCmd| {
        button(
            Canvas::new(WinIcon {
                kind,
                color: p.label_secondary,
            })
            .width(Length::Fixed(9.0))
            .height(Length::Fixed(9.0)),
        )
        .on_press(Message::Window(cmd))
        .style(theme::ghost_button(p))
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(16.0))
        .padding([0, 0])
    };
    container(row![
        tab("任务", Page::Task),
        tab("监控", Page::Monitor),
        tab("配置", Page::Setting),
        // 空白区 = 无边框窗口拖动区（Zed 式：顶栏空白处按住拖动）。
        button(Space::new().width(Length::Fill))
            .on_press(Message::Window(WindowCmd::Drag))
            .style(theme::ghost_button(p)),
        win(WinIconKind::Minimize, WindowCmd::Minimize),
        win(WinIconKind::Maximize, WindowCmd::Maximize),
        win(WinIconKind::Close, WindowCmd::Close),
    ])
    .padding([6, 8])
    .width(Length::Fill)
    .style(theme::surface(p, p.bg_layer1, 0.0))
    .into()
}
