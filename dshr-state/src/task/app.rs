//! AppState：UI 看到的任务页入口（backend-thread 模式，DESIGN §9.5）。
//!
//! 架构：UI 主线程持 [`AppState`]（两个通道的薄壳）；
//! 后台线程自建 tokio runtime，跑 [`Engine`]（收命令）——
//! 每个 runtime 一个 [`RuntimeTask`]（持有 Bridge，select 事件/stderr/命令，落库 + 转 UI）。

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::core::config::Config;
use crate::core::store::Store;
use crate::task::Engine;
use crate::task::events::UiEvent;
use crate::{Error, task::command::Command};

/// 事件接收端（Arc<tokio::sync::Mutex> 包一层：iced Subscription 要 clone 拿走常驻收流，
/// UI 不再 100ms 轮询 try_recv；tokio Mutex 的 guard 跨 await 是 Send，subscription 里直接锁）。
pub type EventReceiver = Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<UiEvent>>>;

/// UI 侧句柄（线程边界：UI 线程持这个，后台线程持 Engine）。
#[derive(Debug)]
pub struct AppState {
    commands: mpsc::UnboundedSender<Command>,
    events: EventReceiver,
}

impl AppState {
    /// 启动后台引擎线程，返回 UI 句柄。
    /// 接收：Config（加载后的全部配置）。
    /// 处理：开 tokio 当前线程 runtime → Engine 常驻收命令。
    /// 生成：AppState（命令/事件两个通道的 UI 端）。
    pub fn start(config: Config) -> Result<Self, Error> {
        let store = Store::open(&config.db_path.to_string_lossy())?.share();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let events = Arc::new(tokio::sync::Mutex::new(ev_rx));

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime 启动失败");
            rt.block_on(async move {
                let mut engine = Engine::new(config, store, ev_tx);
                engine.run(cmd_rx).await;
            });
        });

        Ok(Self {
            commands: cmd_tx,
            events,
        })
    }

    /// 事件接收端（clone 一份给 Iced Subscription 常驻收流）。
    /// 接收：无。
    /// 处理：Arc clone（底层是同一个 channel）。
    /// 生成：可 move 进 subscription 的接收端。
    pub fn take_events(&self) -> EventReceiver {
        self.events.clone()
    }

    /// 发一个命令（不阻塞，后台线程异步处理）。
    pub fn send_command(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
    }

    /// 收一个事件（Iced Subscription 轮询用；事件流改造后仅供测试）。
    pub async fn recv_event(&self) -> Option<UiEvent> {
        self.events.lock().await.recv().await
    }

    /// 非阻塞收一个事件（Iced update 轮询 tick 用；事件流改造后仅供测试）。
    pub fn try_recv(&self) -> Option<UiEvent> {
        self.events.try_lock().ok()?.try_recv().ok()
    }
}
