//! bridge：UI 与 dsh runtime 之间的总线（s3 真桥，DESIGN §11.4 / M3.6；2026-09
//! engine 下沉后只做「搬运」）。
//!
//! 分层（DESIGN v3 §9.5 / v4 M3.6）：UI(薄) → dshr-state::engine(常驻会话中台) →
//! dsh-sdk-client → runtime。s3 曾把常驻 worker（Machine/RealBridge，worker.rs +
//! real.rs）放在本 crate、让 UI 直接 import dsh-sdk-client——已整体迁入
//! dshr_state::engine（判定/装配/事件循环/fold→快照 + 落库 + WireLog 全在 state
//! 侧），本文件只剩：
//! - 类型搬运：`BridgeCmd` = engine 的 `EngineCmd` 直别名；
//!   `BridgeEvent` = engine 事件原样透传 + 总线自己的 Ready（命令发送端交付）。
//! - 订阅装配：常驻一条 iced 订阅（engine 进程驱动）＋一条命令通道（App → engine）。
//!   数据管线：runtime → SDK 事件 → engine 折叠 + 落库 → Snapshot 事件 → App。
//!
//! iced 0.14 注意（读 iced_futures-0.14 subscription/tracker.rs 核实）：
//! 订阅身份不变时，已结束的流**不会被重建**——所以总线流永远不自行结束，
//! runtime 的启停全部在 engine 状态机内部完成。
use iced::futures::SinkExt;
use iced::futures::channel::mpsc;
use iced::{Subscription, stream};

use dshr_state::engine::Engine;

/// UI → engine 命令（直别名；App 的按钮/发送动作翻译成命令走命令通道）。
pub use dshr_state::engine::EngineCmd as BridgeCmd;

/// engine 事件类型（bridge 层透传；App 模式匹配经 `BridgeEvent::Engine(..)` 嵌套）。
pub use dshr_state::engine::EngineEvent;

/// 命令/事件通道容量（UI 事件低频，够用；命令通道 = tokio mpsc）。
pub const CHANNEL_CAP: usize = 128;

/// engine → UI 事件。Ready 是总线装配事件（bridge 自产：把命令发送端交给 App）；
/// 其余四个变体（Started/Snapshot/Stopped/Failed）是 engine 事件原样透传，
/// 语义见 `EngineEvent`。
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    /// 总线就绪：把命令通道发送端交给 App（此前 App 收到的 NewRuntime 会补发）。
    Ready(tokio::sync::mpsc::Sender<BridgeCmd>),
    /// engine 事件（Started/Snapshot/Stopped/Failed）。
    Engine(EngineEvent),
}

/// 订阅标识：恒等 → iced 只建一次总线流（见模块头注释：流不自行结束）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BusKey;

/// 常驻总线订阅（App::subscription 恒返回；engine 空闲时零开销等待命令）。
pub fn subscribe() -> Subscription<BridgeEvent> {
    Subscription::run_with(BusKey, bus)
}

/// 总线流构造器（Subscription::run_with 的 builder：fn 指针，不捕获）。
/// `+ use<>`：edition 2024 精确捕获——流不借 &BusKey 的生命周期（ZST 键），
/// 否则 RPIT 默认捕获一切入参生命周期，无法匹配 run_with 的 fn(&D) -> S。
fn bus(_: &BusKey) -> impl iced::futures::Stream<Item = BridgeEvent> + use<> {
    // iced::stream::channel(size, async closure)：闭包持发送端跑"未来"世界，
    // send 出去的元素构成流（签名见 iced_futures-0.14 stream.rs；用法同其文档示例）。
    stream::channel(CHANNEL_CAP, async |mut out: mpsc::Sender<BridgeEvent>| {
        // 总线主体：先交命令发送端（Ready）→ 驱动 engine::Engine 并把事件转发到流。
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(CHANNEL_CAP);
        // 发送端先交出去（App 收到 Ready 前点「新建 runtime」会缓存 pending_start 补发）。
        if out.send(BridgeEvent::Ready(cmd_tx)).await.is_err() {
            return; // App 已退出。
        }
        let mut engine = Engine::new(cmd_rx);
        loop {
            // Engine::next 返回 None = 命令通道关闭（App 退出）→ 总线结束。
            let Some(events) = engine.next().await else {
                break;
            };
            for ev in events {
                if out.send(BridgeEvent::Engine(ev)).await.is_err() {
                    return;
                }
            }
        }
    })
}
