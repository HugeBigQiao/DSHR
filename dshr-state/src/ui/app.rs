//! AppState：UI 看到的唯一入口（backend-thread 模式，DESIGN §9.5）。
//!
//! 架构：UI 主线程持 [`AppState`]（两个通道的薄壳）；
//! 后台线程自建 tokio runtime，跑 [`Engine`]（收命令）——
//! 每个 runtime 一个 [`RuntimeTask`]（持有 Bridge，select 事件/stderr/命令，落库 + 转 UI）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dshr_protocol::rpc::Notification;
use tokio::sync::mpsc;

use crate::bridge::RtInfo;
use crate::core::config::Config;
use crate::core::session::SessionTracker;
use crate::core::store::Store;
use crate::core::transcode;
use crate::ui::{UiEvent, UiStatus};
use crate::{Error, ui::command::Command};

/// UI 侧句柄（线程边界：UI 线程持这个，后台线程持 Engine）。
#[derive(Debug)]
pub struct AppState {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<UiEvent>,
}

impl AppState {
    /// 启动后台引擎线程，返回 UI 句柄。
    /// 接收：Config（.env 加载后的全部配置）。
    /// 处理：开 tokio 当前线程 runtime → Engine 常驻收命令。
    /// 生成：AppState（命令/事件两个通道的 UI 端）。
    pub fn start(config: Config) -> Result<Self, Error> {
        let store = Store::open(&config.db_path.to_string_lossy())?.share();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();

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
            events: ev_rx,
        })
    }

    /// 发一个命令（不阻塞，后台线程异步处理）。
    pub fn send_command(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
    }

    /// 收一个事件（Iced Subscription 轮询用）。
    pub async fn recv_event(&mut self) -> Option<UiEvent> {
        self.events.recv().await
    }

    /// 非阻塞收一个事件（Iced update 轮询 tick 用）。
    pub fn try_recv(&mut self) -> Option<UiEvent> {
        self.events.try_recv().ok()
    }
}

/// runtime 任务收到的命令（Engine 分发后的子集）。
enum RtCmd {
    NewSession { session_id: String },
    Send { session_id: String, text: String },
    Rename { name: String },
    Archive,
    Shutdown,
}

/// 后台引擎：管理多个 runtime 任务。
struct Engine {
    config: Config,
    store: Arc<Mutex<Store>>,
    ev_tx: mpsc::UnboundedSender<UiEvent>,
    /// runtime_id → 命令通道。
    runtimes: HashMap<String, mpsc::UnboundedSender<RtCmd>>,
    /// session_id → runtime_id（Send 时定位）。
    sessions: HashMap<String, String>,
    next_id: u64,
}

impl Engine {
    fn new(
        config: Config,
        store: Arc<Mutex<Store>>,
        ev_tx: mpsc::UnboundedSender<UiEvent>,
    ) -> Self {
        Self {
            config,
            store,
            ev_tx,
            runtimes: HashMap::new(),
            sessions: HashMap::new(),
            next_id: 1,
        }
    }

    /// 生成唯一 id（时间戳 + 自增计数器，够用且无外部依赖）。
    fn gen_id(&mut self, prefix: &str) -> String {
        let n = self.next_id;
        self.next_id += 1;
        format!("{prefix}-{}-{n}", now_ms())
    }

    /// 主循环：收 UI 命令 → 分发。
    async fn run(&mut self, mut cmds: mpsc::UnboundedReceiver<Command>) {
        while let Some(cmd) = cmds.recv().await {
            match cmd {
                Command::Start { name, cwd } => self.start_runtime(name, cwd).await,
                Command::NewSession { runtime_id } => {
                    let session_id = self.gen_id("sess");
                    if let Some(tx) = self.runtimes.get(&runtime_id) {
                        self.sessions.insert(session_id.clone(), runtime_id.clone());
                        let _ = tx.send(RtCmd::NewSession { session_id });
                    }
                }
                Command::Send { session_id, text } => {
                    if let Some(rt_id) = self.sessions.get(&session_id).cloned() {
                        if let Some(tx) = self.runtimes.get(&rt_id) {
                            let _ = tx.send(RtCmd::Send { session_id, text });
                        }
                    }
                }
                Command::RenameRuntime { runtime_id, name } => {
                    if let Some(tx) = self.runtimes.get(&runtime_id) {
                        let _ = tx.send(RtCmd::Rename { name });
                    }
                }
                Command::ArchiveRuntime { runtime_id } => {
                    if let Some(tx) = self.runtimes.remove(&runtime_id) {
                        let _ = tx.send(RtCmd::Archive);
                    }
                }
                Command::Shutdown => {
                    for tx in self.runtimes.values() {
                        let _ = tx.send(RtCmd::Shutdown);
                    }
                    self.runtimes.clear();
                    break;
                }
            }
        }
    }

    /// 添加 runtime：落库 → spawn → initialize → 起 RuntimeTask。
    /// 接收：显示名 + 工作区路径。
    /// 处理：node carrier 参数（tsx + jsonrpc-demo bin + cordis.yml，进程目录 = harness_root）、
    ///       工作区经 DSH_CWD/InitializeParams.cwd 锁死。
    /// 生成：runtimes 行 + 一个常驻 RuntimeTask（失败时归档并报 UI 错误）。
    async fn start_runtime(&mut self, name: String, workspace: String) {
        let id = self.gen_id("rt");
        let created_at = now_ms();
        let args = vec![
            "--import".to_string(),
            "tsx".to_string(),
            "packages/examples/jsonrpc-demo/src/bin.ts".to_string(),
            "examples/jsonrpc-agent/cordis.yml".to_string(),
        ];
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "[]".to_string());

        // 先落库（state=active），失败再改 archived。
        let _ = self.store.lock().unwrap().insert_runtime(
            &id,
            &name,
            "active",
            created_at,
            "node",
            Some(&args_json),
            None,
            None,
        );

        let info = RtInfo {
            id: id.clone(),
            name: name.clone(),
            command: "node".to_string(),
            args,
            process_dir: self.config.harness_root.clone(),
            workspace: workspace.clone(),
            created_at,
        };

        let _ = self.ev_tx.send(UiEvent::Status {
            runtime_id: id.clone(),
            status: UiStatus::Connecting,
            name: name.clone(),
            workspace: workspace.clone(),
        });

        match crate::bridge::Bridge::spawn(
            info.clone(),
            &self.config.api_key,
            &self.config.session_root,
        )
        .await
        {
            Ok(mut bridge) => {
                match bridge
                    .initialize(
                        &self.config.provider,
                        &self.config.model,
                        self.config.max_tokens,
                    )
                    .await
                {
                    Ok((sname, sver)) => {
                        // initialize 请求落库（进程级：无 session/turn）。
                        let _ = self.store.lock().unwrap().insert_request(
                            &id,
                            None,
                            None,
                            created_at,
                            "initialize",
                            None,
                            true,
                            None,
                        );
                        let _ = sname; // serverInfo 校验放测试里，UI 只显示 Ready
                        let _ = sver;
                        let (tx, rx) = mpsc::unbounded_channel();
                        // 拆出接收端（move 进任务，select 需要独占）。
                        let events_rx = bridge.take_events();
                        let stderr_rx = bridge.take_stderr();
                        let task = RuntimeTask {
                            bridge: Some(bridge),
                            events_rx,
                            stderr_rx,
                            info,
                            store: self.store.clone(),
                            ev_tx: self.ev_tx.clone(),
                            trackers: HashMap::new(),
                        };
                        tokio::spawn(task.run(rx));
                        self.runtimes.insert(id.clone(), tx);
                        // Ready 必须在此之后发：UI 收到后会回发 NewSession，
                        // 若先发则 Engine 的 runtimes 还没有这个 runtime，命令会被丢。
                        let _ = self.ev_tx.send(UiEvent::Status {
                            runtime_id: id,
                            status: UiStatus::Ready,
                            name,
                            workspace,
                        });
                    }
                    Err(e) => {
                        let _ = self.store.lock().unwrap().archive_runtime(&id);
                        let _ = self.ev_tx.send(UiEvent::Error(format!(
                            "runtime {name} initialize 失败: {e}"
                        )));
                    }
                }
            }
            Err(e) => {
                let _ = self.store.lock().unwrap().archive_runtime(&id);
                let _ = self
                    .ev_tx
                    .send(UiEvent::Error(format!("runtime {name} 启动失败: {e}")));
            }
        }
    }
}

/// 单个 runtime 的常驻任务：select 事件/stderr/命令。
struct RuntimeTask {
    /// Option 包装：shutdown 消费 self 时 take 出来。
    bridge: Option<crate::bridge::Bridge>,
    /// 事件流接收端（启动时从 bridge 拆出）。
    events_rx: mpsc::UnboundedReceiver<Notification>,
    /// stderr 行接收端（启动时从 bridge 拆出）。
    stderr_rx: mpsc::UnboundedReceiver<String>,
    info: RtInfo,
    store: Arc<Mutex<Store>>,
    ev_tx: mpsc::UnboundedSender<UiEvent>,
    /// session_id → 状态机（子会话事件也会来，懒创建）。
    trackers: HashMap<String, SessionTracker>,
}

impl RuntimeTask {
    /// 主循环：三路 select，任一通道关闭（runtime 退出）即结束。
    async fn run(mut self, mut cmds: mpsc::UnboundedReceiver<RtCmd>) {
        loop {
            tokio::select! {
                cmd = cmds.recv() => {
                    match cmd {
                        Some(c) => { if !self.handle_cmd(c).await { break; } }
                        None => break,
                    }
                }
                n = self.events_rx.recv() => match n {
                    Some(notification) => self.handle_notification(notification),
                    None => {
                        let _ = self.ev_tx.send(UiEvent::Status {
                            runtime_id: self.info.id.clone(),
                            status: UiStatus::Closed,
                            name: self.info.name.clone(),
                            workspace: self.info.workspace.clone(),
                        });
                        break;
                    }
                },
                line = self.stderr_rx.recv() => {
                    if let Some(line) = line {
                        let _ = self.store.lock().unwrap().insert_log(&self.info.id, now_ms(), "stderr", &line);
                        let _ = self.ev_tx.send(UiEvent::Log { runtime_id: self.info.id.clone(), level: "stderr".to_string(), message: line });
                    }
                }
            }
        }
    }

    /// 处理一条 runtime 命令；返回 false 表示任务应结束。
    async fn handle_cmd(&mut self, cmd: RtCmd) -> bool {
        match cmd {
            RtCmd::NewSession { session_id } => {
                self.ensure_session(&session_id);
                true
            }
            RtCmd::Send { session_id, text } => {
                self.ensure_session(&session_id);
                let t0 = Instant::now();
                let result = match self.bridge.as_mut() {
                    Some(bridge) => bridge.prompt(&session_id, &text).await,
                    None => Err(Error::NotStarted),
                };
                match result {
                    Ok(outcome) => {
                        let duration = t0.elapsed().as_millis() as i64;
                        let _ = self.store.lock().unwrap().insert_request(
                            &self.info.id,
                            Some(&session_id),
                            None, // turn_id 等 turn/start 回填
                            now_ms(),
                            "session_prompt",
                            Some(duration),
                            true,
                            None,
                        );
                        // tracker 记录 prompt 时间（备用；turn/start 回填在 handle_event 做）
                        if let Some(t) = self.trackers.get_mut(&session_id) {
                            t.on_prompt(now_ms());
                        }
                        let _ = outcome.message_id;
                    }
                    Err(e) => {
                        let _ = self.store.lock().unwrap().insert_request(
                            &self.info.id,
                            Some(&session_id),
                            None,
                            now_ms(),
                            "session_prompt",
                            None,
                            false,
                            Some(&e.to_string()),
                        );
                        let _ = self.ev_tx.send(UiEvent::Error(format!("发送失败: {e}")));
                    }
                }
                true
            }
            RtCmd::Rename { name } => {
                let _ = self
                    .store
                    .lock()
                    .unwrap()
                    .update_runtime_name(&self.info.id, &name);
                true
            }
            RtCmd::Archive => {
                let _ = self.store.lock().unwrap().archive_runtime(&self.info.id);
                if let Some(bridge) = self.bridge.take() {
                    let _ = bridge.shutdown().await;
                }
                let _ = self.ev_tx.send(UiEvent::Status {
                    runtime_id: self.info.id.clone(),
                    status: UiStatus::Closed,
                    name: self.info.name.clone(),
                    workspace: self.info.workspace.clone(),
                });
                false
            }
            RtCmd::Shutdown => {
                let _ = self
                    .store
                    .lock()
                    .unwrap()
                    .update_runtime_name(&self.info.id, "closed");
                let _ = self.store.lock().unwrap().archive_runtime(&self.info.id);
                if let Some(bridge) = self.bridge.take() {
                    let _ = bridge.shutdown().await;
                }
                false
            }
        }
    }

    /// 会话首次出现时：建 tracker + 落库 sessions + 告知 UI。
    fn ensure_session(&mut self, session_id: &str) {
        if self.trackers.contains_key(session_id) {
            return;
        }
        self.trackers
            .insert(session_id.to_string(), SessionTracker::new());
        let _ = self.store.lock().unwrap().insert_session(
            session_id,
            &self.info.id,
            &self.info.workspace,
            None,
            now_ms(),
            Some("idle"),
        );
        let _ = self.ev_tx.send(UiEvent::SessionCreated {
            runtime_id: self.info.id.clone(),
            session_id: session_id.to_string(),
        });
    }

    /// 处理一条通知：按 Kind 分发 → 落库 → 转 UI。
    fn handle_notification(&mut self, notification: Notification) {
        match dshr_protocol::notifications::parse(&notification) {
            Ok(Some(kind)) => match kind {
                dshr_protocol::notifications::Kind::SessionEvent(n) => {
                    self.handle_session_event(&n.session_id, &n.event);
                }
                dshr_protocol::notifications::Kind::SessionStatus(n) => {
                    let status = match n.status {
                        dshr_protocol::notifications::SessionStatus::Idle => "idle",
                        dshr_protocol::notifications::SessionStatus::Running => "running",
                    };
                    let _ = self
                        .store
                        .lock()
                        .unwrap()
                        .update_session_status(&n.session_id, status);
                }
                dshr_protocol::notifications::Kind::SubagentStarted(n) => {
                    // 血缘：子会话挂到父会话下（懒创建不覆盖已有行）。
                    if !self.trackers.contains_key(&n.child_session_id) {
                        let _ = self.store.lock().unwrap().insert_session(
                            &n.child_session_id,
                            &self.info.id,
                            &self.info.workspace,
                            Some(&n.parent_session_id),
                            now_ms(),
                            Some("idle"),
                        );
                    }
                }
                dshr_protocol::notifications::Kind::SubagentFinished(_) => {}
            },
            Ok(None) => {} // 未知方法：跳过
            Err(e) => {
                let _ = self
                    .ev_tx
                    .send(UiEvent::Error(format!("通知解析失败: {e}")));
            }
        }
    }

    /// 一条 session.event：lossless 落库（events 表永远先写）→ turn 落库 → 转 UI。
    fn handle_session_event(
        &mut self,
        session_id: &str,
        event: &dshr_protocol::session_event::SessionEvent,
    ) {
        // 1. lossless 底线：events 表 + last_seq 书签。
        let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
        let (turn, step) = event.turn_step();
        let _ = self.store.lock().unwrap().insert_event(
            session_id,
            event.seq() as i64,
            event.event_type(),
            event.time() as i64,
            turn.map(|t| t as i64),
            step.map(|s| s as i64),
            &payload,
        );
        let _ = self
            .store
            .lock()
            .unwrap()
            .update_session_last_seq(session_id, event.seq() as i64);

        // 2. 未知 session 懒创建（子会话事件先于通知到达的情况）。
        let tracker = self
            .trackers
            .entry(session_id.to_string())
            .or_insert_with(SessionTracker::new);

        // 3. turn 生命周期：开行/回填（含 requests.turn_id 回填）。
        match event {
            dshr_protocol::session_event::SessionEvent::TurnStart { time, data, .. } => {
                let turn_id = transcode::on_turn_start(
                    &self.info.id,
                    session_id,
                    data.turn,
                    *time as i64,
                    tracker,
                );
                let _ = self.store.lock().unwrap().insert_turn(
                    &turn_id,
                    &self.info.id,
                    session_id,
                    data.turn as i64,
                    *time as i64,
                );
                let _ = self.store.lock().unwrap().update_request_turn_id(
                    &self.info.id,
                    session_id,
                    &turn_id,
                );
            }
            dshr_protocol::session_event::SessionEvent::TurnEnd { time, data, .. } => {
                if let Some((fin, reason)) =
                    transcode::on_turn_end(&data.reason, *time as i64, tracker)
                {
                    let duration = fin.ended_at.saturating_sub(fin.started_at);
                    let u = fin.usage.as_ref();
                    let _ = self.store.lock().unwrap().finish_turn(
                        &fin.turn_id,
                        Some(fin.ended_at),
                        Some(duration),
                        Some(&reason),
                        u.map(|x| x.input_tokens as i64),
                        u.map(|x| x.output_tokens as i64),
                        u.and_then(|x| x.cache_read_tokens).map(|x| x as i64),
                        u.and_then(|x| x.cache_write_tokens).map(|x| x as i64),
                        u.and_then(|x| x.reasoning_tokens).map(|x| x as i64),
                        fin.user_text.as_deref(),
                        fin.assistant_text.as_deref(),
                    );
                    let _ = self.ev_tx.send(UiEvent::TurnEnd {
                        runtime_id: self.info.id.clone(),
                        session_id: session_id.to_string(),
                        turn: fin.turn,
                        reason,
                        usage: fin.usage,
                    });
                }
            }
            _ => {}
        }

        // 4. 转 UI（消息/工具/标题）。
        if let Some(ui) = transcode::event_to_ui(&self.info.id, session_id, event, tracker) {
            let _ = self.ev_tx.send(ui);
        }
    }
}

/// 当前时间（epoch ms）。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
