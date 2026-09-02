//! 视图模型：UI 消费的数据形状（纯数据 + Clone，渲染层只 import 本模块）。
//!
//! s3 升级（DESIGN §11.4 / 里程碑 M3.6）：不再由占位桥喂假数据，而是把
//! `dshr_state::fold::Folder` 的折叠快照（snapshot.rs 纯数据）映射成这里的形状。
//! 依赖方向 ui → state：本模块可 import dshr_state::snapshot（共享纯数据层，
//! 它不 import 本 crate）；UI 其它文件不直接碰 SDK/协议类型。snapshot 整体替换式
//! 刷新（worker 每事件发一次快照，消息行/统计在映射处重建；增量传输留后续）。

use dsh_sdk_protocol::notifications::SessionStatus;
use dshr_state::snapshot::{MsgItem, SessionSnapshot, ToolItem};

// 消息种类直接复用 state 快照的种类（User/Assistant/Reasoning/Tool/Notice 一一对应）。
pub use dshr_state::snapshot::MsgKind;

/// 对话/运行生命周期状态（由 worker 事件 + 快照 session.status 推导）。
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
#[derive(Debug, Clone)]
pub struct SessionView {
    pub id: String,
    pub title: String,
}

/// 一个 runtime（= 一个 dsh 子进程）。s3 收敛为单 runtime：vec 恒 ≤1 项，
/// 多 runtime 管理（每 runtime 一个进程/会话树）留后续。
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
    fn from_usage(u: &dsh_sdk_protocol::llm::TokenUsage) -> Self {
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
    /// Tool 行的卡片内容（快照 call↔result 配对后的 ToolItem：name/call_id/duration/
    /// is_error/result/diffs）。
    pub tool: Option<ToolItem>,
    /// 时间标签：由事件 time(epoch ms) 格式化（UTC HH:mm；本地化待办，见 hhmm_utc）。
    pub time_label: String,
    /// 事件 seq（稳定序；工具卡展开状态按它索引——UI 侧 expanded 集合放 App）。
    pub seq: u64,
}

impl MsgView {
    fn from_item(item: &MsgItem) -> Self {
        Self {
            kind: item.kind,
            text: item.text.clone(),
            reasoning: item.reasoning.clone(),
            usage: item.usage.as_ref().map(TokenCounts::from_usage),
            tool: item.tool.clone(),
            time_label: hhmm_utc(item.time),
            seq: item.seq,
        }
    }
}

/// 会话级统计（由快照 SessionStats 映射；渲染在 chat.rs 按需格式化）。
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
    pub fn new() -> Self {
        Self::default()
    }

    /// 以一份会话快照整体替换视图内容（worker 每事件折叠后发送）。
    /// 会话生命周期状态从快照 session.status 映射：Running → running；
    /// Idle/None（未收到状态）→ idle。
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
#[derive(Debug, Clone, Default)]
pub struct AppData {
    /// runtime 树（s3 收敛：至多一个 runtime + 一个当前会话）。
    pub runtimes: Vec<RuntimeView>,
    pub chat: ChatState,
}

// —— 纯格式化（不引 chrono：整除直接算，输出 UTC；本地化待办见注释）——

/// epoch 毫秒 → "HH:mm"（UTC）。不依赖系统时区/chrono：先按 UTC 折算并注释
/// 「本地化待办」（桌面端未来接 iced_time/chrono-tz 时换成本地时区）。
pub fn hhmm_utc(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{h:02}:{m:02}")
}

/// token 数简短显示：≥1000 → "1.2k"（统计行/工具卡用）。
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// 统计行文本（对话区底部）：轮 · 步 · 消息 · in/out tokens · 错误。
/// 缓存/reasoning 桶不入行（监控页 s4 展示），字段仍保留在 ChatStats。
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
pub fn session_title(title: &Option<String>, id: &str) -> String {
    match title {
        Some(t) if !t.is_empty() => t.clone(),
        _ => format!("会话 {}", short_id(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsh_sdk_protocol::notifications::SessionStatus;
    use dshr_state::snapshot::{FileDiff, MsgKind as SK, SessionSnapshot, SessionStats, UsageAgg};

    /// 手工造快照（快照是纯数据，直接构造）。
    fn snap_with(
        messages: Vec<MsgItem>,
        status: Option<SessionStatus>,
        title: Option<String>,
    ) -> SessionSnapshot {
        SessionSnapshot {
            session_id: "s-42".to_string(),
            title,
            status,
            messages,
            turns: vec![],
            stats: SessionStats {
                turns: 2,
                steps: 4,
                messages: 1,
                tool_calls: 1,
                usage: UsageAgg {
                    input: 100,
                    output: 30,
                    cache_read: 5,
                    cache_write: 2,
                    reasoning: 8,
                    total: 140,
                },
                llm_ms: 0,
                tool_ms: 0,
                errors: 1,
            },
        }
    }

    fn user_item(seq: u64) -> MsgItem {
        MsgItem {
            kind: SK::User,
            text: "第一问".to_string(),
            reasoning: None,
            usage: None,
            tool: None,
            // 2024-01-02T03:04:05Z
            time: 1704164645000,
            seq,
        }
    }

    #[test]
    fn apply_snapshot_maps_rows_stats_and_status() {
        let tool_item = ToolItem {
            call_id: "c1".to_string(),
            name: "bash".to_string(),
            arguments: "{}".to_string(),
            duration_ms: 50,
            is_error: false,
            result: Some("ok".to_string()),
            diffs: vec![FileDiff {
                path: "a.rs".to_string(),
                added: 2,
                removed: 1,
            }],
        };
        let snap = snap_with(
            vec![
                user_item(1),
                MsgItem {
                    kind: SK::Tool,
                    text: String::new(),
                    reasoning: None,
                    usage: None,
                    tool: Some(tool_item),
                    time: 1704164645000,
                    seq: 2,
                },
            ],
            Some(SessionStatus::Running),
            Some("标题".to_string()),
        );
        let mut chat = ChatState::new();
        chat.apply_snapshot(&snap);
        assert_eq!(chat.session_id, "s-42");
        assert_eq!(chat.status, ChatStatus::Running);
        assert_eq!(chat.title.as_deref(), Some("标题"));
        assert_eq!(chat.messages.len(), 2);
        assert_eq!(chat.messages[0].kind, MsgKind::User);
        assert_eq!(chat.messages[0].text, "第一问");
        // time → UTC HH:mm（03:04:05 → 03:04）
        assert_eq!(chat.messages[0].time_label, "03:04");
        let tool = chat.messages[1].tool.as_ref().expect("tool 行");
        assert_eq!(tool.name, "bash");
        assert_eq!(tool.duration_ms, 50);
        assert_eq!(tool.diffs.len(), 1);
        assert_eq!(chat.stats.turns, 2);
        assert_eq!(chat.stats.steps, 4);
        assert_eq!(chat.stats.usage.input, 100);
        assert_eq!(chat.stats.usage.total, 140);
        assert_eq!(chat.stats.errors, 1);
        assert_eq!(chat.stats.tool_calls, 1);
    }

    #[test]
    fn status_none_maps_to_idle_and_idle_stays_idle() {
        for status in [None, Some(SessionStatus::Idle)] {
            let mut chat = ChatState::new();
            chat.apply_snapshot(&snap_with(vec![user_item(1)], status, None));
            assert_eq!(chat.status, ChatStatus::Idle);
        }
    }

    #[test]
    fn hhmm_boundaries_are_utc() {
        assert_eq!(hhmm_utc(0), "00:00");
        assert_eq!(hhmm_utc(23 * 3600_000 + 59 * 60_000 + 999), "23:59");
        assert_eq!(hhmm_utc(25 * 3600_000), "01:00"); // 跨天回绕
    }

    #[test]
    fn fmt_tokens_and_stats_line() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1234), "1.2k");
        let line = stats_line(&ChatStats {
            turns: 2,
            steps: 4,
            messages: 1,
            tool_calls: 1,
            usage: TokenCounts {
                input: 1000,
                output: 30,
                ..TokenCounts::default()
            },
            errors: 1,
        });
        assert!(line.contains("2 轮"), "{line}");
        assert!(line.contains("1.0k"), "{line}");
        assert!(line.contains("err 1"), "{line}");
    }

    #[test]
    fn session_title_falls_back_to_short_id() {
        assert_eq!(session_title(&None, "s-1234567890123"), "会话 …890123");
        assert_eq!(session_title(&Some("你好".to_string()), "s-1"), "你好");
        // 短 id（不足 6 位）全显。
        assert_eq!(session_title(&None, "s-12"), "会话 12");
    }

    #[test]
    fn chat_state_initial_is_off_with_hint() {
        let chat = ChatState::new();
        assert_eq!(chat.status, ChatStatus::Off);
        assert!(chat.status_line.contains("新建 runtime"));
        assert_eq!(ChatStatus::Running.label(), "running");
    }
}
