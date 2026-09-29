//! bridge：UI 与 dsh runtime 之间的总线（s3 真桥，DESIGN.md §7.2；2026-09
//! engine 下沉后只做「搬运」）。
//!
//! 分层（dshr/DESIGN.md §3）：UI(薄) → dshr-state::engine(核心数据处理) →
//! dshr-state::raw(与 SDK 沟通) → dsh-sdk-client → runtime。s3 曾把常驻 worker
//! （Machine/RealBridge，worker.rs + real.rs）放在本 crate、让 UI 直接 import
//! dsh-sdk-client——已整体迁入 state 侧（判定/装配/事件循环/fold→快照 + 落库 +
//! WireLog 全在 state），本文件只剩：
//! - 类型搬运：`BridgeCmd` = engine 的 `EngineCmd` 直别名；
//!   `BridgeEvent` = engine 事件原样透传 + 总线自己的 Ready（命令发送端交付）。
//! - 订阅装配：常驻一条 iced 订阅（engine 进程驱动）＋一条命令通道（App → engine）。
//!   数据管线：runtime → SDK 事件 → engine 折叠 + 落库 → Snapshot 事件 → App。
//!
//! 注：M1 阶段 `Runtime` 仍由 `dshr_state::raw` 提供（`engine` 模块是其再出口）；
//! M3 起 engine 落成多 runtime 注册表，`Engine` 类型由 engine 自己提供。
//!
//! iced 0.14 注意（读 iced_futures-0.14 subscription/tracker.rs 核实）：
//! 订阅身份不变时，已结束的流**不会被重建**——所以总线流永远不自行结束，
//! runtime 的启停全部在 engine 状态机内部完成。
//!
//! 主要用途：给 `App` 两条接缝——一条「事件进来的订阅」（`subscribe()`，engine 事件 +
//! Ready 发送端）与一条「命令出去的通道」（`BridgeCmd`，Ready 交付的 tokio mpsc 发送端）。
//! 为什么需要：UI 不能直接持有 runtime/进程（分层约束：`ui → engine → raw → client`），
//! 也不能在自己的 crate 里 spawn engine 的任务调度；iced 的订阅正好是「把外部异步世界
//! 接进 update」的官方机制。把总线单独成文件，也让 `app.rs` 只依赖消息类型、不依赖 iced
//! 的 futures/stream API。它约束：订阅身份必须是常量（见下 `BusKey`），且流不得结束。
//! 上接：`app::App::subscription`（装配）、`app::App::handle_bridge`（消费事件）。
//! 下接：`dshr_state::engine`（Cmd/Event 类型 → 再出口自 `dshr_state::raw`）、
//! `dshr_state::raw::Runtime`（`Runtime::new(cmd_rx)` / `Runtime::next()`）。
//! 官方对应：无——官方客户端与 runtime 同进程（`packages/client/connection` + slots/注入），
//! 没有跨进程总线这一层；dshr 的它对应官方「客户端 store ↔ 组件」之间那条内部通道的 Rust 替身。
use iced::futures::SinkExt;
use iced::futures::channel::mpsc;
use iced::{Subscription, stream};

use dshr_state::engine::Engine;

/// UI → engine 命令（直别名；App 的按钮/发送动作翻译成命令走命令通道）。
///
/// 为什么需要：用 `pub use` 直别名而不是新建包装枚举，是为了让「加命令」只改 state 一处；
/// UI 侧写 `BridgeCmd::Prompt{..}` 与 state 侧 `EngineCmd::Prompt{..}` 是同一个类型，
/// 不存在需要同步的映射代码（代价：UI 编译期就绑定 state 的命令集）。
pub use dshr_state::engine::EngineCmd as BridgeCmd;

/// engine 事件类型（bridge 层透传；App 模式匹配经 `BridgeEvent::Engine(..)` 嵌套）。
pub use dshr_state::engine::EngineEvent;

/// 会话标识（engine 生成）。UI 侧的会话行 id 用它，避免与 runtime id 混用裸字符串。
pub use dshr_state::engine::SessionId;

/// runtime 标识。UI 侧单槽视图保存"当前那一个"的 id，发命令时带上（engine 按它路由）。
pub use dshr_state::engine::RuntimeId;

/// 命令/事件通道容量（UI 事件低频，够用；命令通道 = tokio mpsc）。
///
/// 为什么需要：两端都要求有界通道，容量太小会在引擎瞬时放量（快照连发）时丢事件；
/// 取 128 是因为事件到达速率受折叠节奏限制、远低于此，正常路径不会触发背压。
pub const CHANNEL_CAP: usize = 128;

/// engine → UI 事件。Ready 是总线装配事件（bridge 自产：把命令发送端交给 App）；
/// 其余四个变体（Started/Snapshot/Stopped/Failed）是 engine 事件原样透传，
/// 语义见 `EngineEvent`。
///
/// 为什么需要：App 需要一个「命令通道现在可用」的信号才能从 pending 转为真正发命令；
/// 把这个握手信号与 engine 事件合成同一个流，App 只订阅一次、只处理一个消息类型。
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    /// 总线就绪：把命令通道发送端交给 App（此前 App 收到的 NewRuntime 会补发）。
    Ready(tokio::sync::mpsc::Sender<BridgeCmd>),
    /// engine 事件（Started/Snapshot/Stopped/Failed）。
    Engine(EngineEvent),
}

/// 订阅标识：恒等 → iced 只建一次总线流（见模块头注释：流不自行结束）。
///
/// 为什么需要：`Subscription::run_with` 按 key 做去重/重建判定；ZST 常量键让身份永不变，
/// 于是流只在进程内建一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BusKey;

/// 常驻总线订阅（App::subscription 恒返回；engine 空闲时零开销等待命令）。
///
/// 为什么需要：`App::subscription` 每帧都会被调用，返回的 `Subscription` 必须是可比较的
/// 惰性描述（而非立即启动的流）；`run_with(BusKey, bus)` 正好把「身份」与「构造器」分开。
/// 入参/出参：无入参；返回产出 `BridgeEvent` 的订阅描述。
pub fn subscribe() -> Subscription<BridgeEvent> {
    Subscription::run_with(BusKey, bus)
}

/// 总线流构造器（Subscription::run_with 的 builder：fn 指针，不捕获）。
/// `+ use<>`：edition 2024 精确捕获——流不借 &BusKey 的生命周期（ZST 键），
/// 否则 RPIT 默认捕获一切入参生命周期，无法匹配 run_with 的 fn(&D) -> S。
///
/// 为什么需要：这是「iced 世界」与「engine 世界」真正接上的地方：先交出命令发送端（Ready），
/// 再驱动 `Runtime` 的事件循环并把每批事件推进流；它保证的是**命令先可用、事件后到达**这个
/// 顺序（否则用户在新 runtime 按钮上的第一次点击会丢）。
/// 入参/出参：`&BusKey`（不用）；返回产出 `BridgeEvent` 的流。
/// 结束条件：命令通道关闭（App 退出）或下游流关闭——两者都表示整个 UI 已不存在。
fn bus(_: &BusKey) -> impl iced::futures::Stream<Item = BridgeEvent> + use<> {
    // iced::stream::channel(size, async closure)：闭包持发送端跑"未来"世界，
    // send 出去的元素构成流（签名见 iced_futures-0.14 stream.rs；用法同其文档示例）。
    stream::channel(CHANNEL_CAP, async |mut out: mpsc::Sender<BridgeEvent>| {
        // 总线主体：先交命令发送端（Ready）→ 驱动 engine 的事件循环并把事件转发到流。
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
