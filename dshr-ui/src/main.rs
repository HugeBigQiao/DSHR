//! dshr UI：入口（iced 应用接线）。
//!
//! 布局（对标官方基础版 + 旧 DESIGN 三页设计）：
//! - 顶部菜单栏：任务 / 监控 / 配置（nav.rs）
//! - 任务页内部三区：左侧边栏（会话列表）/ 中间对话（消息流 + composer）/ 右侧预留（未来 turn rail）
//!
//! 数据接入（s3，DESIGN §11.4）：bridge 总线（bridge.rs 类型/订阅 → worker.rs 进程驱动
//! → real.rs Fake/Real 判定 + Folder 折叠）→ Snapshot 事件 → app.rs 刷 model 视图模型。
//! UI 不直接碰 SDK/协议类型；PlaceholderBridge 假数据已随 s3 删除。

mod app;
mod bridge;
mod model;
mod monitor;
mod nav;
mod real;
mod setting;
mod statusbar;
mod task;
mod theme;
mod widgets;
mod worker;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .window_size(iced::Size::new(1200.0, 760.0))
        .title("dshr")
        // 中文渲染：内置 Noto Sans SC（OFL，见 assets/fonts/OFL.txt）并设为全局默认字体。
        // Linux 等无系统中文字体的环境必需；字体为可变字重，默认 Regular。
        .default_font(iced::font::Font::with_name("Noto Sans SC"))
        .font(include_bytes!("../assets/fonts/NotoSansSC.ttf").as_slice())
        // Zed 式无边框窗口：顶栏（nav.rs）兼作窗框，空白处可拖动，右侧为窗口控制。
        .window(iced::window::Settings {
            decorations: false,
            ..iced::window::Settings::default()
        })
        .subscription(App::subscription)
        .theme(|app: &App| app.theme())
        .run()
}
