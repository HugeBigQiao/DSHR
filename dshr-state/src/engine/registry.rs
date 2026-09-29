//! engine 的注册表与事件循环：多 runtime 的持有、命令路由、事件聚合。
//!
//! 主要用途：[`Engine`] 是所有 runtime 的唯一持有者——它把每个 runtime 的通知流聚合到
//! 一个循环里，按 `sessionId` 路由到对应会话态，再按需要落库并把快照发给 UI。
//! 为什么需要：多 runtime 是**产品能力而非协议能力**（官方 wire 协议里没有 runtime 概念，
//! 只有一条连接）。持有注册表、做命令路由与失败隔离的职责只能落在某一层，
//! 本模块就是那一层。
//!
//! **多路复用怎么做的**：每个 runtime 一个 `events` 接收端，用
//! `futures::stream::select_all` 把它们合成一条流——于是事件循环只有**两个**分支
//!（命令 ∨ 任一 runtime 的事实），不需要为每个 runtime 起转发任务。
//! 为什么不用「每 runtime 一个 task + 汇总通道」：那会引入 N 个任务的生命周期
//!（谁在什么时候 abort？进程退出后任务怎么收敛？），而 `select_all` 让所有 runtime
//! 的等待都留在同一个循环里，生命周期清晰。
//!
//! 上接：`crate::engine`（类型定义处）、`dshr-ui/src/bridge.rs`（驱动本循环）。
//! 下接：`crate::raw`（`Runtime` 与 `RuntimeEvent`）、`crate::store`（落库）、
//!       `SessionState`（每会话折叠态，见 `super::session`）。
//! 官方对应：无（见 `engine.rs` 文件头）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::mpsc;

use dsh_sdk_protocol::rpc::Notification;

use super::session::SessionState;
use super::{EngineCmd, EngineEvent, SessionId};
use crate::raw::{Runtime, RuntimeEvent};
use crate::store::{Store, default_db_path};

/// runtime 标识（engine 生成：`rt-<epoch>`）。
///
/// 为什么用 newtype：`EngineCmd` 里同时有 runtime id 与 session id，
/// 裸 `String` 时极易写反；类型区分让错误在编译期暴露。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeId(pub String);

impl RuntimeId {
    /// 包一层（测试与调用方传入已有 id）。
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// 生成一个新的 runtime id（epoch 毫秒，与 session id 同源便于日志对齐）。
    pub fn generate() -> Self {
        Self(format!("rt-{}", crate::raw::epoch_ms()))
    }

    /// 借用内部字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 一个 runtime 在 engine 侧的全部状态。
///
/// 注：不 derive `Debug`——`Runtime` 内含 `Box<dyn SessionDriver>`（trait 对象不支持 Debug），
/// 而本结构只在 engine 内部使用，调试时按需打印单个字段即可。
pub struct RuntimeSlot {
    /// raw 层句柄（进程 + 协议）。
    pub runtime: Runtime,
    /// 该 runtime 下的会话表（当前实现：同时只有一个活跃会话，
    /// 但结构已是「多会话」——加会话只需插表，不改路由代码）。
    pub sessions: HashMap<SessionId, SessionState>,
    /// 展示 label（未启动时为空）。
    pub label: String,
}

impl RuntimeSlot {
    /// 新建槽位（runtime 未启动、无会话）。
    ///
    /// # 参数
    /// - `id`：runtime 标识——句柄一建出来就带着它（stderr/退出落库都要用）。
    pub fn new(id: RuntimeId) -> Self {
        Self {
            runtime: Runtime::new(id),
            sessions: HashMap::new(),
            label: String::new(),
        }
    }
}

/// 核心数据处理中台。
///
/// 为什么需要：见模块头——它是「多 runtime + 多会话 + 落库」的唯一持有者。
/// 生命周期：由 UI 总线（`dshr-ui/src/bridge.rs`）创建并常驻驱动；
/// 命令通道关闭（UI 退出）时 `next` 返回 `None`，循环结束。
pub struct Engine {
    /// UI → engine 命令通道。
    cmd_rx: mpsc::Receiver<EngineCmd>,
    /// 多 runtime 注册表。
    runtimes: HashMap<RuntimeId, RuntimeSlot>,
    /// 加工库（`None` = 尚未打开；首次启动 runtime 时惰性打开）。
    store: Option<Store>,
    /// 全程记录器（`None` = 打不开文件或未启用；见 `crate::record`）。
    ///
    /// 为什么 engine 持有它：它同时要写 app 轨迹、并把**同一个路径**交给 SDK 写线级记录。
    /// 两者必须同源（否则一次运行会散成两个文件，时间线无法对齐）。
    recorder: Option<crate::record::Recorder>,
    /// 测试接缝：强制 Fake 模式（跳过 `config.json` 判定）。
    fake_only: bool,
}

impl Engine {
    /// 新建（尚未打开库；首次 `StartRuntime` 时惰性打开）。
    pub fn new(cmd_rx: mpsc::Receiver<EngineCmd>) -> Self {
        Self {
            cmd_rx,
            runtimes: HashMap::new(),
            store: None,
            recorder: None,
            fake_only: false,
        }
    }

    /// 打开本次运行的线级记录文件，并把路径填进 spawn 配置。
    ///
    /// # 返回
    /// `Some(path)` = 已开记录（也是交给 SDK 的 `wire_log_path`）；`None` = 未启用/打不开。
    ///
    /// 为什么失败不致命：记录是诊断设施，打不开就退化为「本次不记录」，
    /// 不该拦住用户对话（只打一行 stderr）。
    /// 路径规则：`<workspace>/data/wire-logs/<epoch>-<pid>.jsonl`——
    /// 用 epoch 保证多次运行不互相覆盖；不带 label 是因为 label 里有中文/空格，
    /// 而路径的**可读性**不如「不冲突」重要（同一个文件里 app 轨迹会写明 label）。
    fn open_wire_log(&mut self) -> Option<String> {
        if let Some(rec) = self.recorder.as_ref() {
            return Some(rec.wire_log_path());
        }
        let dir = crate::raw::workspace_root().join("data").join("wire-logs");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("[engine] 建 wire-logs 目录失败（本次不记录线级日志）：{e}");
            return None;
        }
        let file = format!("{}-{}.jsonl", now_epoch_ms(), std::process::id());
        match crate::record::Recorder::open(dir.join(file)) {
            Ok(rec) => {
                let path = rec.wire_log_path();
                rec.app("recorder.opened", &serde_json::json!({ "path": path }));
                self.recorder = Some(rec);
                Some(path)
            }
            Err(e) => {
                eprintln!("[engine] 打开线级记录失败（本次不记录）：{e}");
                None
            }
        }
    }

    /// 测试接缝：强制 Fake 模式（测试机若配了真 api-key，不能让它去拉真实 dsh）。
    pub fn force_fake(&mut self) {
        self.fake_only = true;
    }

    /// 持久化接缝：注入内存库（生产路径由 `ensure_db` 惰性打开 `data/dshr.db`）。
    pub fn with_db(&mut self, db: Store) {
        self.store = Some(db);
    }

    /// 库的只读引用（测试断言落库结果用；生产不读——读侧走 B 方案的快照接口）。
    pub fn store_ref(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    /// 测试接缝：把某个 runtime 标成「进程已退出」。
    ///
    /// 为什么需要经 engine 提供：`Engine` 是 runtime 的唯一持有者，测试拿不到内部句柄。
    /// 置位后走**正常路径**——该 runtime 的 `next_event` 观察到流关闭 → 产出
    /// `RuntimeEvent::Exited` → `on_runtime_event` 走 teardown。
    pub fn mark_runtime_exited_for_test(&mut self, id: &RuntimeId) {
        if let Some(slot) = self.runtimes.get_mut(id) {
            slot.runtime.mark_exited_for_test();
        }
    }

    /// 线级记录器的只读引用（测试断言 app 轨迹用）。
    pub fn recorder_ref(&self) -> Option<&crate::record::Recorder> {
        self.recorder.as_ref()
    }

    /// 测试接缝：挂一个线级记录器（写到给定路径），断言 app 轨迹用。
    ///
    /// 为什么需要：生产路径只在 `StartRuntime` 里开记录器（那会 spawn 进程），
    /// 而注入式测试要验证的是**记录内容**（如 `event.degraded` 的协议漂移回执）。
    pub fn with_recorder_for_test(&mut self, path: std::path::PathBuf) -> std::io::Result<()> {
        self.recorder = Some(crate::record::Recorder::open(path)?);
        Ok(())
    }

    /// 测试接缝：注册一个「已就绪」的 runtime（注入 driver + 通知流 + 初始会话），
    /// **不 spawn 任何进程**。
    ///
    /// 为什么需要：engine 的核心逻辑是「按会话路由 + 脏检测 + 落库」，这些都必须能不起
    /// node 子进程地验证（`SessionDriver` 抽象的价值所在）。生产路径只能经
    /// `EngineCmd::StartRuntime` → `Runtime::start` 造出真实 driver。
    pub fn register_injected_runtime(
        &mut self,
        id: RuntimeId,
        driver: Box<dyn crate::raw::SessionDriver>,
        session: SessionId,
        events: tokio::sync::broadcast::Receiver<Notification>,
    ) {
        self.register_injected_runtime_with_stderr(id, driver, session, events, None)
    }

    /// 同上，但可额外注入 stderr 接收端（用于验证 stderr 落库链路）。
    pub fn register_injected_runtime_with_stderr(
        &mut self,
        id: RuntimeId,
        driver: Box<dyn crate::raw::SessionDriver>,
        session: SessionId,
        events: tokio::sync::broadcast::Receiver<Notification>,
        stderr: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    ) {
        let mut slot = RuntimeSlot::new(id.clone());
        slot.runtime.with_driver_for_test(
            driver,
            session.0.clone(),
            events,
            crate::raw::mode::RuntimeMode::Fake,
        );
        if let Some(rx) = stderr {
            slot.runtime.inject_stderr_for_test(rx);
        }
        slot.label = "test".to_string();
        // `runtime_logs.runtime_id` 有外键约束指向 `runtimes(id)`——生产路径由
        // `start_runtime` 写这一行，注入路径必须自己补，否则 stderr 落库会
        // FOREIGN KEY 失败（这正是本接缝跑测试时暴露出来的）。
        if let Some(db) = self.store.as_ref() {
            let _ = db.upsert_runtime(id.as_str(), "test", "running");
        }
        let mut state = SessionState::new(session.clone());
        state.set_status(super::SessionStatus::Idle);
        slot.sessions.insert(session, state);
        self.runtimes.insert(id, slot);
    }

    /// 一步驱动事件循环：等「一条命令」或「任一 runtime 的一件事实」，处理后返回事件批。
    ///
    /// # 返回
    /// - `Some(events)`：本次处理产出的事件（可能为空批）；
    /// - `None`：命令通道关闭（UI 退出）→ 调用方结束循环。
    ///
    /// # 实现要点（为什么这样写）
    /// 1. **聚合而非每 runtime 一个任务**：把每个 runtime 的 `next_event` 放进
    ///    `futures::future::join_all` 一起等，谁先就绪就用谁——于是循环只有两个分支。
    ///    另一条路是「每 runtime 起一个转发任务 + 汇总通道」，但那会引入 N 个任务的生命周期
    ///    （进程退出后任务何时收敛？），而这里所有等待都留在同一个循环里。
    /// 2. **借用必须跨 await 分开**：`runtime_events` 里的 future 各自借走一个
    ///    `&mut slot.runtime`，也就是借走了 `self`。所以「等结果」与「用 `&mut self` 处理结果」
    ///    必须分两段：先在 `select!` 里拿到所有权数据（`id` 已克隆、`RuntimeEvent` 已 move），
    ///    再把 future 全部丢弃（`join_all` 消费掉它们），最后才调 `self.on_*`。
    ///    这也解释了为什么不能用 `FuturesUnordered` 跨迭代持有——那样借用会活过整轮循环。
    pub async fn next(&mut self) -> Option<Vec<EngineEvent>> {
        // **先把已排队的命令排空**（`try_recv`，不阻塞）。
        //
        // 为什么必须先做这一步：`select!` 在多个分支同时就绪时是**随机**选择的。
        // 只要命令通道里有排队项（UI 点几下就会有几个），runtime 事件分支就会有一半概率
        // 被跳过；反过来若命令持续到达，事件会被饿死（写测试时实测到：注入 runtime 后
        // 通知永远不被处理）。先排空命令让「命令优先」成为一个确定性的顺序。
        match self.cmd_rx.try_recv() {
            Ok(cmd) => return Some(self.on_cmd(cmd).await),
            Err(mpsc::error::TryRecvError::Disconnected) => return None, // UI 退出。
            Err(mpsc::error::TryRecvError::Empty) => {}                  // 没有排队命令：进入等待。
        }

        // **一个 runtime 都没有：只等命令通道。**
        //
        // 为什么必须单独处理（这不是优化，是正确性）：`FuturesUnordered::next()` 在**空集合**上
        // 立刻返回 `None`，于是下面那个 `select!` 会立即完成并返回一个空批。而调用方
        //（`dshr-ui/src/bridge.rs` 的总线）是 `loop { engine.next().await }`——它会因此空转，
        // 在 iced 的执行器线程上烧掉一个核，**界面上却没有任何异常**。
        // 这正是本项目里最贵的那类 bug（静默失败）的形态：不报错、不崩溃，只是「什么都没发生」。
        // 触发条件很常见：程序刚启动（还没 StartRuntime）、以及用户停掉最后一个 runtime。
        if self.runtimes.is_empty() {
            let cmd = self.cmd_rx.recv().await;
            return match cmd {
                Some(cmd) => Some(self.on_cmd(cmd).await),
                None => None, // 命令通道关闭 = UI 退出。
            };
        }

        // **把 runtime 表临时搬出 `self`**：下面的 future 各自借走一个 `&mut RuntimeSlot`，
        // 即借走 `self.runtimes`；若它仍留在 `self` 里，`select!` 的另一分支
        //（`&mut self.cmd_rx`）以及之后的 `self.on_*` 都会被判为对 `*self` 的二次可变借用。
        // 搬出后 future 借的是本地变量，与本结构无关；本轮结束原样放回。
        let mut runtimes = std::mem::take(&mut self.runtimes);
        let mut runtime_events: FuturesUnordered<
            Pin<Box<dyn Future<Output = (RuntimeId, Option<RuntimeEvent>)> + Send + '_>>,
        > = FuturesUnordered::new();
        for (id, slot) in runtimes.iter_mut() {
            let id = id.clone();
            runtime_events.push(Box::pin(async move {
                let ev = slot.runtime.next_event().await;
                (id, ev)
            }));
        }

        // select! 只负责把数据取出来（id 已克隆、事件已 move），**不触碰 self**。
        enum Input {
            Cmd(Option<EngineCmd>),
            Runtime(RuntimeId, Option<RuntimeEvent>),
            /// 没有任何在等的 runtime（一个都没启动）：本轮无事。
            Idle,
        }
        let input = tokio::select! {
            cmd = self.cmd_rx.recv() => Input::Cmd(cmd),
            ev = runtime_events.next() => match ev {
                Some((id, ev)) => Input::Runtime(id, ev),
                None => Input::Idle,
            },
        };
        drop(runtime_events); // 借用在此结束。
        self.runtimes = runtimes; // 原样放回（本轮不增删 runtime）。

        match input {
            Input::Cmd(None) => None, // 命令通道关闭 = UI 退出。
            Input::Cmd(Some(cmd)) => Some(self.on_cmd(cmd).await),
            Input::Runtime(id, Some(ev)) => Some(self.on_runtime_event(&id, ev)),
            // 句柄未启动：该 runtime 没有可等的事实，忽略本轮。
            Input::Runtime(_, None) | Input::Idle => Some(Vec::new()),
        }
    }

    /// 惰性开库：首次启动 runtime 时打开默认库。
    ///
    /// 为什么失败只打一行 stderr：落库失败不该拦住会话（用户要的是能对话），
    /// 错误面留给监控页（`DESIGN.md` §12.1 M3.8）。
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

    /// 记一条协议请求事实（落 `requests` 表；库未开或写失败都只打一行 stderr，不阻断会话）。
    ///
    /// 为什么吞掉错误：落库是**观测**，不该让一次统计写入失败影响用户正在做的事
    /// （与 `take_changed` 里「落库失败不打断会话」同一条原则）。
    fn record_request(
        &self,
        id: &RuntimeId,
        session: &SessionId,
        method: &str,
        time: u64,
        duration_ms: u64,
        success: bool,
        error: Option<&str>,
    ) {
        let Some(db) = self.store.as_ref() else {
            return;
        };
        if let Err(e) = db.append_request(
            session.as_str(),
            id.as_str(),
            None,
            method,
            time,
            duration_ms,
            success,
            error,
        ) {
            eprintln!("[engine] 记录请求事实失败（忽略）：{e}");
        }
    }

    /// 命令分发。
    async fn on_cmd(&mut self, cmd: EngineCmd) -> Vec<EngineEvent> {
        match cmd {
            EngineCmd::StartRuntime { id } => self.start_runtime(id).await,
            EngineCmd::StopRuntime { id } => self.stop_runtime(id).await,
            EngineCmd::ResetSession { id } => self.reset_session(id),
            EngineCmd::Prompt { id, session, text } => self.prompt(id, session, text).await,
            EngineCmd::ReadSnapshot { session } => self.read_snapshot(&session),
            EngineCmd::ListSessions => self.list_sessions(),
        }
    }

    /// **按需拉取**一份快照：运行中的会话优先，否则从库里复原（M6 的 B 方案入口）。
    ///
    /// 三个边界（都写在实现里，避免后来者误改）：
    /// - **只读**：不脏检测、不落库、不改状态——「拉」不该有副作用（落库由变更路径负责）；
    ///   所以这里用 `SessionState::snapshot()` 而不是 `take_changed()`。
    /// - **不缓存历史态**：从库读几百行是毫秒级，缓存反而引入失效问题（会话可能又被跑起来）；
    ///   真成瓶颈时再加 LRU，那时也才知道该按什么淘汰。
    /// - **找不到就静默**：不造空快照（那会让 UI 以为会话存在且是空的），也不报错
    ///   （目录由 `ListSessions` 提供，UI 选到的 id 必然来自目录或事件）。
    fn read_snapshot(&mut self, session: &SessionId) -> Vec<EngineEvent> {
        if session.as_str().is_empty() {
            return Vec::new();
        }
        // ① 运行中：在注册表里按会话 id 找（一个 runtime 内可并存多会话）。
        for (id, slot) in &self.runtimes {
            if let Some(state) = slot.sessions.get(session) {
                return vec![EngineEvent::SessionLoaded {
                    session: session.clone(),
                    runtime: Some(id.clone()),
                    snapshot: Box::new(state.snapshot()),
                }];
            }
        }
        // ② 库里复原：会话早已结束（或本次应用刚启动）。
        let Some(db) = self.store.as_ref() else {
            return Vec::new();
        };
        match db.load_snapshot(session.as_str()) {
            Ok(Some(snapshot)) => vec![EngineEvent::SessionLoaded {
                session: session.clone(),
                runtime: None,
                snapshot: Box::new(snapshot),
            }],
            Ok(None) => Vec::new(),
            Err(e) => {
                // 读库失败不是会话错误：报一行 stderr 并当「没有」处理（UI 侧看到的是没变化）。
                eprintln!("[engine] 拉取快照失败（忽略）：{e}");
                Vec::new()
            }
        }
    }

    /// 列出库里的会话目录（§8.3 聚合：轮/token/工具/错误/标题）。
    fn list_sessions(&mut self) -> Vec<EngineEvent> {
        let Some(db) = self.store.as_ref() else {
            return Vec::new();
        };
        match db.session_summaries() {
            Ok(rows) => vec![EngineEvent::Sessions { rows }],
            Err(e) => {
                eprintln!("[engine] 列出会话目录失败（忽略）：{e}");
                Vec::new()
            }
        }
    }

    /// 启动一个 runtime：建槽位 → 拉起进程 → 建初始会话 → 发 `Started` + 空快照。
    async fn start_runtime(&mut self, id: RuntimeId) -> Vec<EngineEvent> {
        self.ensure_db();
        if self.runtimes.contains_key(&id) {
            return Vec::new(); // 已存在：忽略（重启 = 先 Stop 再 Start）。
        }
        let mut slot = RuntimeSlot::new(id.clone());
        if self.fake_only {
            slot.runtime.force_fake();
        }
        // 线级记录路径由这里决定（而不是 raw 自己拼）：engine 还要把**同一个路径**
        // 交给 `Recorder` 写 app 轨迹，两边必须同源，否则 app 记录会写到另一个文件。
        let wire_log_path = self.open_wire_log();
        match slot.runtime.start(wire_log_path).await {
            Ok(label) => {
                slot.label = label.clone();
                // 记录 runtime 实例（`runtimes` 表此前无写入方 → 会话行 runtime_id 恒 NULL）。
                if let Some(db) = self.store.as_ref() {
                    if let Err(e) = db.upsert_runtime(id.as_str(), &label, "running") {
                        eprintln!("[engine] 记录 runtime 失败（忽略）：{e}");
                    }
                }
                if let Some(rec) = self.recorder.as_ref() {
                    rec.app(
                        "runtime.started",
                        &serde_json::json!({ "id": id.as_str(), "label": label }),
                    );
                }
                let session = SessionId::new(slot.runtime.session_id().to_string());
                let mut state = SessionState::new(session.clone());
                state.set_status(super::SessionStatus::Idle);
                slot.sessions.insert(session.clone(), state);
                self.runtimes.insert(id.clone(), slot);

                let mut out = vec![EngineEvent::Started {
                    id: id.clone(),
                    label,
                    session: session.clone(),
                }];
                if let Some(snap) = self.emit_changed(&id, &session) {
                    out.push(snap);
                }
                out
            }
            Err(e) => {
                // 启动失败：不留槽位（UI 的失败提示由本事件承载）。
                vec![EngineEvent::Failed { id, reason: e.0 }]
            }
        }
    }

    /// 停止并移除一个 runtime：收尾落库 → 协议 shutdown → 移除槽位。
    async fn stop_runtime(&mut self, id: RuntimeId) -> Vec<EngineEvent> {
        let Some(mut slot) = self.runtimes.remove(&id) else {
            return Vec::new();
        };
        // 收尾落库：把每个会话的当前快照补一次 persist（最终态）。
        // 先借出 store，避免与 slot 的可变借用被判为对 *self 的二次可变借用。
        let store = self.store.as_ref();
        for state in slot.sessions.values_mut() {
            state.flush(store);
        }
        let reason = match slot.runtime.stop().await {
            Ok(()) => String::new(),
            Err(e) => format!("收尸异常：{e}"),
        };
        vec![EngineEvent::Stopped { id, reason }]
    }

    /// 在某个 runtime 内新建会话（换新 id + 清空折叠态 + 发 `SessionReset` + 空快照）。
    ///
    /// 为什么新 id 在这里生成：唯一性规则（`s-<epoch>`）与「真实 dsh 按 id 落盘」这条
    /// 约束都属于 state 层；调用方只说「换一个」，结果经 `SessionReset` 回报。
    fn reset_session(&mut self, id: RuntimeId) -> Vec<EngineEvent> {
        let Some(slot) = self.runtimes.get_mut(&id) else {
            return Vec::new(); // 未启动的 runtime：忽略。
        };
        let new_id = SessionId::new(slot.runtime.reset_session());
        let mut state = SessionState::new(new_id.clone());
        state.set_status(super::SessionStatus::Idle);
        slot.sessions.clear(); // 当前实现：同一时刻只有一个活跃会话（旧会话的折叠态丢弃）。
        slot.sessions.insert(new_id.clone(), state);
        let mut out = vec![EngineEvent::SessionReset {
            id: id.clone(),
            session: new_id.clone(),
        }];
        if let Some(ev) = self.emit_changed(&id, &new_id) {
            out.push(ev);
        }
        out
    }

    /// 向某个会话发一条用户消息。
    async fn prompt(
        &mut self,
        id: RuntimeId,
        session: SessionId,
        text: String,
    ) -> Vec<EngineEvent> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Vec::new();
        }
        // 先取该会话是否 Fake（决定要不要本地回显用户行），再借 runtime 发请求。
        let is_fake = {
            let Some(slot) = self.runtimes.get(&id) else {
                return Vec::new();
            };
            slot.runtime.is_fake()
        };
        // 计时包住真正的发送：`requests` 表要的是「宿主发一次请求花了多久、成不成」
        //（wire 事件里没有这个信息——失败时连响应都没有）。
        let started = now_epoch_ms();
        let send = {
            let Some(slot) = self.runtimes.get_mut(&id) else {
                return Vec::new();
            };
            slot.runtime.prompt(&text).await
        };
        let elapsed = now_epoch_ms().saturating_sub(started);
        if let Err(e) = send {
            // ① 请求事实：失败也留痕（含原因），这是监控页「成功率/失败原因」的数据源。
            self.record_request(
                &id,
                &session,
                "session/prompt",
                started,
                elapsed,
                false,
                Some(&e.to_string()),
            );
            // ② 会话里留一条**可见**记录，并立刻发快照（先落库再发，见 take_changed 注释）——
            // 这样「发送失败：<原因>」既在 UI 上看得见，也在库里查得到（用户明确要求）。
            let mut out = Vec::new();
            if let Some(state) = self.session_mut(&id, &session) {
                state.push_local_notice(format!("发送失败：{e}"), Some(e.to_string()));
            }
            if let Some(ev) = self.emit_changed(&id, &session) {
                out.push(ev);
            }
            out.extend(self.teardown(&id, format!("prompt 发送失败：{e}")));
            return out;
        }
        // 成功也记一行：耗时分布是监控页最有用的指标之一。
        self.record_request(
            &id,
            &session,
            "session/prompt",
            started,
            elapsed,
            true,
            None,
        );
        // Fake runtime 不回发 user/message → 本地补一行，否则 UI 看不到用户说过的话。
        if is_fake {
            if let Some(state) = self.session_mut(&id, &session) {
                state.push_local_user_message(&text);
            }
        }
        // 乐观 running：真实 dsh 的 session.status 未到之前先置 running，idle 到达即回落。
        if let Some(state) = self.session_mut(&id, &session) {
            state.set_status(super::SessionStatus::Running);
        }
        match self.emit_changed(&id, &session) {
            Some(ev) => vec![ev],
            None => Vec::new(),
        }
    }

    /// 处理一件 runtime 事实。
    fn on_runtime_event(&mut self, id: &RuntimeId, ev: RuntimeEvent) -> Vec<EngineEvent> {
        match ev {
            RuntimeEvent::Notification(n) => self.route_notification(id, n),
            // 消费太慢被广播丢事件：不算错误（官方 lagging 语义），但值得留痕。
            RuntimeEvent::Lagged(n) => {
                eprintln!("[engine] runtime {id} 事件丢失 {n} 条（消费太慢）");
                Vec::new()
            }
            RuntimeEvent::Exited {
                exit_code,
                stderr_tail,
            } => {
                let reason = match exit_code {
                    Some(code) => format!("runtime 进程已退出（exit code {code}）"),
                    None => "runtime 进程已退出".to_string(),
                };
                let reason = if stderr_tail.is_empty() {
                    reason
                } else {
                    format!("{reason}；stderr 尾部：{}", stderr_tail.join(" | "))
                };
                self.teardown(id, reason)
            }
            // `Ready` 只在 `start()` 内部消费（见 `start_runtime`），不会走到这里。
            RuntimeEvent::Ready { .. } => Vec::new(),
            // stderr 行：落 `runtime_logs` 表 + app 轨迹。
            //
            // 为什么不进会话的折叠管道：stderr 是**进程级**事实，不属于任何会话；
            // 折叠它会让 UI 消息流里混进运行时噪音（崩溃时除外——那时用户要看现场）。
            RuntimeEvent::Stderr(line) => {
                if let Some(db) = self.store.as_ref() {
                    if let Err(e) = db.append_runtime_log(id.as_str(), "stderr", &line) {
                        eprintln!("[engine] 写 runtime 日志失败（忽略）：{e}");
                    }
                }
                if let Some(rec) = self.recorder.as_ref() {
                    rec.app(
                        "runtime.stderr",
                        &serde_json::json!({ "id": id.as_str(), "line": line }),
                    );
                }
                Vec::new()
            }
            // stderr 流结束：无事可做（退出本身由 `Exited` 承载）。
            RuntimeEvent::StderrEnded => Vec::new(),
        }
    }

    /// 把一条通知路由到拥有它的会话。
    ///
    /// # 为什么要路由而不是「谁当前活跃就给谁」
    /// 一个 runtime 内的多个会话都可能发通知（含子代理血缘会话）。按 `sessionId`
    /// 精确归属是唯一不会串台的做法；找不到归属的通知被丢弃（血缘树是后续里程碑）。
    fn route_notification(&mut self, id: &RuntimeId, n: Notification) -> Vec<EngineEvent> {
        let Some(slot) = self.runtimes.get_mut(id) else {
            return Vec::new();
        };
        // 找出拥有此通知的会话 id（先判定归属，再折叠——避免对每个会话重复解析）。
        let owner = slot
            .sessions
            .values()
            .find(|s| s.owns(&n))
            .map(|s| s.id().clone());
        let Some(session) = owner else {
            return Vec::new(); // 不属于任何已知会话（子代理血缘等）——当前丢弃。
        };
        // 降级回执：本条通知里有「已知类型但 data 解析失败」的事件 → 写一条 app 轨迹。
        // 为什么在这里写（而不是 fold 里）：record 归 engine 持有（同源写同一文件），
        // 且这条记录是**宿主运维事实**，不属于投影语义。
        let degraded = slot
            .sessions
            .get_mut(&session)
            .and_then(|state| state.feed(&n));
        if let Some(d) = degraded {
            if let Some(rec) = self.recorder.as_ref() {
                rec.app(
                    "event.degraded",
                    &serde_json::json!({
                        "sessionId": session.as_str(),
                        "eventType": d.event_type,
                        "seq": d.seq,
                    }),
                );
            }
        }
        match self.emit_changed(id, &session) {
            Some(ev) => vec![ev],
            None => Vec::new(),
        }
    }

    /// 取某个会话的可变状态（不存在则 `None`）。
    fn session_mut(&mut self, id: &RuntimeId, session: &SessionId) -> Option<&mut SessionState> {
        self.runtimes
            .get_mut(id)
            .and_then(|slot| slot.sessions.get_mut(session))
    }

    /// 若该会话快照有变化：落库并发 `Snapshot` 事件。
    ///
    /// 为什么分两步（先取快照再落库）：`state` 需要 `&mut self`，而 `store` 需要
    /// `&self.store`——两者同时持有会被判为对 `*self` 的冲突借用。所以先拿到
    /// 快照的所有权、结束会话借用，再单独借 `store` 落库。
    /// 这也让「落库失败不打断会话」的语义更清楚：快照已经产出，落库是副作用。
    fn emit_changed(&mut self, id: &RuntimeId, session: &SessionId) -> Option<EngineEvent> {
        let state = self.session_mut(id, session)?;
        let snap = state.take_changed(None)?; // 只做脏检测，不落库。
        if let Some(db) = self.store.as_ref() {
            if let Err(e) = db.persist_snapshot(&snap) {
                eprintln!("[engine] 落库失败（忽略继续）：{e}");
            }
        }
        Some(EngineEvent::Snapshot {
            id: id.clone(),
            session: session.clone(),
            snapshot: Box::new(snap),
        })
    }

    /// 会话级异常拆除：收尾落库 → 移除该 runtime → 报 `Failed`。
    ///
    /// 为什么移除整个 runtime 而不是只标会话：进程已死时该 runtime 下的所有会话都无法继续，
    /// 保留空壳只会让 UI 显示一个点不动的条目。
    fn teardown(&mut self, id: &RuntimeId, reason: String) -> Vec<EngineEvent> {
        // 同样先借出 store，避免与 runtimes 的借用冲突。
        let store = self.store.as_ref();
        if let Some(mut slot) = self.runtimes.remove(id) {
            for state in slot.sessions.values_mut() {
                state.flush(store);
            }
        }
        // 退出落盘：把 runtime 生命周期终点写进 runs 表与 app 轨迹。
        if let Some(db) = store {
            let _ = db.upsert_runtime(id.as_str(), "", "stopped");
        }
        if let Some(rec) = self.recorder.as_ref() {
            rec.app(
                "runtime.exited",
                &serde_json::json!({ "id": id.as_str(), "reason": reason }),
            );
        }
        vec![EngineEvent::Failed {
            id: id.clone(),
            reason,
        }]
    }
}

/// 当前 epoch 毫秒（记录文件名用；`dshr-state` 内多处需要，集中一处避免各写一遍）。
fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
