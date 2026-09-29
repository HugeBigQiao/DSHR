//! dshr UI：入口（iced 应用接线）。
//!
//! 布局（对标官方基础版 + 旧 DESIGN 三页设计）：
//! - 顶部菜单栏：任务 / 文件 / 监控 / 配置（nav.rs）
//! - 任务页内部三区：左侧边栏（会话列表）/ 中间对话（消息流 + composer）/ 右侧预留（未来 turn rail）
//!
//! 数据接入（分层见 dshr/DESIGN.md §3）：bridge 总线（bridge.rs
//! 类型/订阅）→ dshr_state::engine（核心数据处理）→ dshr_state::raw（与 SDK 沟通：
//! Fake/Real 判定 + 装配 + 事件循环 + WireLog）→ Snapshot 事件 → app.rs 刷 model
//! 视图模型。UI 只搬运命令/事件，不直接 import SDK/协议类型。
//!
//! 主要用途：把 `App` 装进 iced 运行时——声明窗口设置（尺寸/位置/无边框）、内置中文字体、
//! 主题与订阅回调，然后 `run()`。除此之外不做任何业务决策。
//! 为什么需要：DPI 自洽修复（`make_consistent`）必须发生在**创建任何窗口之前**，而窗口
//! 创建由 iced 的 builder 触发；所以「先修 DPI、再按桌面收敛尺寸/位置、最后建窗口」这段
//! 顺序只在这一个文件里成立，不能下移到 `app.rs`（那里已经太晚）。
//! 上接：进程入口（本文件的 `main`，无上层模块）；被 iced 运行时回调
//! `App::new/update/view/subscription`（app.rs）。
//! 下接：`app::App`（根状态机）、`dpi`（DPI 修复 + 尺寸/位置收敛）、`assets/fonts/NotoSansSC.ttf`。
//! 官方对应：`packages/client/web/src/mount.ts` 的 `mountClient(ctx, container)`
//! 与 `packages/client/web/src/boot.ts`（官方把根组件挂到容器；dshr 由 iced 直接接管窗口）。

mod app;
mod bridge;
mod files;
mod model;
mod monitor;
mod nav;
mod setting;
mod statusbar;
mod task;
mod theme;
mod widgets;

#[cfg(windows)]
mod dpi;
#[cfg(not(windows))]
mod dpi {
    //! 非 Windows：无 DPI 自洽问题（X11/Wayland 由 winit 正常处理）。
    //!
    //! 主要用途/为什么需要：给 `main()` 提供与 Windows 版同名的公开函数（`display_scale` /
    //! `make_consistent` / `clamp_to_desktop` / `centered_position`），让入口代码不必写平台分支。
    //! 上接：`main()`（crate 根）。下接：`iced::window::Position`（仅 `centered_position` 用到）。
    //! 官方对应：无（官方为 Web 客户端，无窗口 DPI 面）。

    /// 非 Windows 的缩放因子恒为 winit 报告值，这里返回 1.0（恒等）。
    ///
    /// 为什么需要：与 Windows 版同名，供入口判断用；本平台无需据此改任何行为。
    /// 返回：`1.0`。
    pub fn display_scale() -> f32 {
        1.0
    }

    /// 无需修正。
    ///
    /// 为什么需要：与 Windows 版同名，让 `main()` 不必写平台分支。
    /// 返回：`false`（表示"未做修正"，与 Windows 版在无需修正时的返回值一致）。
    pub fn make_consistent() -> bool {
        false
    }

    /// 原样返回。
    ///
    /// 为什么需要：与 Windows 版同名——那里会按真实桌面尺寸收缩窗口。
    /// 入参：目标逻辑尺寸。返回：入参原值（winit 在本平台会自行约束到可见区域）。
    pub fn clamp_to_desktop(width: f32, height: f32) -> (f32, f32) {
        (width, height)
    }

    /// 非 Windows 下不自行定位：交给窗口管理器。
    ///
    /// 为什么需要：`main()` 在所有平台都调用本函数，所以必须存在（否则非 Windows 目标编译失败）。
    /// 为什么这里不做真正的居中：读显示器尺寸需要 `EventLoop`（winit 0.30 的 `primary_monitor`）
    /// 或 iced 的 `window::monitor_size`，而后者是异步 `Task`、前者要再建一个事件循环——
    /// iced 的 `run()` 自己会建事件循环，多建一个有平台风险。**故本平台返回 `(0.0, 0.0)`**，
    /// 让 WM 决定位置（X11/Wayland 的常规行为）。
    /// 入参：窗口逻辑尺寸（此处未使用）。返回：`(0.0, 0.0)`。
    pub fn centered_position(_width: f32, _height: f32) -> (f32, f32) {
        (0.0, 0.0)
    }
}

use app::App;

/// 进程入口：先做 DPI 自洽与窗口几何收敛，再交给 iced 建窗口。
///
/// 为什么需要：iced builder 一旦 `run()` 就会创建窗口，之后无法再调
/// `SetProcessDpiAwarenessContext`（每进程只生效一次），所以修复必须在这里、在 `run()` 之前。
/// 入参/出参：无入参；返回 iced 事件循环的结束结果（`iced::Result`，窗口关闭即 Ok）。
/// 主要功能：`dpi::make_consistent()` → `clamp_to_desktop(1200x760)` → `centered_position`
/// → 装配 `iced::application(App::new, App::update, App::view)`（窗口/字体/主题/订阅）→ `run()`。
fn main() -> iced::Result {
    // 必须在创建任何窗口之前调用：DPI 不一致的机器上，winit 会按 1:1 建窗口却按缩放
    // 因子渲染，导致布局画布比可见区域大一倍（UI 右/下半被切掉、顶栏窗口按钮在屏幕外）。
    // 详见 dpi.rs 的实测记录。
    dpi::make_consistent();

    // 目标逻辑尺寸；再按当前桌面收敛，避免在小桌面/高缩放下窗口超出屏幕。
    let (width, height) = dpi::clamp_to_desktop(1200.0, 760.0);
    let (x, y) = dpi::centered_position(width, height);

    iced::application(App::new, App::update, App::view)
        .title("dshr")
        // 中文渲染：内置 Noto Sans SC（OFL，见 assets/fonts/OFL.txt）并设为全局默认字体。
        // Linux 等无系统中文字体的环境必需；字体为可变字重，默认 Regular。
        .default_font(iced::font::Font::with_name("Noto Sans SC"))
        .font(include_bytes!("../assets/fonts/NotoSansSC.ttf").as_slice())
        // Zed 式无边框窗口：顶栏（nav.rs）兼作窗框，空白处可拖动，右侧为窗口控制。
        //
        // 注意：尺寸必须写在 Settings 里——iced 的 `.window_size()` 会被后续的
        // `.window(..)` 整体覆盖（`Settings::default().size` 是 1024x768），
        // 之前 1200x760 一直没生效。
        .window(iced::window::Settings {
            size: iced::Size::new(width, height),
            position: iced::window::Position::Specific(iced::Point::new(x, y)),
            decorations: false,
            ..iced::window::Settings::default()
        })
        .subscription(App::subscription)
        .theme(|app: &App| app.theme())
        .run()
}
