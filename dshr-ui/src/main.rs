//! dshr UI：入口（iced 应用接线）。
//!
//! 布局（对标官方基础版 + 旧 DESIGN 三页设计）：
//! - 顶部菜单栏：任务 / 监控 / 配置（nav.rs）
//! - 任务页内部三区：左侧边栏（会话列表）/ 中间对话（消息流 + composer）/ 右侧预留（未来 turn rail）
//!
//! 数据接入：state 冻结期间走 `bridge` 的 PlaceholderBridge（演示数据）；
//! 接 dshr-state 后换成真实桥（UI 只消费 `model` 的视图模型，不直接碰 SDK 类型）。

mod app;
mod bridge;
mod model;
mod monitor;
mod nav;
mod setting;
mod statusbar;
mod task;
mod theme;
mod widgets;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .window_size(iced::Size::new(1200.0, 760.0))
        .title("dshr")
        // Zed 式无边框窗口：顶栏（nav.rs）兼作窗框，空白处可拖动，右侧为窗口控制。
        .window(iced::window::Settings {
            decorations: false,
            ..iced::window::Settings::default()
        })
        .subscription(App::subscription)
        .theme(|app: &App| app.theme())
        .run()
}
