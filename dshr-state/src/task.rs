//! 任务页运行时：Engine（多 runtime 生命周期）+ RuntimeTask（单 runtime 常驻）。
//!
//! 分层（每文件 ≤400 行，决策 22）：
//! - [`command`]：Command（UI → state）
//! - [`events`]：UiEvent（state → UI）
//! - [`app`]：AppState（UI 句柄，两个通道）
//! - [`bridge`]：runtime 对接（RtInfo/Bridge）
//! - [`request`]：RuntimeTask::handle_cmd（dshr → dsh 请求侧）
//! - [`workspace`]：文件树读取
//! - [`event`]：dsh → dshr 事件处理（48 事件按族拆）

pub mod app;
pub mod bridge;
pub mod command;
pub mod event;
pub mod events;
pub mod fetch;
pub mod request;
pub mod workspace;

pub use app::EventReceiver;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use dshr_protocol::rpc::Notification;
use tokio::sync::mpsc;

use crate::core::config::Config;
use crate::core::session::SessionTracker;
use crate::core::store::Store;
use crate::task::bridge::{Bridge, RtInfo};
use crate::task::command::Command;
use crate::task::events::{UiEvent, UiStatus};
use crate::toast;

/// 当前 epoch 毫秒（Engine/RuntimeTask 公共时间源）。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// runtime 任务收到的命令（Engine 分发后的子集）。
pub enum RtCmd {
    NewSession {
        session_id: String,
    },
    Send {
        session_id: String,
        text: String,
    },
    Rename {
        name: String,
    },
    /// 补设工作区（= 带新 cwd 重新 initialize，幂等；参数来自 Engine 持有的 config）。
    SetWorkspace {
        cwd: String,
        provider: String,
        model: String,
        max_tokens: u64,
    },
    RenameSession {
        session_id: String,
        name: String,
    },
    ArchiveSession {
        session_id: String,
    },
    DeleteSession {
        session_id: String,
    },
    ListWorkspace {
        path: String,
    },
    Archive,
    Delete,
    Shutdown,
}

/// 后台引擎：管理多个 runtime 任务（进程管理器雏形，DESIGN §9.5）。
pub struct Engine {
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
    pub fn new(
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
                Command::SetWorkspace { runtime_id, cwd } => {
                    if let Some(tx) = self.runtimes.get(&runtime_id) {
                        let _ = tx.send(RtCmd::SetWorkspace {
                            cwd,
                            provider: self.config.dshr.provider.clone(),
                            model: self.config.dshr.model.clone(),
                            max_tokens: self.config.dshr.max_tokens,
                        });
                    }
                }
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
                Command::DeleteRuntime { runtime_id } => {
                    if let Some(tx) = self.runtimes.remove(&runtime_id) {
                        let _ = tx.send(RtCmd::Delete);
                    }
                }
                Command::RenameSession { session_id, name } => {
                    if let Some(rt_id) = self.sessions.get(&session_id).cloned() {
                        if let Some(tx) = self.runtimes.get(&rt_id) {
                            let _ = tx.send(RtCmd::RenameSession { session_id, name });
                        }
                    }
                }
                Command::ArchiveSession { session_id } => {
                    if let Some(rt_id) = self.sessions.get(&session_id).cloned() {
                        if let Some(tx) = self.runtimes.get(&rt_id) {
                            let _ = tx.send(RtCmd::ArchiveSession { session_id });
                        }
                    }
                }
                Command::DeleteSession { session_id } => {
                    if let Some(rt_id) = self.sessions.get(&session_id).cloned() {
                        self.sessions.remove(&session_id);
                        if let Some(tx) = self.runtimes.get(&rt_id) {
                            let _ = tx.send(RtCmd::DeleteSession { session_id });
                        }
                    }
                }
                Command::ListWorkspace { runtime_id, path } => {
                    if let Some(tx) = self.runtimes.get(&runtime_id) {
                        let _ = tx.send(RtCmd::ListWorkspace { path });
                    }
                }
                Command::FetchDsh => {
                    // 下载/更新 dsh：data/ 的上级 = workspace，dsh 目录与 data 同级。
                    // 镜像源来自 config.json 的 npm_registry（空 = 官方 registry）。
                    let dsh_dir = self
                        .config
                        .data_dir
                        .parent()
                        .map(|p| p.join("dsh"))
                        .unwrap_or_else(|| PathBuf::from("dsh"));
                    let registry = self.config.dshr.npm_registry.clone();
                    let ev_tx = self.ev_tx.clone();
                    tokio::spawn(async move {
                        fetch::fetch(dsh_dir, &registry, ev_tx).await;
                    });
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
    /// 接收：显示名 + 可选工作区（决策 21：可不设，设置后锁死）。
    /// 处理：node carrier 双路径——dsh 已安装用 dsh 运行时（决策 23，node + lib/bin.js + cordis.yml，
    ///       进程目录 = dsh/）；否则回退官方仓库（tsx + jsonrpc-demo，进程目录 = harness_root）。
    ///       工作区经 DSH_CWD/InitializeParams.cwd 锁死。
    /// 生成：runtimes 行 + 一个常驻 RuntimeTask（失败时归档并报 UI 错误）。
    async fn start_runtime(&mut self, name: String, workspace: Option<String>) {
        let id = self.gen_id("rt");
        let created_at = now_ms();
        // 双路径选择：dsh 运行时优先（与 data 同级），否则官方仓库（tsx）。
        let dsh_dir = self
            .config
            .data_dir
            .parent()
            .map(|p| p.join("dsh"))
            .unwrap_or_else(|| std::path::PathBuf::from("dsh"));
        let (args, process_dir) = if fetch::is_installed(&dsh_dir) {
            (
                vec![
                    fetch::BIN_ENTRY.to_string(),
                    self.config.cordis_path.to_string_lossy().to_string(),
                ],
                dsh_dir,
            )
        } else {
            (
                vec![
                    "--import".to_string(),
                    "tsx".to_string(),
                    "packages/examples/jsonrpc-demo/src/bin.ts".to_string(),
                    "examples/jsonrpc-agent/cordis.yml".to_string(),
                ],
                std::path::PathBuf::from(&self.config.dshr.harness_root),
            )
        };
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
            process_dir: process_dir.to_string_lossy().to_string(),
            workspace: workspace.clone(),
            created_at,
        };

        let _ = self.ev_tx.send(UiEvent::Status {
            runtime_id: id.clone(),
            status: UiStatus::Connecting,
            name: name.clone(),
            workspace: workspace.clone(),
        });

        // spawn 前校验 API key（secrets.json）；缺失是配置错误，UI 启动时已提示。
        let api_key = self.config.secrets.api_key.clone().unwrap_or_default();
        match Bridge::spawn(info.clone(), &api_key, &self.config.dshr.session_root).await {
            Ok(mut bridge) => {
                match bridge
                    .initialize(
                        &self.config.dshr.provider,
                        &self.config.dshr.model,
                        Some(self.config.dshr.max_tokens),
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
                            auto_named: true,
                            titled: false,
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
                        toast!(&self.ev_tx, "runtime {name} initialize 失败: {e}");
                        // 状态同步：侧边栏不再显示 ⏳，而是 ⏹（启动失败可见）。
                        let _ = self.ev_tx.send(UiEvent::Status {
                            runtime_id: id,
                            status: UiStatus::Closed,
                            name,
                            workspace,
                        });
                    }
                }
            }
            Err(e) => {
                let _ = self.store.lock().unwrap().archive_runtime(&id);
                toast!(&self.ev_tx, "runtime {name} 启动失败: {e}");
                // 状态同步：侧边栏不再显示 ⏳，而是 ⏹（启动失败可见）。
                let _ = self.ev_tx.send(UiEvent::Status {
                    runtime_id: id,
                    status: UiStatus::Closed,
                    name,
                    workspace,
                });
            }
        }
    }
}

/// 单个 runtime 的常驻任务：select 事件/stderr/命令。
pub struct RuntimeTask {
    /// Option 包装：shutdown 消费 self 时 take 出来。
    pub bridge: Option<Bridge>,
    /// 事件流接收端（启动时从 bridge 拆出）。
    pub events_rx: mpsc::UnboundedReceiver<Notification>,
    /// stderr 行接收端（启动时从 bridge 拆出）。
    pub stderr_rx: mpsc::UnboundedReceiver<String>,
    pub info: RtInfo,
    pub store: Arc<Mutex<Store>>,
    pub ev_tx: mpsc::UnboundedSender<UiEvent>,
    /// session_id → 状态机（子会话事件也会来，懒创建）。
    pub trackers: HashMap<String, SessionTracker>,
    /// 自动命名跟随：true 时首个会话标题会同步成 runtime 名；手动改名后置 false。
    pub auto_named: bool,
    /// 是否已自动命名过（避免每次标题都改 runtime 名）。
    pub titled: bool,
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
}
