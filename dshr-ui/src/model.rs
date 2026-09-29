//! 视图模型：UI 消费的数据形状（纯数据 + Clone，渲染层只 import 本模块）。
//!
//! s3 升级（DESIGN.md §7.2）：不再由占位桥喂假数据，而是把
//! `dshr_state::fold::Folder` 的折叠快照（snapshot.rs 纯数据）映射成这里的形状。
//! 依赖方向 ui → state：本模块可 import dshr_state::snapshot（共享纯数据层，
//! 它不 import 本 crate）；共享协议类型（SessionStatus/TokenUsage）经
//! dshr_state::raw 再出口使用——UI 不直接依赖 dsh-sdk-protocol。snapshot 整体
//! 替换式刷新（engine 每事件发一次快照，消息行/统计在映射处重建；增量传输留后续）。
//!
//! 主要用途：定义渲染层真正消费的所有类型（ChatState/MsgView/RuntimeView/ChatStats…）与
//! 纯格式化函数（时间、token 数、统计行、会话标题），并负责「快照 → 视图」的唯一映射
//! （`ChatState::apply_snapshot`、`MsgView::from_item`）。
//! 为什么需要：它是 state 快照与 iced 渲染之间的**防腐层**——快照由协议驱动（字段随官方发版
//! 变），视图模型由 UI 驱动（字段随页面变）。不隔这一层，`fold` 的字段调整会波及每个渲染函数；
//! 隔了这一层，协议漂移只在这里一次性消化。它也约束：渲染层不得直接 import
//! `dshr_state::snapshot`/协议类型（`MsgKind` 是唯一经 `pub use` 放行的类型）。
//! 上接：`app::App`（`data: AppData`）、`task/{sidebar,chat}`、`statusbar`。
//! 下接：`dshr_state::snapshot`（SessionSnapshot/MsgItem/ToolItem/StreamSummary）、
//! `dshr_state::raw`（再出口的 SessionStatus/TokenUsage）。
//! 官方对应：无同名文件——官方在 TS 侧由 `packages/client/ui-chat/src/client/conversation-nodes/`
//! 的投影（`chat-snapshot-builder.ts`、`event-projection.ts`）产出视图节点；
//! token 格式化对应 `packages/client/ui-chat/src/client/chat/token-format.ts`。

use dshr_state::engine::{SessionStatus, TokenUsage};
use dshr_state::snapshot::{MsgItem, SessionSnapshot, StreamSummary, ToolItem};

// 消息种类直接复用 state 快照的种类（User/Assistant/Reasoning/Tool/Notice 一一对应）。
pub use dshr_state::snapshot::MsgKind;

/// 对话/运行生命周期状态（由 engine 事件 + 快照 session.status 推导）。
///
/// 为什么需要：协议面的 `SessionStatus` 只有 idle/running，但 UI 还要区分「还没启动」
/// 「用户停了」「出错了」——这三种在协议里没有取值，只能由 engine 事件（Started/Stopped/Failed）
/// 补成 UI 侧状态。渲染统一读它取色/取文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatStatus {
    /// 未启动 runtime（App 初始态）。
    Off,
    /// 会话就绪/agent idle（session.status idle；快照状态 None 按 idle 显示）。
    Idle,
    /// 运行中（session.status running；worker 在 prompt 后乐观置 running）。
    Running,
    /// 已停止（用户停止；消息流保留可查）。
    Stopped,
    /// 启动/运行异常（进程退出、请求失败等；红色提示）。
    Failed,
}

impl ChatStatus {
    /// 状态短标签（英文小写为主，与官方 running/idle 术语一致）。
    ///
    /// 为什么需要：同一状态要出现在对话区、底部小字与底部图标栏三处，文案必须一致；
    /// 返回 `&'static str` 避免每次渲染都分配字符串。
    /// 入参/出参：无；返回静态文案（`未启动` / `idle` / `running` / `已停止` / `错误`）。
    pub fn label(self) -> &'static str {
        match self {
            ChatStatus::Off => "未启动",
            ChatStatus::Idle => "idle",
            ChatStatus::Running => "running",
            ChatStatus::Stopped => "已停止",
            ChatStatus::Failed => "错误",
        }
    }
}

/// 一个会话（runtime 树叶子；s3 简化：单 runtime 单会话，多会话目录管理留 s4）。
///
/// 为什么需要：侧边栏要画「会话行」，但快照里没有独立的会话目录（s3 的单会话形态），
/// 所以视图侧持有一份最小描述（id + 展示标题）即可；标题由 `session_title` 在无定题时兜底。
#[derive(Debug, Clone)]
pub struct SessionView {
    pub id: String,
    pub title: String,
}

/// 一个 runtime（= 一个 dsh 子进程）。s3 收敛为单 runtime：vec 恒 ≤1 项，
/// 多 runtime 管理（每 runtime 一个进程/会话树）留后续。
///
/// 为什么需要：侧边栏是树形（runtime → 会话），即使当前只有一项也必须保留层级形状，
/// 否则多 runtime 落地时要重写整棵树的渲染与命中判定。
#[derive(Debug, Clone)]
pub struct RuntimeView {
    pub id: String,
    /// 展示名（worker Started 事件带模式说明：如 "Fake runtime（未配置 API key）"）。
    pub name: String,
    pub expanded: bool,
    pub sessions: Vec<SessionView>,
    pub selected_session: Option<String>,
}

/// assistant 消息的 token 账目（快照 TokenUsage → 视图纯字段；渲染按需格式化）。
///
/// 为什么需要：`TokenUsage` 的三个缓存/推理桶在协议里是 `Option`，而 UI 的加减汇总用 0 更省事；
/// 这里做一次「None → 0」的归一，渲染层就不必到处 `unwrap_or(0)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub total: u64,
}

impl TokenCounts {
    /// 快照 usage → 视图 token 账目（缺失的可选桶按 0 计）。
    ///
    /// 入参/出参：`u` 为快照里的 usage 引用；返回填好的 `TokenCounts`（纯值，无失败路径）。
    fn from_usage(u: &TokenUsage) -> Self {
        Self {
            input: u.input_tokens,
            output: u.output_tokens,
            cache_read: u.cache_read_tokens.unwrap_or(0),
            cache_write: u.cache_write_tokens.unwrap_or(0),
            reasoning: u.reasoning_tokens.unwrap_or(0),
            total: u.total_tokens.unwrap_or(0),
        }
    }
}

/// 一条消息（kind 对齐快照 MsgKind；工具行携带快照 ToolItem 内容）。
///
/// 为什么需要：快照里的 `MsgItem` 是「协议字段的集合」，而渲染要的是「已经能直接画」的行
/// （时间格式化好、token 归一好、工具卡内容已配对）；把这次转换固定成 `from_item` 一处，
/// 渲染函数就完全不碰原始时间戳/可选字段。
#[derive(Debug, Clone)]
pub struct MsgView {
    pub kind: MsgKind,
    /// User/Assistant 正文（markdown 源）；Notice 时是一行说明；Tool/Reasoning 行通常为空。
    pub text: String,
    /// Reasoning 行的思考文本（snapshot MsgItem.reasoning 照搬）。
    pub reasoning: Option<String>,
    /// Assistant 消息的 token 账目（无则 None）。已映射但 UI 渲染待 turn-tail/详情
    /// （DESIGN 会话消息流阶段），先保留字段。
    #[allow(dead_code)]
    pub usage: Option<TokenCounts>,
    /// v3 assistant stream 摘要（无流记录 = None）。
    pub stream: Option<StreamSummary>,
    /// Tool 行的卡片内容（快照 call↔result 配对后的 ToolItem：name/call_id/duration/
    /// is_error/result/diffs）。
    pub tool: Option<ToolItem>,
    /// 时间标签：由事件 time(epoch ms) 格式化（UTC HH:mm；本地化待办，见 hhmm_utc）。
    pub time_label: String,
    /// 事件 seq（稳定序；工具卡展开状态按它索引——UI 侧 expanded 集合放 App）。
    pub seq: u64,
}

impl MsgView {
    /// 快照消息项 → 视图行（时间格式化 + token 归一 + 工具卡内容照搬）。
    ///
    /// 入参/出参：`item` 为快照 `MsgItem` 引用；返回可直接渲染的 `MsgView`（无失败路径）。
    fn from_item(item: &MsgItem) -> Self {
        Self {
            kind: item.kind,
            text: item.text.clone(),
            reasoning: item.reasoning.clone(),
            usage: item.usage.as_ref().map(TokenCounts::from_usage),
            stream: item.stream,
            tool: item.tool.clone(),
            time_label: hhmm_utc(item.time),
            seq: item.seq,
        }
    }
}

/// 会话级统计（由快照 SessionStats 映射；渲染在 chat.rs 按需格式化）。
///
/// 为什么需要：统计是快照里已经聚合好的事实，视图只需一份 `Copy` 的值就能画统计行/工具计数；
/// 缓存/reasoning 桶在这里保留但暂不入行（监控页要），避免统计行字段将来又要改结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatStats {
    pub turns: u64,
    pub steps: u64,
    pub messages: u64,
    pub tool_calls: u64,
    /// usage 六桶：input/output/cache_read/cache_write/reasoning/total。
    pub usage: TokenCounts,
    pub errors: u64,
}

/// 对话区状态（消息流 + 统计；composer 草稿在 App 的 text_editor Content 里）。
/// status/status_line 之外全部由快照整体刷新（apply_snapshot）。
///
/// 为什么需要：这是「一次会话的可见内容」的完整承载物——engine 每次折叠完发一份快照，
/// 本结构就是它的 UI 侧对应体；`status_line` 特意不随快照刷新（它承载的是快照里没有的
/// 启动提示/停止原因/失败原因）。
#[derive(Debug, Clone)]
pub struct ChatState {
    pub session_id: String,
    /// 会话标题（session/title 折叠而来；None = 未定题，侧边栏显示"会话 <短id>"）。
    pub title: Option<String>,
    pub status: ChatStatus,
    pub messages: Vec<MsgView>,
    pub stats: ChatStats,
    /// 状态行补充说明（未启动提示 / 停止原因 / 启动失败原因；快照刷新不清除）。
    pub status_line: String,
}

impl Default for ChatState {
    /// 初始态：无会话、`Off`，状态行提示去点「新建 runtime」。
    ///
    /// 为什么需要：未启动时页面不能空白——状态行必须告诉用户下一步动作；
    /// 抽成 `Default` 是为了让 `ChatState::new()` 与 `App::handle_bridge` 里的重置共用一份文案。
    fn default() -> Self {
        Self {
            session_id: String::new(),
            title: None,
            status: ChatStatus::Off,
            messages: Vec::new(),
            stats: ChatStats::default(),
            status_line: "未启动：点侧边栏「＋ 新建 runtime」".to_string(),
        }
    }
}

impl ChatState {
    /// 初始态别名（`Default` 的显式构造入口）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 以一份会话快照整体替换视图内容（worker 每事件折叠后发送）。
    /// 会话生命周期状态从快照 session.status 映射：Running → running；
    /// Idle/None（未收到状态）→ idle。
    ///
    /// 为什么需要：s3 采用「整份替换」而不是增量合并——快照本身就是 fold 的确定性输出，
    /// 整份替换让 UI 永不与 engine 状态漂移（代价是每次重建消息行，见文件头说明）。
    /// 入参/出参：`snap` 为 engine 发来的会话快照引用；无返回值，就地改写。
    /// 注意：`status_line` 不在本函数职责内（快照里没有），调用方负责在 Started/Stopped/Failed
    /// 时另行设置。
    pub fn apply_snapshot(&mut self, snap: &SessionSnapshot) {
        if !snap.session_id.is_empty() {
            self.session_id = snap.session_id.clone();
        }
        if let Some(title) = &snap.title {
            if !title.is_empty() {
                self.title = Some(title.clone());
            }
        }
        self.status = match snap.status {
            Some(SessionStatus::Running) => ChatStatus::Running,
            _ => ChatStatus::Idle,
        };
        self.messages = snap.messages.iter().map(MsgView::from_item).collect();
        self.stats = ChatStats {
            turns: snap.stats.turns,
            steps: snap.stats.steps,
            messages: snap.stats.messages,
            tool_calls: snap.stats.tool_calls,
            usage: TokenCounts {
                input: snap.stats.usage.input,
                output: snap.stats.usage.output,
                cache_read: snap.stats.usage.cache_read,
                cache_write: snap.stats.usage.cache_write,
                reasoning: snap.stats.usage.reasoning,
                total: snap.stats.usage.total,
            },
            errors: snap.stats.errors,
        };
    }
}

/// 应用数据（bridge 事件驱动，不再是 PlaceholderBridge 一次性快照）。
///
/// 为什么需要：`App` 里把「树」与「当前对话」打包成一个字段，是为了让 `view` 只读一处、
/// 也便于将来把 `AppData` 整体交给监控页/详情页做只读展示。
#[derive(Debug, Clone, Default)]
pub struct AppData {
    /// runtime 树（s3 收敛：至多一个 runtime + 一个当前会话）。
    pub runtimes: Vec<RuntimeView>,
    pub chat: ChatState,
}

// —— 纯格式化（不引 chrono：整除直接算，输出 UTC；本地化待办见注释）——

/// epoch 毫秒 → "HH:mm"（UTC）。不依赖系统时区/chrono：先按 UTC 折算并注释
/// 「本地化待办」（桌面端未来接 iced_time/chrono-tz 时换成本地时区）。
///
/// 为什么需要：消息行只需要「几点几分」，为此引 chrono/时区库不划算（也会把依赖树拉大）；
/// 整除即可，且纯函数便于单测。
/// 入参/出参：`epoch_ms` 为事件 `time`（epoch 毫秒）；返回 `HH:mm`（UTC，超 24h 自动回绕）。
pub fn hhmm_utc(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{h:02}:{m:02}")
}

/// token 数简短显示：≥1000 → "1.2k"（统计行/工具卡用）。
///
/// 为什么需要：统计行长且窄，四位数以上会把行挤爆；保留一位小数的 k 记法够表达量级。
/// 入参/出参：`n` 为 token 数；返回显示串（<1000 用原十进制）。
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// 统计行文本（对话区底部）：轮 · 步 · 消息 · in/out tokens · 错误。
/// 缓存/reasoning 桶不入行（监控页 s4 展示），字段仍保留在 ChatStats。
///
/// 为什么需要：同一行要同时出现在 composer 上方与底部小字里，格式必须在唯一处生成；
/// 也是 model.rs 的纯函数单测覆盖点（`fmt_tokens_and_stats_line`）。
/// 入参/出参：`s` 为会话统计；返回单行展示文本。
pub fn stats_line(s: &ChatStats) -> String {
    format!(
        "{} 轮 · {} 步 · {} 条 · ↑{} · ↓{} · err {}",
        s.turns,
        s.steps,
        s.messages,
        fmt_tokens(s.usage.input),
        fmt_tokens(s.usage.output),
        s.errors
    )
}

/// 会话短 id（侧边栏标题用："s-1738…" → 取 '-' 后数字尾 6 位）。
///
/// 为什么需要：会话 id 是长 epoch 前缀串，侧边栏行宽有限；取尾 6 位既短又足以在单 runtime
/// 内区分（多会话目录落地后需重新评估唯一性）。
/// 入参/出参：`id` 为完整 session id；返回短 id（不足 6 位时全显；截断时前缀 `…`）。
pub fn short_id(id: &str) -> String {
    let tail = id.rsplit('-').next().unwrap_or(id);
    let mut chars = tail.chars();
    let last: String = chars
        .by_ref()
        .rev()
        .take(6)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if last.len() < tail.chars().count() {
        format!("…{last}")
    } else {
        tail.to_string()
    }
}

/// 会话行标题：定题用标题，否则"会话 <短id>"。
///
/// 为什么需要：`session/title` 事件不是每条会话都会到（短会话可能一直没定题），侧边栏不能
/// 因此显示空白行；这里给出稳定兜底，侧边栏与 App 的会话行同步共用它保证两处一致。
/// 入参/出参：`title` 为快照标题（None/空串视为未定题）；`id` 为完整 session id；
/// 返回展示标题。
pub fn session_title(title: &Option<String>, id: &str) -> String {
    match title {
        Some(t) if !t.is_empty() => t.clone(),
        _ => format!("会话 {}", short_id(id)),
    }
}

// 单元测试：把「手工构造的快照」喂给 apply_snapshot，断言映射出的行/统计/状态，
// 并覆盖时间与 token 的纯格式化边界（这是 UI 侧唯一可离屏验证的部分，不依赖 runtime）。
