//! Windows DPI 自洽修复。
//!
//! ## 问题（2026-09-17 实测）
//!
//! 在「系统 DPI ≠ 显示器 DPI」的机器上（本例：`GetDpiForSystem() = 96`，显示器 192 DPI /
//! 200% 缩放，通常是登入后显示器 DPI 变过），winit 会出现自相矛盾的状态：
//!
//! - 创建窗口时按 **1:1** 换算 → 请求 1000x700 得到 1000x700 **物理**像素；
//! - 之后向 iced 报告缩放因子 **2.0** → iced 按 2 倍渲染。
//!
//! iced 的布局画布 = 物理尺寸 / `program.scale_factor`，渲染缩放 = winit 缩放 ×
//! `program.scale_factor`，因此**可见逻辑区域恒为画布的 1/缩放因子**。实测（100x100 红块探针）：
//!
//! | program.scale_factor | 窗口物理 | 渲染缩放 | 结果 |
//! |---|---|---|---|
//! | 1.0（默认） | 1000x700 | 2 | 只有左上 1/4 可见，窗口按钮跑到屏幕外 |
//! | 0.5 | 500x350 | 1 | 仍是画布的左上 1/2 |
//!
//! 推导：可见 = 物理/(Z·s)，画布 = 物理/s，故 画布/可见 ≡ Z —— **Z > 1 时任何 s 都无法自洽**。
//! 这就是「整个 UI 右半边和下半边被切掉、顶栏窗口按钮完全看不见」的根因。
//!
//! ## 修法
//!
//! 在创建任何窗口之前把进程设为 **DPI-unaware**：Windows 随后统一报告 96 DPI，
//! winit 缩放 = 1，画布 = 物理 = 可见，全部自洽（代价：200% 屏上由系统做位图放大，
//! 文字不如原生缩放锐利）。
//!
//! 仅在检测到「显示器被缩放」时才这么做：用 `EnumDisplaySettingsW` 取真实分辨率，
//! 与 unaware 进程看到的虚拟桌面比较；比值为 1（100% 缩放）时保持 iced 默认行为。
//! 环境变量 `DSHR_DPI=aware|unaware|auto` 可强制覆盖（默认 auto）。
//!
//! 主要用途：给 `main()` 提供「进程 DPI 感知修正 + 初始窗口几何」三件事——
//! `make_consistent()`（是否已设为 unaware）、`clamp_to_desktop()`、`centered_position()`。
//! 为什么需要：这是对 winit 自相矛盾行为的唯一可行绕法，且**必须**在创建窗口前调用
//! （`SetProcessDpiAwarenessContext` 每进程只成功一次）。放在独立文件是因为它整块是
//! 平台/FFI 细节（裸 `user32`/`gdi32` 声明 + `DEVMODEW` 布局），与 UI 布局代码无任何交集；
//! 非 Windows 由 `main.rs` 里的恒等占位模块顶替。它约束：`make_consistent` 必须先于任何
//! 窗口创建，且调用点只有一处。
//! 上接：`main()`（crate 根，`use` 前先调用）。
//! 下接：Win32 `SetProcessDpiAwarenessContext` / `GetSystemMetrics` / `EnumDisplaySettingsW`。
//! 官方对应：无——官方是浏览器内 Web 客户端，DPI 由浏览器/系统处理。
#![cfg(windows)]

use std::sync::OnceLock;

/// 真实分辨率 / 虚拟桌面宽度的比值（1.0 = 未缩放）。
///
/// 为什么需要：`display_scale()` 要被「是否接管」判定与诊断共用，而探测要调 Win32；
/// 进程 DPI 感知在生命周期内不会变，所以算一次就够（`OnceLock` 保证一致且无锁开销）。
static DISPLAY_SCALE: OnceLock<f32> = OnceLock::new();

#[link(name = "user32")]
unsafe extern "system" {
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn EnumDisplaySettingsW(device_name: *const u16, mode_num: u32, dev_mode: *mut DevModeW)
    -> i32;
}

/// `DEVMODEW` 的最小子集：只需要分辨率字段，但要保留完整布局以免越界写。
///
/// 为什么需要：不引 `windows`/`winapi` crate（只为两个调用不值得多一棵依赖树），
/// 因此必须手工声明与 Win32 一致的结构体布局；字段名与官方头文件同名（含本模块不读的），
/// 且顺序/宽度一字不能错——`EnumDisplaySettingsW` 会按 `dm_size` 往里写。
#[repr(C)]
#[derive(Clone, Copy)]
struct DevModeW {
    dm_device_name: [u16; 32],
    dm_spec_version: u16,
    dm_driver_version: u16,
    dm_size: u16,
    dm_driver_extra: u16,
    dm_fields: u32,
    dm_position_x: i32,
    dm_position_y: i32,
    dm_display_orientation: u32,
    dm_display_fixed_output: u32,
    dm_color: i16,
    dm_duplex: i16,
    dm_y_resolution: i16,
    dm_tt_option: i16,
    dm_collate: i16,
    dm_form_name: [u16; 32],
    dm_log_pixels: u16,
    dm_bits_per_pel: u32,
    dm_pels_width: u32,
    dm_pels_height: u32,
    dm_display_flags: u32,
    dm_display_frequency: u32,
    dm_icm_method: u32,
    dm_icm_intent: u32,
    dm_media_type: u32,
    dm_dither_type: u32,
    dm_reserved1: u32,
    dm_reserved2: u32,
    dm_panning_width: u32,
    dm_panning_height: u32,
}

impl Default for DevModeW {
    /// 全零初值（`dm_size` 由调用方在探测前填入）。
    fn default() -> Self {
        // SAFETY: 全零是 DEVMODEW 的合法初值（dm_size 由调用前设置）。
        unsafe { std::mem::zeroed() }
    }
}

/// 主显示器真实分辨率（物理像素，不受进程 DPI 感知影响）。
///
/// 为什么需要：`GetSystemMetrics(SM_CXSCREEN)` 在 DPI-unaware 进程里返回的是被系统折算过的
/// 虚拟桌面宽度，只有驱动层枚举模式才能拿到真实像素——两者相除就是缩放比。
/// 入参/出参：无；返回主显示器宽度（物理像素）；枚举失败或宽度非正时返回 `None`
/// （调用方退化为「与虚拟桌面相同」= 视为未缩放）。
fn real_display_width() -> Option<f32> {
    let mut mode = DevModeW {
        dm_size: std::mem::size_of::<DevModeW>() as u16,
        ..DevModeW::default()
    };
    // ENUM_CURRENT_SETTINGS = -1（以 u32 传入）
    let ok = unsafe { EnumDisplaySettingsW(std::ptr::null(), u32::MAX, &mut mode) };
    (ok != 0 && mode.dm_pels_width > 0).then_some(mode.dm_pels_width as f32)
}

/// 本进程当前看到的桌面宽度（DPI-unaware 进程会看到被缩放后的虚拟桌面）。
///
/// 为什么需要：它是「真实 / 虚拟」比值的分母，也是窗口几何收敛时唯一可用的桌面尺寸
/// （unaware 之后真实像素已无意义——布局用的就是这套虚拟坐标）。
/// 入参/出参：无；返回虚拟桌面宽度（像素，f32 以配合 iced 的逻辑单位运算）。
fn virtual_desktop_width() -> f32 {
    const SM_CXSCREEN: i32 = 0;
    unsafe { GetSystemMetrics(SM_CXSCREEN) as f32 }
}

/// 显示器缩放比（真实宽度 ÷ 虚拟桌面宽度）。1.0 表示没有缩放。
///
/// 为什么需要：把「这台机器是否需要接管」变成一个可缓存、可诊断的数字（也便于打印/断言）。
/// 入参/出参：无；返回比值，首次调用探测并缓存；虚拟桌面宽度非正（探测异常）时返回 1.0。
pub fn display_scale() -> f32 {
    *DISPLAY_SCALE.get_or_init(|| {
        let virtual_width = virtual_desktop_width();
        let real_width = real_display_width().unwrap_or(virtual_width);
        if virtual_width > 0.0 {
            real_width / virtual_width
        } else {
            1.0
        }
    })
}

/// 按需把进程设为 DPI-unaware，返回是否执行了该操作。
///
/// **必须在创建任何窗口之前调用**：`SetProcessDpiAwarenessContext` 每个进程只能成功一次。
///
/// 为什么需要：这是「糊 / 切」二选一里的兜底选择——宁可让系统做位图拉伸（略糊），
/// 也不能让可见区域只剩画布的 1/缩放因子（UI 被切掉）。
/// 入参/出参：无入参（读环境变量 `DSHR_DPI`：`aware` 强制不接管、`unaware` 强制接管、
/// 其它/缺省 = auto 仅在 `display_scale() > 1.05` 时接管）；返回 `true` 表示本次真的改成了
/// unaware，`false` 表示保持 iced 默认（含强制 aware 与 Win32 调用失败）。
pub fn make_consistent() -> bool {
    let forced = std::env::var("DSHR_DPI").unwrap_or_else(|_| "auto".into());
    let unaware = match forced.as_str() {
        "aware" => false,
        "unaware" => true,
        // auto：显示器被缩放时才接管（见模块注释）
        _ => display_scale() > 1.05,
    };

    if !unaware {
        return false;
    }

    // DPI_AWARENESS_CONTEXT_UNAWARE = -1
    let ok = unsafe { SetProcessDpiAwarenessContext(-1) };
    ok != 0
}

/// 初始窗口尺寸（逻辑单位）：贴合当前桌面，避免窗口跑到屏幕外。
///
/// 在 200% 缩放的 1280x800 显示器上，unaware 进程看到的桌面可能只有 640x400 ——
/// 直接请求 1200x760 会得到比桌面还大的窗口。
/// 入参/出参：`width`/`height` 为期望尺寸；返回收敛后的 `(宽, 高)`（留 24px / 72px 边距，
/// 并保底 320x240；桌面尺寸探测异常时原样返回）。
pub fn clamp_to_desktop(width: f32, height: f32) -> (f32, f32) {
    let desktop_width = virtual_desktop_width();
    let desktop_height = unsafe { GetSystemMetrics(1) as f32 }; // SM_CYSCREEN
    if desktop_width <= 0.0 || desktop_height <= 0.0 {
        return (width, height);
    }
    // 留边距，避免贴边/被任务栏遮挡。
    let max_width = (desktop_width - 24.0).max(320.0);
    let max_height = (desktop_height - 72.0).max(240.0);
    (width.min(max_width), height.min(max_height))
}

/// 初始窗口位置：按桌面居中（iced 的 `Position::Centered` 在 DPI 不一致时会算错）。
///
/// 为什么需要：`Position::Centered` 是 iced 按自己的缩放理解算的，在本文件描述的那种
/// 不一致状态下会把窗口放到屏幕外；自己按虚拟桌面尺寸算才可靠。
/// 入参/出参：`width`/`height` 为已收敛的窗口尺寸；返回左上角 `(x, y)`（逻辑单位，
/// 负数收敛为 0；桌面尺寸探测异常时返回 `(0.0, 0.0)`）。
pub fn centered_position(width: f32, height: f32) -> (f32, f32) {
    let desktop_width = virtual_desktop_width();
    let desktop_height = unsafe { GetSystemMetrics(1) as f32 };
    if desktop_width <= 0.0 || desktop_height <= 0.0 {
        return (0.0, 0.0);
    }
    (
        ((desktop_width - width) / 2.0).max(0.0),
        ((desktop_height - height) / 2.0).max(0.0),
    )
}
