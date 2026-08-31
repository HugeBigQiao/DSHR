//! dshr UI：入口（iced 应用接线）。
//!
//! 布局：左侧全局导航 + 按页内容（任务 / 监控 / 配置）。
//! 事件接入：AppState 后台线程 + Subscription 事件流直收（零延迟，不再 100ms 轮询）。
//! 工作区锁死：Start 时 cwd 定死，runtime 标题下展示路径（只读）。
//!
//! 分层（每页一个文件：状态 + 处理 + 渲染）：
//! - [`app`]：根组件（App 状态机 + 根 view 分发 + 公共渲染工具）
//! - [`task`]：任务页（runtime 树 + 聊天；收后台事件）
//! - [`monitor`]：监控页（数据看板，M3 占位）
//! - [`setting`]：配置页（三配置文件编辑 + 外观）
//! - [`message`]：Message 枚举

mod app;
mod message;
mod monitor;
mod nav;
mod setting;
mod task;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .window_size(iced::Size::new(1200.0, 760.0))
        .title("dshr")
        .subscription(App::subscription)
        // 主题跟随配置（配置页「外观」可即时切换）。
        .theme(|app: &App| app.theme.clone())
        .run()
}
