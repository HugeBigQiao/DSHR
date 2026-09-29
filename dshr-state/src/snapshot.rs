//! 内存会话快照：fold 的折叠产物，纯数据结构 + 无逻辑、字段全 `pub`。
//!
//! 主要用途：UI 展示的唯一数据形状（经 `dshr-ui/src/model.rs` 映射成视图模型）；
//! 同时是落库的输入（`store::Store::persist_snapshot` 的参数）。
//! 为什么需要：它是 fold 与 UI / store 之间的**稳定契约**——只要这个形状不变，
//! 协议层加事件、落库加表都不影响 UI 渲染代码。
//! 依赖方向：**只出不进**——本模块不 import `dshr-ui`（依赖方向恒为 `ui → state`，禁止反向），
//! 只依赖 `dsh_sdk_protocol` 的 `TokenUsage` / `SessionStatus` 两个共享类型。
//! 上接：`fold.rs`（产出快照）、`raw.rs`（`EngineEvent::Snapshot` 携带它）、`store.rs`（落库输入）、
//!       `dshr-ui/src/model.rs`（映射成视图模型）。
//! 下接：`dsh_sdk_protocol::llm::TokenUsage`、`dsh_sdk_protocol::notifications::SessionStatus`。
//! 官方对应：无（官方 wire 上没有这个形状；它是 dshr 为 UI 定义的投影）。
//!
//! 注：语义上对照 `dshr-ui/src/model.rs` 的 `MsgKind` / `MsgView` / `ToolView` / `ChatState`
//! （UI 的消费意图），但两者刻意分开：本文件是「数据」，`model.rs` 是「视图」。
use dsh_sdk_protocol::llm::TokenUsage;
use dsh_sdk_protocol::notifications::SessionStatus;

/// 消息行种类（渲染形态；对照 dshr-ui model.rs 的 MsgKind）。
///
/// 注意「记录」与「显示」的分工（2026-09-29 用户要求「能记录多少就记多少」）：
/// fold 把**所有**消息都折成行（含程序化注入与未提交的尝试），**显示策略归 UI**
///（UI 可以只渲染 User/Assistant/Tool，其余折叠或隐藏）。这样落库与导出是全量的，
/// 而聊天视图仍保持干净。
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
    /// 系统/上下文一行小字（compaction / 重试等；一行简述在 `text`）。
    Notice,
    /// **程序化注入**的消息：`role=user 但 source.kind≠user`（runtime-context / plan-mode /
    /// goal / webhook / skill-* …）、`system/message`（系统提示）、`developer/message`（工具增删）。
    /// 为什么折成行：它们是「模型当时看到了什么」的事实，用户要求落库保真；UI 默认不显示。
    Injected,
    /// 一次**未提交 surface 消息**的模型尝试（`assistant/attempt`）。
    /// 它没有 message，但有流摘要与 turn/step——是「模型确实跑过、token 花过」的证据
    ///（中断/失败场景下往往只剩这个可查）。
    Attempt,
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
    /// v3 流记录摘要（来自 assistant/message data.stream；用于回放/延迟展示）。
    pub stream: Option<StreamSummary>,
    /// Tool 行的卡片内容。
    pub tool: Option<ToolItem>,
    /// 事件 time（dsh 侧 epoch ms，信封印章；Tool 行 = tool/call 的时刻）。
    pub time: u64,
    /// 事件 seq（稳定排序/增量书签）。
    pub seq: u64,
    /// 所属轮（事件 data.turn；本地合成行/无轮事件为 None）。落库时填 `messages.turn`。
    pub turn: Option<u64>,
    /// 所属步（step；Notice/Injected 行尽量沿用当前步，取不到为 None）。
    pub step: Option<u64>,
    /// 来源 kind 文本（`MessageSource` 的判别值：`user` / `model` / `tool` / `system-prompt` /
    /// `runtime-context` / …；非消息事件折叠的行为空串）。
    /// 为什么存文本而不是枚举：落库/CSV 是给人看的，文本比判别枚举稳定（枚举改名不炸数据）。
    pub source: String,
    /// 失败原因（工具失败 `tool/result` 的 error 文本、尝试失败、本地发送失败……）；成功为 None。
    pub error: Option<String>,
}

/// 一次 assistant 流记录的统计摘要（不保留逐 chunk 内容，避免快照重复克隆）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamSummary {
    /// 展开后的 chunk 总数。
    pub chunks: u64,
    /// 第一个 chunk 的时间戳（epoch ms）。
    pub first_time: Option<u64>,
    /// 最后一个 chunk 的时间戳。
    pub last_time: Option<u64>,
    /// 第一个 text/reasoning/tool-call 增量时间（近似首 token）。
    pub first_token_time: Option<u64>,
    /// text 增量的字符数。
    pub text_chars: u64,
    /// reasoning 增量的字符数。
    pub reasoning_chars: u64,
    /// tool-call 参数增量的字符数。
    pub tool_args_chars: u64,
}

/// 工具调用卡片（对照 dshr-ui model.rs 的 ToolView 并扩展 result/diffs）。
/// call 与 result 按 call_id 配对：result 未到 = 挂起态（result None / is_error false / duration 0）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolItem {
    pub call_id: String,
    pub name: String,
    /// 模型产出的原始 arguments JSON（**不再截断**；用户要求落库保真，
    /// 截断只该发生在渲染层。全文本来也另有一份在 wire log）。
    pub arguments: String,
    /// tool/result.time − tool/call.time；差值不可靠（回放/时钟错乱）时 saturating 归 0。
    pub duration_ms: u64,
    /// tool/result 的 data.error 存在或内容块 isError=true（挂起态 = false，未知）。
    pub is_error: bool,
    /// 工具失败的**原因文本**（`data.error.reason`，其次 `code: name`）；成功为 None。
    /// 为什么单独存：`is_error` 只说明「失败了」，排查需要的是原因（用户 2026-09-29 明确要求）。
    pub error: Option<String>,
    /// 结果首文本块（**不再截断**）；挂起态 None。
    pub result: Option<String>,
    /// 自 tool/result data.meta.diffs 折叠的逐文件行数（file_ops 事实表的数据源）。
    pub diffs: Vec<FileDiff>,
    /// `tool/result` 的 `meta` **原样 JSON**（如 fs 工具的完整 diff：含 oldText/newText 全文）。
    /// 为什么留着：行数摘要丢失正文，而「这次工具到底改了什么」只有原样 meta 能回答；
    /// 体积代价由 wire log 兜底（库里存的是同一份内容的加工副本，可随库整体删除重建）。
    pub meta: Option<serde_json::Value>,
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

/// token 六桶累计（DESIGN.md §8.2 / §8.3）。
/// 注意计数不相交：input 不含缓存，计费 = input + cache_read + cache_write；
/// total 只在 adapter 报权威 totalTokens 时入账，缺省该桶为 0。
///
/// serde 派生用于**复原**：`sessions.meta_json` 把会话级聚合原样存进库（见 store 的复原读），
/// 缺字段按默认值补（`#[serde(default)]`），将来加桶不会读不回老数据。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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

/// 会话级汇总（DESIGN.md §8.3）。
///
/// serde 派生用于复原（存 `sessions.meta_json`；缺字段按默认值补，见 [`UsageAgg`] 的说明）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    /// s2 落库后用 step/request 配对/精确计时校准（DESIGN.md §8.3）。
    pub llm_ms: u64,
    /// 工具总耗时毫秒：s1 恒 0——事件时间差不可靠（重放/时钟），s2 落库后校准。
    pub tool_ms: u64,
    /// 错误计数：tool/result is_error + turn/end reason=error（tool 未配对不重复计）。
    pub errors: u64,
}

/// 最近一次**模型请求**（`request/header`）＋最近一次**路由元数据**（`request/context`）。
///
/// 为什么需要它（2026-09-29 用户提出的产品问题）：发完 prompt 到首个模型产出之间可以安静
/// 几十秒没有任何事件，而 engine 只在**快照有变化**时发事件——于是 UI 上「点了发送之后一片
/// 静止」，用户无法区分「在等模型」与「卡住了」。这两个事件本来就是「模型请求已发起」的
/// 权威事实（官方 `Agent.buildRequest()` 每次请求都发），折进快照后**等待本身也会让快照变化**，
/// 脏检测于是会发快照，UI 才有东西可显示。
///
/// 字段全为 `Option`（缺省 = 该事实还没出现），所以一个 `Default` 值就表示「尚未发起过请求」。
///
/// serde 派生用于复原（存 `sessions.meta_json`；provider/model 是会话级事实，重启后也想知道）。
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RequestView {
    /// `request/header` 的 time（发起时刻，epoch ms）——UI 用它算「已等待 N 秒」。
    /// `None` = 只收到过 `request/context`（路由元数据），还没发起请求。
    pub started_at: Option<u64>,
    /// `request/header` 的 seq。与最后一条 assistant 消息的 seq 比较即可判断
    /// 「这次请求是否已经有产出」（有产出就不该再显示"等待中"）。
    pub seq: Option<u64>,
    /// 本次请求头的追加原因（`initial`/`resume`/`change`/`series`/`other`）。
    pub reason: Option<String>,
    /// 本次请求头携带的工具数（`header.tools` 长度；官方没给 = None）。
    pub tools: Option<usize>,
    /// 最近一次 `request/context` 的 provider。
    pub provider: Option<String>,
    /// 最近一次 `request/context` 的 model。
    pub model: Option<String>,
    /// 最近一次 `request/context` 的上下文窗口（token 数）。
    pub context_window: Option<u64>,
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
    /// 最近一次模型请求 / 路由元数据（见 [`RequestView`]；全 None = 尚未收到）。
    pub last_request: RequestView,
    /// plan 模式开关（`plan/mode` 覆盖式；缺省 false）。
    pub plan_mode: bool,
    /// 沙箱模式（`sandbox/mode` 覆盖式；wire 文本如 `workspace-write`，未收到 = None）。
    pub sandbox_mode: Option<String>,
}
