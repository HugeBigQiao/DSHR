//! bridge：UI 与 dsh runtime 之间的总线（s3 真桥，DESIGN §11.4 / M3.6）。
//!
//! 替换旧 `PlaceholderBridge`：不再是「new 时给一份演示数据」的一次性占位，
//! 而是常驻一条 iced 订阅（worker 进程驱动）＋一条命令通道（App → worker）。
//! 数据管线：runtime（真实 dsh 或 fake）→ SDK 事件 → worker 折叠（real.rs 的
//! RealBridge/Folder）→ Snapshot 事件 → App 刷新 model 视图模型。
//!
//! 实现拆分（目录化 src/bridge/ 留待整理，当前为三个顶层模块）：
//! - bridge.rs   类型 + 订阅装配（本文件）
//! - real.rs     Fake/Real 判定 + spawn/initialize 装配 + 折叠桥（Folder 持有）
//! - worker.rs   常驻驱动循环（Machine：命令处理 + SDK 通知读循环）
//!
//! iced 0.14 注意（读 iced_futures-0.14 subscription/tracker.rs 核实）：
//! 订阅身份不变时，已结束的流**不会被重建**——所以总线流永远不自行结束，
//! runtime 的启停全部在 worker 状态机内部完成。
use iced::futures::SinkExt;
use iced::futures::channel::mpsc;
use iced::{Subscription, stream};

use dshr_state::snapshot::SessionSnapshot;

use crate::worker;

/// 命令/事件通道容量（UI 事件低频，够用）。
pub const CHANNEL_CAP: usize = 128;

/// UI → worker 命令（App 的按钮/发送动作翻译成命令走命令通道）。
#[derive(Debug, Clone)]
pub enum BridgeCmd {
    /// 启动 runtime（Fake/Real 由 real.rs 在启动时刻判定）。已在运行 → 忽略
    /// （重启 = 先 Stop 再 Start）。
    Start,
    /// 停止 runtime（协议 shutdown + dispose 阶梯）；消息流保留在 UI。
    Stop,
    /// 重置当前会话（新建会话语义：Folder 清空 + 新 session id）。
    ResetSession,
    /// 向当前会话发一条用户消息（session/prompt）。
    Prompt { text: String },
}

/// worker → UI 事件。
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    /// 总线就绪：把命令通道发送端交给 App（此前 App 收到的 NewRuntime 会补发）。
    Ready(mpsc::Sender<BridgeCmd>),
    /// runtime 启动完成（label = 模式说明，侧边栏展示；随后跟随空会话 Snapshot）。
    Started { label: String, session_id: String },
    /// 会话快照整体发送（折叠态；每次有变化的通知后发一次——单会话/轻量场景
    /// 可接受，事件密集时的增量传输（只发变化行）留后续）。
    Snapshot(Box<SessionSnapshot>),
    /// 正常停止（用户操作：删除/归档 runtime）。
    Stopped { reason: String },
    /// 启动/运行异常（进程退出、spawn/initialize/prompt 失败等；红色提示）。
    Failed { reason: String },
}

/// 订阅标识：恒等 → iced 只建一次总线流（见模块头注释：流不自行结束）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BusKey;

/// 常驻总线订阅（App::subscription 恒返回；worker 空闲时零开销等待命令）。
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
        // 总线主体：先交命令发送端（Ready）→ 驱动 worker Machine 并把事件转发到流。
        let (cmd_tx, cmd_rx) = mpsc::channel(CHANNEL_CAP);
        // 发送端先交出去（App 收到 Ready 前点「新建 runtime」会缓存 pending_start 补发）。
        if out.send(BridgeEvent::Ready(cmd_tx)).await.is_err() {
            return; // App 已退出。
        }
        let mut machine = worker::Machine::new(cmd_rx);
        loop {
            // Machine::next 返回 None = 命令通道关闭（App 退出）→ 总线结束。
            let Some(events) = machine.next().await else {
                break;
            };
            for ev in events {
                if out.send(ev).await.is_err() {
                    return;
                }
            }
        }
    })
}
