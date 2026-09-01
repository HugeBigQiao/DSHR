//! 视图模型：UI 消费的数据形状（纯数据 + Clone，无 state/SDK 类型依赖）。
//!
//! 转接原则（旧 DESIGN §9.5）：state 层负责把 SDK 事件/通知翻译成这里的形状，
//! UI 只渲染视图模型；本模块不 import 任何 state/SDK 类型。

/// 一个会话（runtime 树叶子）。
#[derive(Debug, Clone)]
pub struct SessionView {
    pub id: String,
    pub title: String,
}

/// 一个 runtime（= 一个 dsh 子进程，含其会话树）。
#[derive(Debug, Clone)]
pub struct RuntimeView {
    pub id: String,
    pub name: String,
    /// 会话列表是否展开（runtime 行 +/− 切换）。
    pub expanded: bool,
    pub sessions: Vec<SessionView>,
    pub selected_session: Option<String>,
}

/// 消息种类（决定渲染形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    /// 用户消息（全宽行，名字行 + 文本）。
    User,
    /// 助手消息（markdown 渲染，代码高亮）。
    Assistant,
    /// 思考过程（灰字折叠行）。
    Reasoning,
    /// 工具调用（摘要卡片，可展开）。
    Tool,
    /// 系统/上下文一行小字（compaction/context 注入等；真实桥接入后构造）。
    #[allow(dead_code)]
    Notice,
}

/// 一条消息。
#[derive(Debug, Clone)]
pub struct MsgView {
    pub kind: MsgKind,
    /// user/assistant 的文本（markdown 源）；Notice 时是一行说明。
    pub text: String,
    /// Reasoning 消息的思考文本。
    pub reasoning: Option<String>,
    /// Tool 消息的卡片内容。
    pub tool: Option<ToolView>,
    /// 时间标签（同天 HH:mm，后续本地化）。
    pub time_label: String,
}

/// 工具调用卡片。
#[derive(Debug, Clone)]
pub struct ToolView {
    pub name: String,
    pub duration_ms: u64,
    /// 结果摘要（前几行）。
    pub summary: String,
    pub is_error: bool,
    pub expanded: bool,
}

/// 对话区状态（消息流 + 统计行；composer 草稿在 App 的 text_editor Content 里）。
#[derive(Debug, Clone, Default)]
pub struct ChatState {
    pub session_id: String,
    pub messages: Vec<MsgView>,
    /// idle / running。
    pub status: String,
    /// 统计行文本（模型 / tokens / 耗时 / token/s；state 接入后填充）。
    pub stats: String,
}

/// 应用数据快照（bridge 每帧/每次事件提供）。
#[derive(Debug, Clone, Default)]
pub struct AppData {
    /// runtime 树（每个 runtime = 一个 dsh 子进程 + 其会话）。
    pub runtimes: Vec<RuntimeView>,
    pub chat: ChatState,
}
