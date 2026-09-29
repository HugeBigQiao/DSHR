//! raw 层：与 SDK 沟通。把「一个有状态的 runtime 子进程」收敛成可替换接口。
//!
//! 主要用途：[`Runtime`] = **一个 runtime 子进程的句柄**：拉起进程、握手、发消息、收通知、
//! 收尸。它只关心「一条 stdio 管道」，不知道有几个会话、更不碰折叠与落库。
//! 为什么需要（独立成层）：它的变化频率跟**官方发版**走，而 engine 跟产品需求走；
//! 且它是唯一碰进程与管道的层——合并会使数据管道无法用假 driver 单测。
//! 详见 `DESIGN.md` §3.2 / §3.3（"raw 不是薄层，是端口"）。
//!
//! **职责边界（M3 起）**：
//! - 本层持有：目标会话 id、driver（`Box<dyn SessionDriver>`）、通知流、运行模式、展示 label。
//! - 本层**不再持有**：`Folder`（折叠）、`Store`（落库）、`last_sent`（脏检测）——
//!   这三样都归 engine（核心数据处理）。本层产出的 [`RuntimeEvent`] 是**未折叠的事实**。
//!
//! 上接：`crate::engine`（`Engine` 按 runtime 路由命令、聚合各 runtime 的事件流）、
//!       `tests/engine_flow.rs`（注入假 driver 驱动本层，不起进程）。
//! 下接：`raw/driver.rs`（`SessionDriver` 接口）、`raw/client_driver.rs`（真实实现）、
//!       `raw/mode.rs`（Fake/Real 装配）、`dsh_sdk_protocol`（请求与通知类型）。
//!
//! 官方对应：`dsh --profile sdk` 这条 stdio JSON-RPC 通道由官方
//! `packages/bundle/sdk-app`（profile 装配）与 `packages/sdk/server`（服务端）提供；
//! 客户端侧对应官方 `packages/sdk/client`。本层是 dshr 侧的宿主封装，官方无对等物。

use std::time::{SystemTime, UNIX_EPOCH};

use dsh_sdk_protocol::requests::{SdkPromptContentBlock, SessionPromptParams};
use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::{broadcast, mpsc};

use crate::raw::mode::{ResolvedMode, RuntimeMode, SpawnKit};

pub mod client_driver;
pub mod driver;
pub mod mode;

pub use driver::SessionDriver;
/// 再出口：`workspace` 等模块经 `crate::raw::workspace_root()` 取工作区根（唯一定义处在 `mode`）。
pub use mode::workspace_root;

use client_driver::ClientDriver;

/// raw 层对外（给 engine）产出的**未折叠事实**。
///
/// 为什么需要这么一组类型（而不是直接把 `Notification` 丢给 engine）：engine 除了通知，
/// 还要知道「进程起来了吗 / 死了吗 / 为什么死的」。把这三类事实统一成一个枚举，
/// engine 的事件循环就只 match 一处。
#[derive(Debug)]
pub enum RuntimeEvent {
    /// runtime 就绪（spawn + initialize 成功）。`label` 供 UI 展示模式说明。
    Ready { label: String },
    /// 一条来自 runtime 的通知（未折叠；engine 按 `sessionId` 路由到对应会话）。
    Notification(Notification),
    /// runtime 进程的一行 stderr（**进程信号 1**）。
    ///
    /// 为什么必须有这条通道：stderr 是「runtime 为什么出错」的唯一现场记录，
    /// 而协议 4 种通知都不带它。engine 拿到后写 `runtime_logs` 表 + app 轨迹。
    Stderr(String),
    /// stderr 流已结束（进程退出/句柄被收）。engine 据此撤掉该路等待。
    StderrEnded,
    /// 通知流被广播丢弃（消费太慢）。engine 可据此提示「事件有丢失」。
    Lagged(u64),
    /// 进程已退出（通知流 `Closed`）。`exit_code`/`stderr_tail` 来自共享状态，
    /// 是「为什么死的」的唯一来源——协议 4 种通知都不含退出信息。
    Exited {
        exit_code: Option<i32>,
        stderr_tail: Vec<String>,
    },
}

/// runtime 启动失败的原因（spawn 或 initialize 阶段）。
///
/// 为什么单独成类型而不是 `String`：engine 要把「启动失败」当成一次可展示的错误，
/// 同时保留「是不是已收尾」的区分；用类型而不是字符串便于将来细分 UI 呈现。
#[derive(Debug)]
pub struct StartError(pub String);

/// 通知接收端的返回类型（`broadcast::Receiver::recv` 的结果）。
///
/// 为什么起别名：它在 `next_event` 的 select 分支里出现多次，且带泛型错误类型，
/// 写全了会淹没真正重要的逻辑。
type NotifOut = Result<Notification, broadcast::error::RecvError>;

/// 一个 runtime 子进程的句柄。
///
/// 为什么需要：见模块头——它是「进程 + 管道」在宿主侧的唯一代表。
/// 一次 `start` 绑定一个会话 id（官方按 id 惰性建会话对）；`reset_session` 换新 id。
pub struct Runtime {
    /// 本 runtime 的标识（engine 生成；落库时作为 `runtime_logs.runtime_id` / `runtimes.id`）。
    ///
    /// 为什么 raw 也持有一份：stderr 与退出都是 **runtime 级**事实，落库需要 id。
    /// 让 engine 在每个事件上附带 id 也行，但 raw 自己知道「我是谁」更不容易漏。
    id: crate::engine::RuntimeId,
    /// 驱动（`None` = 未启动/已停）。
    ///
    /// 用 trait 对象而非具体客户端：`tests/engine_flow.rs` 才能注入假 driver 做单测。
    driver: Option<Box<dyn SessionDriver>>,
    /// 通知流接收端（与 driver 同生共死；进程退出时收到 `Closed`）。
    events: Option<broadcast::Receiver<Notification>>,
    /// stderr 行接收端（**进程信号 1**；`None` = 未取出或未启动）。
    ///
    /// 为什么 raw 要持有它：stderr 来自进程管道，是 raw 的资产（engine 不该碰管道）。
    /// raw 把它作为 `RuntimeEvent::Stderr` 向上报（单消费者流）。
    stderr: Option<mpsc::UnboundedReceiver<String>>,
    /// 当前会话 id。
    ///
    /// 为什么由 raw 持有：`prompt` 需要它（协议要求 `sessionId`），而 raw 是唯一发请求的层。
    /// engine 也持有一份（用于路由），两者由 engine 在 `start`/`reset_session` 时保持一致。
    session_id: String,
    /// 当前运行模式（Fake 需本地回显用户行——fake runtime 不发送 `user/message`）。
    mode: Option<RuntimeMode>,
    /// 展示用 label（模式 + 回落说明）。
    label: String,
    /// 进程是否已退出（由 `next_event` 观察到 `Closed` 时置位）。
    ///
    /// 为什么需要：engine 要区分「主动 stop」与「意外退出」，后者要报错并把
    /// exit code + stderr 尾部一起呈现。
    exited: bool,
    /// 测试接缝：强制 Fake（跳过 `config.json` 判定；生产恒 `false`）。
    fake_only: bool,
}

impl Runtime {
    /// 未启动的空句柄（`start` 之前的状态）。
    ///
    /// # 参数
    /// - `id`：本 runtime 的标识（engine 生成，落库与日志都用它）。
    ///
    /// 为什么需要 id 在建句柄时就有：engine 收到 `StartRuntime { id }` 那一刻
    /// 就要能建槽位（UI 侧边栏立刻出现一行），而 stderr/退出落盘都要用它。
    pub fn new(id: crate::engine::RuntimeId) -> Self {
        Self {
            id,
            driver: None,
            events: None,
            stderr: None,
            session_id: String::new(),
            mode: None,
            label: String::new(),
            exited: false,
            fake_only: false,
        }
    }

    /// 本 runtime 的标识。
    pub fn id(&self) -> &crate::engine::RuntimeId {
        &self.id
    }

    /// 测试接缝：强制 Fake 模式（跳过 `config.json` 判定——测试机若配了真 api-key，
    /// 不能让它去拉真实 dsh）。
    pub fn force_fake(&mut self) {
        self.fake_only = true;
    }

    /// 当前会话 id。
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 展示用 label（未启动时为空）。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 运行模式（未启动时 `None`）。
    pub fn mode(&self) -> Option<RuntimeMode> {
        self.mode
    }

    /// 是否已启动（driver 在位且进程未退出）。
    pub fn is_running(&self) -> bool {
        self.driver.is_some() && !self.exited
    }

    /// 是否已观察到进程退出。
    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// 拉起 runtime：模式判定 → spawn → initialize。
    ///
    /// # 参数
    /// - `wire_log_path`：本次运行的线级记录路径（`None` = 不记录）。由 engine 决定——
    ///   它同时要把这个路径交给 `Recorder` 写 app 轨迹，**必须两边同源**，
    ///   所以路径的生成权归 engine，而不是这里自己拼。
    ///
    /// # 返回 / 错误
    /// 成功返回就绪 label（engine 用作 `Started` 的展示文案）；失败返回 [`StartError`]，
    /// 且**句柄保持未启动态**（可以再次 `start` 重试）。
    ///
    /// # 为什么整条装配链在这里
    /// `mode::kit` 决定了「Fake 还是 Real、用哪个 bin、环境变量怎么给」——那是**进程怎么起**
    /// 的问题，属于本层；engine 不该知道 fake 与 real 的区别。
    /// Real 模式下 `kit` 可能触发 pnpm 安装（分钟级），故用 `spawn_blocking` 包住，
    /// 免得卡住 tokio 执行器。
    pub async fn start(&mut self, wire_log_path: Option<String>) -> Result<String, StartError> {
        if self.driver.is_some() {
            return Ok(self.label.clone()); // 已在运行：幂等返回现有 label。
        }
        let resolved = if self.fake_only {
            ResolvedMode {
                mode: RuntimeMode::Fake,
                note: String::new(),
            }
        } else {
            mode::resolve_mode()
        };
        let ws = workspace_root();
        let mut kit = match resolved.mode {
            RuntimeMode::Fake => mode::kit(RuntimeMode::Fake, &ws)
                .map_err(|e| StartError(format!("Fake 装配失败：{e}")))?,
            RuntimeMode::Real => {
                let ws = ws.clone();
                match tokio::task::spawn_blocking(move || mode::kit(RuntimeMode::Real, &ws)).await {
                    Ok(Ok(kit)) => kit,
                    Ok(Err(e)) => return Err(StartError(format!("装配失败：{e}"))),
                    Err(e) => return Err(StartError(format!("装配任务失败：{e}"))),
                }
            }
        };
        // 回落说明挂到 label（状态栏可见："Fake runtime（无 config.json）"）。
        if !resolved.note.is_empty() {
            kit.label = format!("{}（{}）", kit.label, resolved.note);
        }
        kit.config.wire_log_path = wire_log_path;
        self.spawn_driver(kit).await
    }

    /// spawn + initialize 的实际执行体（`start` 的模式判定之后）。
    ///
    /// 为什么与 `start` 分开：`start` 负责**策略**（Fake/Real 判定与装配），
    /// 本函数负责**动作**（拉起 driver 并握手）。engine 的测试注入路径也需要
    /// 一个「直接给出 kit」的入口，将来可据此加 `start_with(kit)` 接缝。
    async fn spawn_driver(&mut self, kit: SpawnKit) -> Result<String, StartError> {
        let label = kit.label.clone();
        let mut driver: Box<dyn SessionDriver> = match ClientDriver::spawn(kit.config).await {
            Ok(d) => Box::new(d),
            Err(e) => return Err(StartError(format!("runtime 启动失败：{e}"))),
        };
        if let Err(e) = driver.initialize(&kit.init).await {
            // 进程活着但握手失败（真实 dsh 鉴权/参数错）→ 优雅收尾再报。
            let _ = driver.shutdown().await;
            return Err(StartError(format!("initialize 失败：{e}")));
        }
        self.events = Some(driver.events());
        // stderr 是单消费者流：取出后由本层作为事实上报（`take_stderr` 交出所有权）。
        self.stderr = Some(driver.stderr());
        self.session_id = gen_session_id();
        self.mode = Some(kit.mode);
        self.label = label.clone();
        self.exited = false;
        self.driver = Some(driver);
        Ok(label)
    }

    /// 发一条用户消息到当前会话。
    ///
    /// # 参数
    /// - `text`：已 trim 的非空文本（调用方负责校验空串）。
    ///
    /// # 返回 / 错误
    /// 成功返回入队回执 `messageId`；失败返回原因字符串（engine 据此报 `Failed`
    /// 并决定是否拆除）。**不在这里做错误处理策略**——重试/拆除是 engine 的决定。
    pub async fn prompt(&mut self, text: &str) -> Result<String, String> {
        let Some(driver) = self.driver.as_mut() else {
            return Err("runtime 未启动".to_string());
        };
        let params = SessionPromptParams {
            session_id: self.session_id.clone(),
            content_blocks: vec![SdkPromptContentBlock::text(text)],
        };
        driver
            .prompt(&params)
            .await
            .map(|r| r.message_id)
            .map_err(|e| e.to_string())
    }

    /// 换一个新会话 id（「新建会话/删除会话」语义）。
    ///
    /// 为什么不需要通知 runtime：官方按 `sessionId` 惰性创建 agent+session 对
    ///（`session/prompt` 的注释），所以新 id 会在下一次 `prompt` 时自动生效。
    /// 返回新 id，便于 engine 同步自己的路由表。
    pub fn reset_session(&mut self) -> String {
        self.session_id = gen_session_id();
        self.session_id.clone()
    }

    /// 是否是 Fake 模式（engine 据此决定要不要本地回显用户行）。
    ///
    /// 为什么这个问题要问 raw：模式判定只在 `mode` 里发生一次（见 `start`），
    /// engine 不该重复判断「config 在不在、key 有没有」。
    pub fn is_fake(&self) -> bool {
        self.mode == Some(RuntimeMode::Fake)
    }

    /// 本地合成一条 `user/message` 事件（Fake 模式专用）。
    ///
    /// # 为什么需要
    /// fake runtime **不回发** `user/message`，所以真实模式下由 runtime 自己产生的那一行，
    /// 在 Fake 模式下必须由宿主补上，否则 UI 看不到用户发过的话。
    ///
    /// # 返回
    /// 一条可直接交给 fold 的 `SessionEvent`（engine 负责喂给 `Folder`）。
    /// 形状与官方 wire 完全一致（fake runtime 的 `assistant/message` 才能与它配对）。
    pub fn synth_user_message(
        text: &str,
        seq: u64,
    ) -> dsh_sdk_protocol::session_event::SessionEvent {
        serde_json::from_value(serde_json::json!({
            "type": "user/message", "seq": seq, "time": epoch_ms(),
            "data": {
                "id": format!("m-ui-{seq}"),
                "role": "user",
                "content": [{ "type": "text", "text": text }],
                "source": { "kind": "user" },
            },
        }))
        .expect("官方 user/message 形状应可解析")
    }

    /// 等下一件事实（通知 ∨ stderr ∨ 退出）。
    ///
    /// # 返回
    /// - `Some(RuntimeEvent)`：一件事实；
    /// - `None`：句柄未启动（engine 应把该 runtime 从 select 里剔除）。
    ///
    /// # 为什么是「拿一件」而不是「一批」
    /// 事件的折叠与去重发生在 engine（它才有 `Folder` 与 `last_sent`）。
    /// 本层只负责把进程侧的事实**如实**递上去，不做批处理、不做过滤——
    /// 这样 engine 才能按 `sessionId` 自由路由。
    ///
    /// # 为什么 stderr 也走这个函数（而不是单独一个监听任务）
    /// stderr 与通知一样是「进程侧异步到达的事实」。若为它单起一个任务，
    /// 就要再解决「任务何时收敛」（进程退出后谁 abort？）——而放进同一个 select
    /// 里，两者共享同一条生命周期：都是本句柄的一部分。
    pub async fn next_event(&mut self) -> Option<RuntimeEvent> {
        // 先判退出位：进程已退出时即使流已被清空，也要把退出事实报出去。
        // 真实路径里这里通常由下面的 `Closed` 分支置位，但**测试接缝**（以及将来
        // 由 driver 直接观察到退出的情况）可能先置位——两种情况都必须上报。
        if self.exited {
            return Some(self.exit_event());
        }
        // 两个接收端都未就绪 = 未启动：不要在这里永久挂起，把决定权交回 engine。
        if self.events.is_none() && self.stderr.is_none() {
            return None;
        }

        // 先把「本轮的形状」定下来，再进入 select。
        // 为什么要分两步：`tokio::select!` 的分支是**静态展开**的（不能按 Option 增减），
        // 所以用「哪个通道活着」组合出四种情况，各自只 await 活着的那些。
        // 这样也避免了「stderr 已结束」时它的 `recv()` 立刻返回 None 从而饿死通知分支。
        let has_notif = self.events.is_some();
        let has_err = self.stderr.is_some();

        /// select 的原始结果（未解释）。
        enum Raw {
            Notif(NotifOut),
            Stderr(Option<String>),
        }

        let raw = match (has_notif, has_err) {
            (true, true) => {
                let ev = self.events.as_mut().expect("已判 Some");
                let er = self.stderr.as_mut().expect("已判 Some");
                tokio::select! {
                    n = ev.recv() => Raw::Notif(n),
                    e = er.recv() => Raw::Stderr(e),
                }
            }
            (true, false) => {
                let ev = self.events.as_mut().expect("已判 Some");
                Raw::Notif(ev.recv().await)
            }
            (false, true) => {
                let er = self.stderr.as_mut().expect("已判 Some");
                Raw::Stderr(er.recv().await)
            }
            // 已由上面的早期 return 排除，这里只为穷尽。
            (false, false) => return None,
        };

        // 解释结果。注意：到了这里上一段的所有借用都已结束，
        // 因此可以安全地改 `self.stderr` / 调 `self.exit_event()`。
        match raw {
            Raw::Notif(Ok(n)) => Some(RuntimeEvent::Notification(n)),
            // 消费太慢被广播丢事件：不算错误（官方 lagging 语义），但要如实上报数量。
            Raw::Notif(Err(broadcast::error::RecvError::Lagged(n))) => {
                Some(RuntimeEvent::Lagged(n))
            }
            // 广播关闭 = runtime 进程退出（读循环 EOF）。
            Raw::Notif(Err(broadcast::error::RecvError::Closed)) => {
                self.exited = true;
                Some(self.exit_event())
            }
            Raw::Stderr(Some(line)) => Some(RuntimeEvent::Stderr(line)),
            // stderr 流结束（进程退出后管道 EOF）：交出句柄，下次不再等它。
            Raw::Stderr(None) => {
                self.stderr = None;
                Some(RuntimeEvent::StderrEnded)
            }
        }
    }

    /// 组一条「进程已退出」事实（exit code + stderr 尾部取自共享状态）。
    ///
    /// 为什么退出信息要单独取：协议 4 种通知**不含**退出信息，
    /// 「怎么死的」只能从 `RuntimeStatus` 查——这也是把 `runtime_status` 放进
    /// `SessionDriver` 的原因。
    fn exit_event(&mut self) -> RuntimeEvent {
        let (exit_code, stderr_tail) = match self.driver.as_mut() {
            Some(d) => {
                let st = d.runtime_status();
                (st.exit_code(), st.stderr_tail())
            }
            None => (None, Vec::new()),
        };
        RuntimeEvent::Exited {
            exit_code,
            stderr_tail,
        }
    }

    /// 停止：协议 `shutdown` → dispose 阶梯。
    ///
    /// # 返回 / 错误
    /// 收尸结果（`Err` 只在强杀后仍未退出时出现，见 `SessionDriver::shutdown`）。
    /// 无论成败，句柄都回到「未启动」态（`driver` 置空），以便再次 `start`。
    ///
    /// 为什么吞掉「已停止」：`Stop` 是幂等的用户操作，重复点不该报错。
    pub async fn stop(&mut self) -> Result<(), dsh_sdk_client::client::Error> {
        let Some(mut driver) = self.driver.take() else {
            return Ok(());
        };
        let r = driver.shutdown().await;
        self.events = None;
        self.stderr = None;
        self.mode = None;
        r
    }

    /// 测试接缝：注入 driver 与通知流，跳过 spawn（M3 的 engine 测试靠它不起进程）。
    ///
    /// # 参数
    /// - `driver`：假 driver；
    /// - `session_id`：会话 id；
    /// - `events`：通知流接收端（测试自己持有发送端以驱动事件）；
    /// - `mode`：运行模式（决定 engine 是否本地回显用户行）。
    ///
    /// 为什么需要：没有它，任何「多 runtime 路由 / 折叠 / 落库」的测试都必须真的
    /// 拉起 node 子进程。这是 `SessionDriver` 抽象兑现价值的地方。
    pub fn with_driver_for_test(
        &mut self,
        driver: Box<dyn SessionDriver>,
        session_id: String,
        events: broadcast::Receiver<Notification>,
        mode: RuntimeMode,
    ) {
        self.driver = Some(driver);
        self.events = Some(events);
        self.session_id = session_id;
        self.mode = Some(mode);
        self.label = "test".to_string();
        self.exited = false;
    }

    /// 测试接缝：模拟「进程已退出」——清掉通知流并置退出位。
    ///
    /// 为什么要这个接缝（而不是让测试 drop 广播发送端）：`FakeDriver` 自己持有一份
    /// 发送端克隆，所以测试手里的发送端释放**不会**关闭流；而且真实路径里
    /// 「进程退出」= transport 读循环 EOF 后关闭广播，与「谁持有发送端」无关。
    /// 直接置位能精确模拟该状态。
    ///
    /// 注意**保留** `events`：早退分支靠 `exited` 位直接产出 `Exited`（不需要流真的关闭），
    /// 所以这里不能把流清掉——清了就只剩 `None`，engine 只会认为「这个句柄没在等」。
    pub fn mark_exited_for_test(&mut self) {
        self.exited = true;
    }

    /// 测试接缝：注入 stderr 接收端（验证「stderr → runtime_logs 表」整条链路）。
    ///
    /// 为什么需要：假 driver 的 `stderr()` 返回空通道，测试无法制造 stderr 行。
    /// 有了它，测试可自持发送端推一行，覆盖
    /// `RuntimeEvent::Stderr` → engine 写 `runtime_logs` + app 轨迹。
    pub fn inject_stderr_for_test(&mut self, rx: mpsc::UnboundedReceiver<String>) {
        self.stderr = Some(rx);
    }
}

/// 会话 id 唯一化（epoch ms）。
///
/// 为什么必须唯一：真实 dsh 按 id 落盘会话日志，复用固定 id 会撞上
/// `session already has a persisted log on disk`（见 `DESIGN.md` §10）。
pub(crate) fn gen_session_id() -> String {
    format!("s-{}", epoch_ms())
}

pub(crate) fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
