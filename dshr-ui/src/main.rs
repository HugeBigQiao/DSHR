//! dshr UI：简单版（全流程跑通，DESIGN §9.5 / §15-8）。
//!
//! 布局：左 runtime/会话树 + 右聊天区（消息流 + 输入框）。
//! 事件接入：AppState 后台线程 + 100ms 轮询 tick + try_recv（流式渲染 v2 前够用）。
//! 工作区锁死：Start 时 cwd 定死，runtime 标题下展示路径（只读）。
//!
//! 分层（与 state 同风格）：
//! - [`app`]：App 状态机（update/apply_event/订阅）
//! - [`view`]：渲染层（view/sidebar/chat_area）
//! - [`message`]：Message 枚举
//! - [`model`]：视图模型（RtView/SessionView/MsgView）

mod app;
mod message;
mod model;
mod view;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .window_size(iced::Size::new(1100.0, 720.0))
        .title("dshr")
        .subscription(App::subscription)
        .run()
}
