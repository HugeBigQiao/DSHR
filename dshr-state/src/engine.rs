//! engine：常驻会话中台（DESIGN §11.4 的 s3 落地；分层 UI(薄) → dshr-state(中台)
//! → dsh-sdk-client → runtime，DESIGN v3 §9.5 / v4 M3.6）。
//!
//! 2026-09 架构对齐：s3 曾把常驻 worker（Machine/RealBridge，dshr-ui/src/worker.rs
//! + real.rs）错放在 dshr-ui 侧、让 UI 直接 import dsh-sdk-client/dsh-sdk-protocol，
//! 旁路 state 层——本模块把整条「常驻驱动循环」下沉进 dshr-state：
//!   - 模式判定 + spawn 装配：子模块 mode（resolve_mode/kit/workspace_root，原样迁自
//!     dshr-ui real.rs，Fake = node 跑 fake_runtime.mjs；Real = config::load +
//!     runtime::ensure + env 对齐 session.rs）；
//!   - Engine（≈ 原 worker.rs 的 Machine）：select（命令通道 ∨ SDK 通知广播）→ 事件批；
//!     Start/Stop/ResetSession/Prompt 语义与原实现逐条对应，注释保留；
//!   - 折叠桥（原 RealBridge，engine 私有）：Folder 持有 + wire 通知喂入 + 本地合成
//!     user/message（fake 不回发）→ 快照；
//!   - 落库接线（s2，新增）：每次「快照有变化并即将整发」同步 persist_snapshot；
//!     Stop/拆除（teardown）对当前折叠快照补一次收尾 persist。Store 默认在首次
//!     Start 时惰性 Store::open(default_db_path())（data/ 自动建目录），测试经
//!     `with_db` 注入内存库。同一会话重复事件幂等——store 已是整体替换语义。
//!   - WireLog 接线（新增）：Start 时把 `data/wire-logs/<label 化>-<epoch>.jsonl`
//!     填进 HarnessSpawnConfig.wire_log_path。口径（已读 dsh-sdk-client transport.rs/
//!     process.rs 核实）：该字段 = SDK 侧**全程线级记录路径**——HarnessClient::spawn
//!     自开 WireLog，transport 读循环对请求/响应/通知/无法分类逐行写 cat="dsh" JSONL
//!     （record_send/record_recv，双向全量）；app 侧轨迹（record.rs Recorder 的
//!     record_app 合并写同一文件）本模块暂未接——需要时 Start 开 Recorder 并把同一
//!     路径给 config 即可，两路互不冲突。
//!
//! 不依赖 iced/dshr-ui：命令通道用 tokio::sync::mpsc、SDK 事件广播 tokio broadcast，
//! UI 侧（dshr-ui bridge）只搬运 EngineCmd/EngineEvent。
use std::future::Future;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use dsh_sdk_client::client::HarnessClient;
use dsh_sdk_protocol::notifications::{self, Kind, SessionStatusNotification};
use dsh_sdk_protocol::requests::{SdkPromptContentBlock, SessionPromptParams};
use dsh_sdk_protocol::rpc::Notification;
use dsh_sdk_protocol::session_event::SessionEvent;
use tokio::sync::{broadcast, mpsc};

use crate::fold::Folder;
use crate::snapshot::SessionSnapshot;
use crate::store::{Store, default_db_path};

mod mode;

pub use mode::{ResolvedMode, RuntimeMode, SpawnKit, kit, resolve_mode, workspace_root};

/// 共享协议类型再出口：会话快照（crate::snapshot::SessionSnapshot）的
/// status/usage 字段用这两个类型，UI 层经本出口使用、不再直接依赖
/// dsh-sdk-protocol（DESIGN v4 M3.6：UI 只搬运命令/事件）。
pub use dsh_sdk_protocol::{llm::TokenUsage, notifications::SessionStatus};

/// UI → engine 命令（原 dshr-ui bridge.rs BridgeCmd 迁入，语义不变）。
#[derive(Debug, Clone)]
pub enum EngineCmd {
    /// 启动 runtime（Fake/Real 由 mode::resolve_mode 在启动时刻判定）。已在运行 → 忽略
    /// （重启 = 先 Stop 再 Start）。
    Start,
    /// 停止 runtime（协议 shutdown + dispose 阶梯）；消息流保留在 UI。
    Stop,
    /// 重置当前会话（新建会话语义：Folder 清空 + 新 session id）。
    ResetSession,
    /// 向当前会话发一条用户消息（session/prompt）。
    Prompt { text: String },
}

/// engine → UI 事件（原 dshr-ui bridge.rs BridgeEvent 迁入；Ready 是总线装配事件，
/// 留在 dshr-ui bridge 自己发，不属于 engine 事件协议）。
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// runtime 启动完成（label = 模式说明，侧边栏展示；随后跟随空会话 Snapshot）。
    Started { label: String, session_id: String },
    /// 会话快照整体发送（折叠态；每次有变化的通知后发一次，且每次整发前已同步落库）。
    Snapshot(Box<SessionSnapshot>),
    /// 正常停止（用户操作：删除/归档 runtime）。
    Stopped { reason: String },
    /// 启动/运行异常（进程退出、spawn/initialize/prompt 失败等；红色提示）。
    Failed { reason: String },
}

/// 折叠桥（原 dshr-ui real.rs 的 RealBridge 迁入，engine 私有）：单会话 Folder
/// 持有者 + wire 通知喂入。
///
/// s3 简化：一次只折叠「一个当前会话」（多 runtime/多会话目录与血缘 = s4，
/// 届时按 session_id 各持一个 Folder 即可，折叠语义不变）。Start/ResetSession
/// 时 reset()（换新 session id），Stop 不清（UI 保留已收消息）。
#[derive(Debug)]
struct Bridge {
    folder: Folder,
}

impl Bridge {
    fn new() -> Self {
        Self {
            folder: Folder::new(),
        }
    }

    /// 清空折叠态（新会话/新 runtime）。
    fn reset(&mut self) {
        self.folder = Folder::new();
    }

    /// 喂一条 SDK 通知（wire 帧 → notifications::parse → Folder::push_notification）。
    /// 只折 `session_id` 对应会话的通知：session.event/session.status 先按
    /// params.sessionId 过滤（子代理血缘会话 s3 不显示，直接跳过）；
    /// subagent.* 等其余通知 Folder 侧本身忽略。
    fn feed(&mut self, session_id: &str, n: &Notification) {
        if n.method == "session.event" || n.method == "session.status" {
            let sid = n
                .params
                .get("sessionId")
                .and_then(serde_json::Value::as_str);
            if sid != Some(session_id) {
                return;
            }
        }
        match notifications::parse(n) {
            Ok(Some(kind)) => self.folder.push_notification(&kind),
            // Ok(None)：未知通知方法（协议演进跳过）；Err：内容畸形（跳过，wire 保真留 recorder）。
            _ => {}
        }
    }

    /// 直接置会话状态（engine 用于 prompt 后乐观 running / 新会话 idle）。
    /// 走与 wire 同一条折叠路径（合成一条 session.status 通知）。
    fn set_status(&mut self, session_id: &str, status: SessionStatus) {
        self.folder
            .push_notification(&Kind::SessionStatus(SessionStatusNotification {
                session_id: session_id.to_string(),
                status,
            }));
    }

    /// 本地补一条用户消息行（fake runtime 不发送 user/message——回显用户自己的
    /// prompt；真实 runtime 会自己发 user/message，不补、防重复）。
    /// 走官方 wire 形状 JSON → SessionEvent → push_event（与 fold.rs 测试同构）。
    fn push_local_user_message(&mut self, text: &str, seq: u64, time: u64) {
        let ev: SessionEvent = serde_json::from_value(serde_json::json!({
            "type": "user/message", "seq": seq, "time": time,
            "data": {
                "id": format!("m-ui-{seq}"),
                "role": "user",
                "content": [{ "type": "text", "text": text }],
                "source": { "kind": "user" },
            },
        }))
        .expect("官方 user/message 形状应可解析（fold.rs 测试同构）");
        self.folder.push_event(&ev);
    }

    /// 当前快照（每次调用重建，轻量）。
    fn snapshot(&self) -> SessionSnapshot {
        self.folder.snapshot()
    }
}

/// engine 状态机：持有折叠桥 + 客户端 + 会话元信息 + 落库 Store（s2 接线）。
pub struct Engine {
    cmd_rx: mpsc::Receiver<EngineCmd>,
    /// 折叠桥（Folder 持有；Start/ResetSession 重置、Stop 保留给 UI 继续看）。
    bridge: Bridge,
    /// SDK 客户端（None = runtime 未启动/已停）。
    client: Option<HarnessClient>,
    /// SDK 事件广播接收端（与 client 同生共死；进程退出时收到 Closed）。
    events: Option<broadcast::Receiver<Notification>>,
    /// 当前会话 id（engine 生成：s-<epoch>。真实 dsh 按 id 落盘日志，必须唯一化，
    /// 见 crate::session 注释；Start/ResetSession 换新）。
    session_id: String,
    /// 当前模式（Fake 需本地回显用户行——fake runtime 不发送 user/message）。
    mode: Option<RuntimeMode>,
    /// 上次整发的快照（无变化不重复发，忽略性事件零 UI 开销）。
    last_sent: Option<SessionSnapshot>,
    /// 本地合成事件 seq（回显行用；engine 本地单调即可）。
    seq: u64,
    /// 测试接缝：强制 Fake（headless 集成测试用；生产恒 false）。
    fake_only: bool,
    /// s2 落库（store.rs）：None = 尚未打开。默认首次 Start 惰性开
    /// default_db_path()（data/ 自动建目录）；测试 with_db 注入内存库。
    store: Option<Store>,
}

impl Engine {
    pub fn new(cmd_rx: mpsc::Receiver<EngineCmd>) -> Self {
        Self {
            cmd_rx,
            bridge: Bridge::new(),
            client: None,
            events: None,
            session_id: String::new(),
            mode: None,
            last_sent: None,
            seq: 0,
            fake_only: false,
            store: None,
        }
    }

    /// 测试接缝：强制 Fake 模式（跳过 resolve_mode 的 config.json 判定——测试机
    /// 若配了真 api-key，不能让它去拉真实 dsh）。仅测试调用。
    pub fn force_fake(&mut self) {
        self.fake_only = true;
    }

    /// 持久化接缝（s2 落库注入）：测试传 `Store::open_in_memory()`。
    /// 生产不调用：首次 Start 时惰性 `Store::open(default_db_path())`（data/ 自动建目录）。
    pub fn with_db(&mut self, db: Store) {
        self.store = Some(db);
    }

    /// 一步驱动：select（命令 ∨ 通知）→ 事件批。None = 命令通道关闭（总线结束）。
    pub async fn next(&mut self) -> Option<Vec<EngineEvent>> {
        type NotifOut = Result<Notification, broadcast::error::RecvError>;
        // 先取两个接收端的借用（字段级拆分借用，select 结束后释放）。
        let cmd_rx = &mut self.cmd_rx;
        let notif: Pin<Box<dyn Future<Output = NotifOut> + Send>> = match &mut self.events {
            Some(rx) => Box::pin(rx.recv()),
            None => Box::pin(std::future::pending()),
        };
        enum Input {
            Cmd(EngineCmd),
            Notif(NotifOut),
            End,
        }
        let input = tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => Input::Cmd(cmd),
                None => Input::End,
            },
            n = notif => Input::Notif(n),
        };
        match input {
            Input::End => None,
            Input::Cmd(cmd) => Some(self.on_cmd(cmd).await),
            Input::Notif(Ok(n)) => Some(self.on_notification(n)),
            // 消费太慢被广播丢事件：跳过继续（官方 lagging 语义）。
            Input::Notif(Err(broadcast::error::RecvError::Lagged(_))) => Some(vec![]),
            // 广播关闭 = runtime 进程退出（读循环 EOF）。
            Input::Notif(Err(broadcast::error::RecvError::Closed)) => {
                Some(self.teardown("runtime 进程已退出".to_string()))
            }
        }
    }

    async fn on_cmd(&mut self, cmd: EngineCmd) -> Vec<EngineEvent> {
        match cmd {
            EngineCmd::Start => self.start().await,
            EngineCmd::Stop => self.stop().await,
            EngineCmd::ResetSession => self.reset_session(),
            EngineCmd::Prompt { text } => self.prompt(text).await,
        }
    }

    /// 惰性开库：首次 Start 落库前打开默认库（data/ 自动建目录；失败只打一行
    /// stderr，本次会话不落库继续跑——落库失败不打断会话，错误面 s4 再看）。
    fn ensure_db(&mut self) {
        if self.store.is_some() {
            return;
        }
        let path = default_db_path();
        match Store::open(&path) {
            Ok(db) => self.store = Some(db),
            Err(e) => eprintln!(
                "[engine] 打开默认库 {} 失败（本次会话不落库）：{e}",
                path.display()
            ),
        }
    }

    /// 启动 runtime（模式判定在 mode::resolve_mode，集中一处）。
    async fn start(&mut self) -> Vec<EngineEvent> {
        if self.client.is_some() {
            return vec![]; // 已在运行：忽略（重启 = 先 Stop 再 Start）。
        }
        let resolved = if self.fake_only {
            ResolvedMode {
                mode: RuntimeMode::Fake,
                note: String::new(),
            }
        } else {
            mode::resolve_mode()
        };
        let ws = mode::workspace_root();
        let mut kit = match resolved.mode {
            RuntimeMode::Fake => mode::kit(RuntimeMode::Fake, &ws).expect("fake 装配不应失败"),
            RuntimeMode::Real => {
                // ensure 可能 pnpm install（网络/分钟级）→ spawn_blocking 免得卡 UI 执行器。
                let ws = ws.clone();
                match tokio::task::spawn_blocking(move || mode::kit(RuntimeMode::Real, &ws)).await {
                    Ok(Ok(kit)) => kit,
                    Ok(Err(e)) => return vec![EngineEvent::Failed { reason: e }],
                    Err(e) => {
                        return vec![EngineEvent::Failed {
                            reason: format!("装配任务失败：{e}"),
                        }];
                    }
                }
            }
        };
        // 回落说明挂到 label（状态栏可见："Fake runtime（无 config.json）"）。
        if !resolved.note.is_empty() {
            kit.label = format!("{}（{}）", kit.label, resolved.note);
        }
        self.spawn_client(kit).await
    }

    /// spawn + initialize → 就绪（新会话：新 id + Folder 重置 + idle 合成状态）。
    /// 接线点（WireLog）：spawn 前把全程记录路径填进 config.wire_log_path。
    async fn spawn_client(&mut self, mut kit: SpawnKit) -> Vec<EngineEvent> {
        let label = kit.label.clone();
        // 落库接缝：首次 Start 惰性开默认库（data/ 自动建目录；测试已用 with_db 注入）。
        self.ensure_db();
        // WireLog 接线：SDK 的 wire_log_path = 全程线级记录（transport 双向写
        // cat=dsh JSONL，见模块头口径）——建 data/wire-logs/ 并给本 run 一条
        // `<label 化文件名>-<epoch>.jsonl`；失败只打一行 stderr（本次不记录，会话继续）。
        if kit.config.wire_log_path.is_none() {
            let dir = workspace_root().join("data").join("wire-logs");
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {
                    let file = format!("{}-{}.jsonl", slugify(&kit.label), epoch_ms());
                    kit.config.wire_log_path = Some(dir.join(file).to_string_lossy().into_owned());
                }
                Err(e) => {
                    eprintln!("[engine] 建 wire-logs 目录失败（本次运行不记录线级日志）：{e}")
                }
            }
        }
        let mut client = match HarnessClient::spawn(kit.config).await {
            Ok(c) => c,
            Err(e) => {
                return vec![EngineEvent::Failed {
                    reason: format!("runtime 启动失败：{e}"),
                }];
            }
        };
        if let Err(e) = client.initialize(&kit.init).await {
            // 进程活着但握手失败（真实 dsh 鉴权/参数错）→ 优雅收尾再报。
            let _ = client.shutdown().await;
            return vec![EngineEvent::Failed {
                reason: format!("initialize 失败：{e}"),
            }];
        }
        let events_rx = client.take_events(); // 广播接收端归 Engine（读循环在 transport）。
        self.session_id = gen_session_id();
        self.mode = Some(kit.mode);
        self.bridge.reset();
        self.seq = 0;
        self.last_sent = None;
        self.client = Some(client);
        self.events = Some(events_rx);
        // 新会话初始 idle（合成通知走折叠 → 空快照带 session id，UI 清场）。
        self.bridge
            .set_status(&self.session_id, SessionStatus::Idle);
        let mut out = vec![EngineEvent::Started {
            label,
            session_id: self.session_id.clone(),
        }];
        if let Some(ev) = self.emit_snapshot() {
            out.push(ev);
        }
        out
    }

    /// 停止：收尾落库 → 协议 shutdown + dispose 阶梯（失败忽略：进程已死则当收尸）。
    /// 折叠态保留 → UI 消息流不丢（下次 Start 才重置）。
    async fn stop(&mut self) -> Vec<EngineEvent> {
        let Some(client) = self.client.take() else {
            return vec![];
        };
        // 收尾落库：当前折叠快照补一次 persist（即使与上次整发相同——把会话最终态
        // 落盘；store 替换语义保证幂等）。
        self.flush_persist();
        let _ = client.shutdown().await;
        self.events = None;
        vec![EngineEvent::Stopped {
            reason: String::new(),
        }]
    }

    /// 会话级异常拆除（进程退出/请求失败）：收尾落库 → 客户端置空，保留折叠态供查看。
    fn teardown(&mut self, reason: String) -> Vec<EngineEvent> {
        self.flush_persist();
        self.client = None;
        self.events = None;
        vec![EngineEvent::Failed { reason }]
    }

    /// 重置当前会话（「新建会话/删除会话」语义：Folder 清空 + 新 session id；
    /// 未来多会话管理 = 旧 Folder 归档/落库，这里只清当前）。
    fn reset_session(&mut self) -> Vec<EngineEvent> {
        if self.client.is_none() {
            return vec![]; // 未启动：忽略。
        }
        self.bridge.reset();
        self.seq = 0;
        self.last_sent = None;
        self.session_id = gen_session_id();
        self.bridge
            .set_status(&self.session_id, SessionStatus::Idle);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 发一条用户消息：session/prompt 入队回执即刻返回，agent 产出走通知流。
    async fn prompt(&mut self, text: String) -> Vec<EngineEvent> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return vec![];
        }
        if self.client.is_none() {
            return vec![]; // runtime 未启动（App 侧已挡）。
        }
        let session_id = self.session_id.clone();
        let is_fake = self.mode == Some(RuntimeMode::Fake);
        let result = self
            .client
            .as_mut()
            .expect("已判 Some")
            .prompt(&SessionPromptParams {
                session_id: session_id.clone(),
                content_blocks: vec![SdkPromptContentBlock::text(text.clone())],
            })
            .await;
        if let Err(e) = result {
            // 进程侧已挂/协议错 → 拆除并报错（快照保留最后状态）。
            return self.teardown(format!("prompt 发送失败：{e}"));
        }
        // Fake 不回发 user/message → 本地回显用户行（真实 runtime 会自己发，不补防重复）。
        if is_fake {
            self.seq += 1;
            self.bridge
                .push_local_user_message(&text, self.seq, epoch_ms());
        }
        // 乐观 running：真实 dsh 的 session.status 未到前先置 running，idle 到达即回落。
        self.bridge.set_status(&session_id, SessionStatus::Running);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 通知 → 折叠（Bridge::feed 按当前会话过滤）→ 变化才整发快照。
    fn on_notification(&mut self, n: Notification) -> Vec<EngineEvent> {
        if self.client.is_none() {
            return vec![]; // 理论不发生（events 只在运行时存在）。
        }
        self.bridge.feed(&self.session_id, &n);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 折叠快照整发（与上次相同则跳过；Snapshot 事件带 Box 走 UI 消息）。
    /// 落库接线点：判定「有变化并即将整发」后先同步落库再发——rusqlite 同步小写，
    /// 失败 eprintln 一行并继续（落库不打断会话；错误面 s4 再看）。同一会话重复
    /// 事件幂等（store 替换语义，见 store.rs persist_snapshot 注释）。
    fn emit_snapshot(&mut self) -> Option<EngineEvent> {
        let snap = self.bridge.snapshot();
        if self.last_sent.as_ref() == Some(&snap) {
            return None;
        }
        self.persist(&snap);
        self.last_sent = Some(snap.clone());
        Some(EngineEvent::Snapshot(Box::new(snap)))
    }

    /// 落库一行（同步；失败只打一行 stderr，不打断会话）。
    fn persist(&mut self, snap: &SessionSnapshot) {
        if let Some(db) = &mut self.store {
            if let Err(e) = db.persist_snapshot(snap) {
                eprintln!("[engine] 落库失败（忽略继续）：{e}");
            }
        }
    }

    /// 收尾落库：Stop/拆除时把当前折叠快照补一次 persist（未启动无会话则跳过）。
    fn flush_persist(&mut self) {
        if self.session_id.is_empty() {
            return;
        }
        let snap = self.bridge.snapshot();
        self.persist(&snap);
    }
}

/// 会话 id 唯一化（epoch ms；真实 dsh 按 id 持久化会话日志，固定 id 会撞冲突）。
fn gen_session_id() -> String {
    format!("s-{}", epoch_ms())
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// label → 文件名安全片段（wire-log 文件名用；非 [A-Za-z0-9._-] 一律 '-'）。
/// 中文/空格/「·（）」等说明文字会变成一连串 '-'——可读性够用即可。
fn slugify(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
