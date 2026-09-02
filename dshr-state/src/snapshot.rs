//! 内存会话快照（数据管道 s1 的折叠产物）：纯数据结构 + 无逻辑，全部 pub。
//!
//! 语义对照 dshr-ui/src/model.rs 的 MsgKind/MsgView/ToolView/ChatState（UI 消费意图），
//! 但**不 import dshr-ui**（依赖方向 ui → state，禁止反向）。本模块只依赖
//! dsh-sdk-protocol 的 TokenUsage / SessionStatus 两个共享类型。
//! s2 落库时按 DESIGN §11.2 事实表从本快照取数即可，两路同源。
use dsh_sdk_protocol::llm::TokenUsage;
use dsh_sdk_protocol::notifications::SessionStatus;

/// 消息行种类（渲染形态；对照 dshr-ui model.rs 的 MsgKind）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    /// 用户消息（source.kind=user 的人类输入；全宽行）。
    User,
    /// 助手消息（markdown 渲染；正文 + 可选思考 + usage）。
    Assistant,
    /// 只有思考没有正文的助手产出（灰字折叠行；思考文本在 `reasoning` 字段）。
    Reasoning,
    /// 工具调用卡片（call ↔ result 配对后的内容在 `tool` 字段）。
    Tool,
    /// 系统/上下文一行小字（compaction 等；一行简述在 `text`）。
    Notice,
}

/// 消息流里的一行（无 Default：kind 无自然缺省，由 fold 显式构造）。
#[derive(Debug, Clone, PartialEq)]
pub struct MsgItem {
    pub kind: MsgKind,
    /// User/Assistant 的正文（markdown 源）；Notice 时是一行简述；Tool 行为空。
    pub text: String,
    /// Assistant/Reasoning 行的思考文本（content 里 type=reasoning 各块按事件序合并；无则 None）。
    pub reasoning: Option<String>,
    /// Assistant 消息的 token 账目（来自 assistant/message data.usage；Reasoning 行同源携带）。
    pub usage: Option<TokenUsage>,
    /// Tool 行的卡片内容。
    pub tool: Option<ToolItem>,
    /// 事件 time（dsh 侧 epoch ms，信封印章；Tool 行 = tool/call 的时刻）。
    pub time: u64,
    /// 事件 seq（稳定排序/增量书签）。
    pub seq: u64,
}

/// 工具调用卡片（对照 dshr-ui model.rs 的 ToolView 并扩展 result/diffs）。
/// call 与 result 按 call_id 配对：result 未到 = 挂起态（result None / is_error false / duration 0）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolItem {
    pub call_id: String,
    pub name: String,
    /// 模型产出的原始 arguments JSON 的截断摘要（≤300 字符；全文在 wire log）。
    pub arguments: String,
    /// tool/result.time − tool/call.time；差值不可靠（回放/时钟错乱）时 saturating 归 0。
    pub duration_ms: u64,
    /// tool/result 的 data.error 存在或内容块 isError=true（挂起态 = false，未知）。
    pub is_error: bool,
    /// 结果首文本块截断摘要（≤300 字符）；挂起态 None。
    pub result: Option<String>,
    /// 自 tool/result data.meta.diffs 折叠的逐文件行数（s2 file_ops 事实表的数据源）。
    pub diffs: Vec<FileDiff>,
}

/// 一个文件变更的行数摘要（自 meta.diffs 的 {path, oldText, newText} 折叠）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    pub path: String,
    /// newText 的行数（null/缺失 = 0）。
    pub added: u64,
    /// oldText 的行数（null/缺失 = 0）。
    pub removed: u64,
}

/// token 六桶累计（DESIGN §11.2 turns 表六列 / §11.3 六桶语义）。
/// 注意计数不相交：input 不含缓存，计费 = input + cache_read + cache_write；
/// total 只在 adapter 报权威 totalTokens 时入账，缺省该桶为 0。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsageAgg {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub total: u64,
}

impl UsageAgg {
    /// 并入一次 assistant/message 的 usage；adapter 未报的可选桶按 0 计。
    pub fn add(&mut self, u: &TokenUsage) {
        self.input += u.input_tokens;
        self.output += u.output_tokens;
        self.total += u.total_tokens.unwrap_or(0);
        self.cache_read += u.cache_read_tokens.unwrap_or(0);
        self.cache_write += u.cache_write_tokens.unwrap_or(0);
        self.reasoning += u.reasoning_tokens.unwrap_or(0);
    }
}

/// 一轮 turn 的统计（turn/start 打开、turn/end 结算；截断日志里未结算轮 end/reason 为 None）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TurnStat {
    pub turn: u64,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    /// 结束原因一行（"completed"/"aborted/user"/"error/<code>: <message>"/…）；未结算 None。
    pub reason: Option<String>,
    /// 该轮内各 assistant/message 的 token 六桶合计。
    pub usage: UsageAgg,
}

/// 会话级汇总（DESIGN §11.3 会话层）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionStats {
    pub turns: u64,
    /// step/start 事件数（按 start 计，截断日志下比 end 侧稳）。
    pub steps: u64,
    /// 消息行数（仅 User/Assistant 行；Reasoning/Tool/Notice 行不占——它们不是对话消息）。
    pub messages: u64,
    /// tool/call 事件数（含 result 未到的挂起调用）。
    pub tool_calls: u64,
    /// 全部 assistant/message 的 token 六桶合计。
    pub usage: UsageAgg,
    /// LLM 耗时毫秒：s1 恒 0——单靠事件无可靠起止对（assistant/message 无配对计时），
    /// s2 落库后用 step/request 配对/精确计时校准（DESIGN §11.3 请求层）。
    pub llm_ms: u64,
    /// 工具总耗时毫秒：s1 恒 0——事件时间差不可靠（重放/时钟），s2 落库后校准。
    pub tool_ms: u64,
    /// 错误计数：tool/result is_error + turn/end reason=error（tool 未配对不重复计）。
    pub errors: u64,
}

/// 单个会话的内存快照（消息流 + 轮统计 + 汇总；会话树/跨会话聚合在 s2 目录层）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionSnapshot {
    /// 会话 id（来自 session.event / session.status 通知的 sessionId；WireLog 回放自动带出）。
    pub session_id: String,
    /// 会话标题（session/title 最后写入者胜）。
    pub title: Option<String>,
    /// 代理生命周期状态 idle/running（来自 session.status 通知；未收到 = None）。
    pub status: Option<SessionStatus>,
    /// 消息流（事件序；Tool 行已在原地与 result 配对）。
    pub messages: Vec<MsgItem>,
    /// 轮统计（已结算轮 + 进行中轮，进行中轮 end_time/reason 为 None）。
    pub turns: Vec<TurnStat>,
    pub stats: SessionStats,
}
