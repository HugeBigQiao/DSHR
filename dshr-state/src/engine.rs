//! engine 层：核心数据处理。多 runtime / 多会话的路由、折叠、落库。
//!
//! 主要用途：[`Engine`] 是 UI 与 raw 之间的中台——它按 `runtime_id` 路由命令，
//! 把每个 runtime 的通知**按会话**路由到对应的 [`SessionState`]（内含 fold 的 `Folder`），
//! 做脏检测与落库节流，最后把变化过的快照发给 UI。
//! 为什么需要（独立成层）：它的变化频率跟**产品需求**走（多 runtime、多会话、增量、
//! 监控页），而 raw 跟官方发版走。合并二者会让每次官方发版都要动数据管道代码，
//! 且使纯数据逻辑无法用假 driver 单测（见 `DESIGN.md` §3.2）。
//!
//! **与 raw 的职责边界（M3 起）**：
//! - raw：进程 + 协议 + 一条通知流（每 runtime 一条），产出**未折叠事实**；
//! - engine（本层）：registry（多 runtime）+ 每会话 `Folder` + 脏检测 + 落库 Store +
//!   面向 UI 的 `EngineCmd` / `EngineEvent`。
//!
//! **粒度**：本层的对外接口是**双层**的——命令与事件都带 `RuntimeId`，
//! 而 `Prompt` 还带 `SessionId`（一个 runtime 内可并存多个会话；官方按 `sessionId`
//! 惰性创建 agent+session 对）。raw 层不知道「有几个会话」，只管一条管道。
//!
//! 上接：`dshr-ui/src/bridge.rs`（把 UI 意图翻译成 `EngineCmd`、把 `EngineEvent` 刷进视图模型）。
//! 下接：`raw`（`Runtime` 句柄）、`fold`（`Folder` 折叠）、`store`（落库）、
//!       `snapshot`（快照类型）、`dsh_sdk_protocol`（通知解析与类型再出口）。
//! 官方对应：无单点对应。官方把「多会话」放在会话服务（`packages/api/session-controller`）
//! 与客户端 store 里；dshr 是单进程宿主，把等价职责收在本层。

use crate::snapshot::SessionSnapshot;

pub mod registry;
pub mod session;

pub use registry::{Engine, RuntimeId};

/// 共享协议类型再出口：会话快照（`crate::snapshot::SessionSnapshot`）的 status/usage
/// 字段用这两个类型；UI 经本出口使用、不再直接依赖 `dsh-sdk-protocol`。
pub use dsh_sdk_protocol::{llm::TokenUsage, notifications::SessionStatus};

/// 会话目录行的再出口（`EngineEvent::Sessions` 的载荷类型；来自 §8.3 的聚合 SQL）。
pub use crate::store::SessionSummary;

/// UI → engine 命令。
///
/// 为什么每个变体都带 `RuntimeId`：多 runtime 是产品能力（协议里没有 runtime 概念），
/// 引擎靠 id 把命令路由到对应的 [`Runtime`]。
/// `Prompt` 额外带 `SessionId`——一个 runtime 内可并存多个会话。
#[derive(Debug, Clone)]
pub enum EngineCmd {
    /// 新建一个 runtime 条目并立刻启动它。
    ///
    /// 为什么在命令里就带 id：UI 侧边栏要在用户点击那一刻就出现一行
    ///（即便进程还没起来/起来了又失败），所以 id 由调用方生成、engine 只负责落实。
    StartRuntime { id: RuntimeId },
    /// 停止并移除某个 runtime（协议 shutdown + dispose 阶梯）。
    StopRuntime { id: RuntimeId },
    /// 在某个 runtime 内新建一个会话（换新 session id + 清空折叠态）。
    ///
    /// 为什么**不**由调用方给新 session id：id 的生成规则（`s-<epoch>`）属于 state 层，
    /// 且必须唯一（真实 dsh 按 id 落盘日志）。调用方只需说「换一个」，
    /// 新 id 经 `EngineEvent::SessionReset` 回报——避免两侧各有一套生成逻辑。
    ResetSession { id: RuntimeId },
    /// 向某个会话发一条用户消息。
    Prompt {
        id: RuntimeId,
        session: SessionId,
        text: String,
    },
    /// **按需拉取**某个会话的快照（M6 的 B 方案：拉 + 变更通知）。
    ///
    /// 为什么需要「拉」这个方向：推（`EngineEvent::Snapshot`）只在**变更时**发生，而有两种
    /// 场景没有变更可推——① UI 切到另一个会话想看它当前的样子；② 会话早已结束/应用重启过，
    /// 库里还留着完整历史（复原）。这两种都只能由消费方发起读。
    ///
    /// 查找顺序：**运行中的会话优先**（实时态）→ 否则回落到库里的复原（`Store::load_snapshot`）。
    /// 找不到就什么都不发（由 `EngineEvent::Sessions` 的目录告诉 UI 有哪些可选）。
    ReadSnapshot { session: SessionId },
    /// **列出库里的会话目录**（历史会话列表 / 复原入口）。
    ///
    /// 为什么走库而不是问运行中的 runtime：库里装的是「引擎跑过并落盘过的」，正是历史；
    /// 尚在内存里的会话由 `Started` / `SessionReset` / `Snapshot` 事件自带。
    ListSessions,
}

/// 会话 id（engine 生成：`s-<epoch>`）。
///
/// 为什么用 newtype 而不是裸 `String`：`EngineCmd` 里同时出现 runtime 与 session 两个 id，
/// 都是字符串时极易写反（`Prompt { id, session }`）。类型区分让这种错误在编译期暴露。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    /// 包一层（调用方传入已生成的 id 字符串）。
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// 借用内部字符串（协议请求需要 `&str`）。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// engine → UI 事件。
///
/// 与 `EngineCmd` 对称：都带 `RuntimeId`；快照类事件还带会话标识与快照本体。
/// 「Ready」不再出现在这里——总线就绪是 `dshr-ui/src/bridge.rs` 自己产生的事件
///（engine 不参与总线装配）。
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// runtime 启动完成（label = 模式说明，侧边栏展示）。
    Started {
        id: RuntimeId,
        label: String,
        session: SessionId,
    },
    /// 某个会话的快照整体发送（折叠态；每次整发前已同步落库）。
    Snapshot {
        id: RuntimeId,
        session: SessionId,
        snapshot: Box<SessionSnapshot>,
    },
    /// 某个 runtime 内已换新会话（`ResetSession` 的结果；新 id 由 engine 生成）。
    ///
    /// 为什么单独一个事件（而不是只发一条空快照）：UI 需要立刻知道新会话 id 才能
    /// 建侧边栏行、并把 composer 指向新会话；空快照里虽有 `session_id`，但「这是换会话
    /// 而不是同会话刷新」这件事只有本事件能表达。
    SessionReset { id: RuntimeId, session: SessionId },
    /// 某个 runtime 已停止（正常收尾）。
    Stopped { id: RuntimeId, reason: String },
    /// 启动/运行异常（进程退出、spawn/initialize/prompt 失败等；UI 红色提示）。
    Failed { id: RuntimeId, reason: String },
    /// **按需拉取的快照**（`EngineCmd::ReadSnapshot` 的结果）。
    ///
    /// 与 [`EngineEvent::Snapshot`] 的分工：那个是「**变了**，顺手给你」（推送，带 runtime 标）；
    /// 这个是「**我问你要**」（拉取，可能是运行中的实时态，也可能是库里复原的历史态）。
    SessionLoaded {
        session: SessionId,
        /// `Some` = 运行中的会话（当前归属该 runtime）；`None` = **库里复原的历史会话**——
        /// 它当初的 runtime 可能早已不在（进程死了、应用重启过），硬塞一个 id 会让 UI
        /// 把这条历史挂到一行并不存在的 runtime 下面。
        runtime: Option<RuntimeId>,
        snapshot: Box<SessionSnapshot>,
    },
    /// **会话目录**（`EngineCmd::ListSessions` 的结果；库里 `sessions` 表的聚合视图）。
    Sessions { rows: Vec<SessionSummary> },
}
