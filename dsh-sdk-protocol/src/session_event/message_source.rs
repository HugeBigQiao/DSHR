//! 消息来源（MessageSource）：一条消息是谁生产的。
//!
//! 主要用途：给出 `MessageSource` 判别联合（基座 4 kind + 十余个插件扩展 kind）及其
//! 辅助类型（技能目录条目、指令变更、会话引用等），是「这条消息为什么出现在上下文里」的答案。
//! 为什么需要：这是全 crate 扩展 kind **最多**的类型（十余个包各自 `declare module` 注册），
//! 且它与事件级兜底相互作用——未知 kind 会让整个 user/message 事件降级 `Unknown`，
//! 因此「宽容策略」必须写在文件级；单列文件也是为了把逐 kind 的官方行号钉法集中存放。
//! 上接：`session_event/message.rs::Message.source`（经 `pub use` 再出口给全 crate）；
//!       `session_event/fallback.rs` 的 `known()`（未知 kind → 事件降级 Unknown）；
//!       `dshr-state` 的 fold（来源标识/上下文分组）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：见下方「官方基座」与各扩展 kind 的逐行钉法（能钉到文件+行号）。
//!
//! 官方基座：`packages/llm/llm/src/message.ts` 的 MessageSourceMap 与 ContextFormed
//!（form 载荷并入各 kind）；官方类型 = `MessageSourceMap[keyof]`，**merge-extensible**——
//! 每个生产者在自己的包里 `declare module '@deepseek-ai/dsh-llm'` 注册自己的 kind
//!（没有统一的 catch-all `plugin` 那种设计，注释里明写过）。
//! ⚠️ 上面那句「基座 = user/plugin/model/tool」是**旧版**（0.1.2 时代）的形状；
//! 0.1.7-rc.2 起基座与扩展 kind 混在各包声明里，**权威判据是真实帧**：
//! 2026-09-29 扫全部 `data/wire-logs/*.jsonl`（44 个文件、149 条会话事件）实测出现的 kind
//! 只有 4 种：`model`(39) / `user`(5) / `system-prompt`(5) / `runtime-context`(5)。
//! 其中后两种当时**没有建模**，代价是 `system/message` 与这类 `user/message` **整条降级
//! `Unknown`**（由 `dsh-sdk-protocol/tests/frame_shape.rs` 的真实帧对账抓出）。
//! 本文件 port 的扩展 kind（官方引用逐个钉到文件）：
//!   system-prompt → packages/core/agent-loop 的系统提示投影（真实帧形状 `{kind:'system-prompt'}`）
//!   runtime-context → packages/core/agent-loop/src/runtime-context.ts 的 declare module
//!                     （`{ kind:'runtime-context' } & ContextFormed`；真实帧带 form:'snapshot' + sections）
//!   goal → packages/goal/goal/src/domain.ts 的 GoalMessageSource（L46-59）
//!   user-rpc → packages/api/session-controller/src/types.ts 的 declare module（L364-369）——
//!              注意 kind 仍是 'user'，只加 rpcId/clientTimeZone? 字段
//!   webhook → packages/webhook/webhook/src/types.ts（L71-83，内联对象 + form:'notice'）
//!   skill-catalog → packages/skill/tool-skill/src/index.ts 的 SkillCatalogSource（L29-47）
//!   skill-invocation → packages/skill/skill/src/index.ts 的 SkillInvocationSource（L141-161）
//!   agent-instructions → packages/context/agent-instructions/src/state.ts 的 AgentInstructionSource
//!                        （L34-52）与 render.ts 的 AgentInstructionChange（L46-52）
//!   session-reference → packages/context/session-reference/src/types.ts 的 SessionReferenceSource（L12-36）
//!   agent-message / subagent-settled → packages/subagent/subagent/src/continuation.ts（L58-89）
//!   team-message → packages/experimental/agent-team/src/types.ts 的 TeamMessageSource（L114-127）
//! 已建模 kind 的官方形状：user = {kind:'user'}；plugin = {kind:'plugin', plugin} & ContextFormed；
//! model = {kind:'model', provider, model, replayState?}（message.ts 的 ModelMessageSource L23-26）；
//! tool = {kind:'tool', callId}。
//!
//! **已知的覆盖缺口（待用户拍板，见 AI-LOG「消息来源 kind 的覆盖缺口」）**：
//! `packages/session/session-format-v3-to-v4/src/sources.ts` 列出的生产者 kind 远多于此文件建模的
//! 13 种（至少还有 compact-checkpoint / compact-basic / ptc-mode / coordinator / subagent-report /
//! model-selection / plan-mode / time-context / tmux-context / user-approval / repeat-tool-reminder …）。
//! 那些插件一旦参与，含其 source 的消息就会整条降级。**别在这里逐个补**——先在真实日志里确认
//! 它真的出现（同 2026-09-29 的做法），再决定是补变体还是加 catch-all。
//!
//! 宽容策略（对齐官方 merge-extensible，勿加 deny_unknown_fields）：未知 kind → 含该 source 的
//! 消息事件走 fallback.rs 的 known() 降级 Unknown（lossless，与同步前行为一致）；
//! 已知 kind 的新增字段自动忽略。官方把固定字面量 form 写进 source 对象，这里用
//! MessageSourceForm 枚举如实保留（form 本身有 `Other` 兜底——官方文档写明未知 form 是合法默认形态）。
use serde::{Deserialize, Serialize};

/// 官方 ContextFormed 的 form 值（语义词汇，见 message.ts L50-62）。
/// 用在各扩展 kind 的 form 字段（官方各 declare module 把 form 固定成单个字面量）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageSourceForm {
    Instructions,
    Catalog,
    /// 当前状态快照：同生产者的后续快照**覆盖**前一个（官方 ContextForm.snapshot，
    /// 带 `sections`）。真实会话实测：`runtime-context` 的 source 即 `form:'snapshot'`。
    Snapshot,
    Notice,
    Relay,
    Recall,
    /// 兜底：官方 ContextForm 的文档写明「缺省或未知值即默认形态，按不透明内容呈现」——
    /// 所以未知 form 不该让整条消息降级（与 `MessageRole`/`TurnEndReason` 同款策略）。
    #[serde(other)]
    Other,
}

/// 消息来源（谁生产的这条消息；wire 上是 {kind:...} 对象）。
/// 官方：packages/llm/llm/src/message.ts 的 MessageSourceMap + 各插件 declare module
/// 用在 Message.source。
/// 简化：plugin 的 ContextFormed（form/sections/summary 等）仍不 port，反序列化自动忽略；
/// 各扩展 kind 的 form 已结构化（见各变体）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MessageSource {
    /// kind 'user'：普通用户消息；user-rpc（浏览器 prompt 入队）也只加字段不改 kind——
    /// 官方：packages/api/session-controller/src/types.ts 的 declare module（L364-369）
    #[serde(rename_all = "camelCase")]
    User {
        /// user-rpc 的入队回执 id（官方 branded SessionRequestId）；普通用户消息无此字段。
        #[serde(skip_serializing_if = "Option::is_none")]
        rpc_id: Option<String>,
        /// user-rpc 的客户端时区（官方 clientTimeZone?）。
        #[serde(skip_serializing_if = "Option::is_none")]
        client_time_zone: Option<String>,
    },
    Plugin {
        plugin: String,
    },
    /// kind 'model'：模型产出的助手消息（官方 ModelMessageSource，message.ts L23-26）。
    /// 用在 assistant/message 的 source。
    #[serde(rename_all = "camelCase")]
    Model {
        provider: String,
        model: String,
        /// adapter 私有回放状态（官方 AssistantProvenance.replayState?，lossless JSON），
        /// 仅目标 adapter 同进程持有源与目标 provider 时使用。
        #[serde(skip_serializing_if = "Option::is_none")]
        replay_state: Option<serde_json::Value>,
    },
    /// kind 'system-prompt'：渲染后的系统提示消息（`system/message` 的来源，wire 上就是
    /// `{ kind: 'system-prompt' }`）。
    ///
    /// 官方 0.1.7-rc.2 就有：`packages/core/agent-loop/tests/system-prompt-projection.spec.ts`
    /// 断言该消息的 source 恒为 `{ kind: 'system-prompt' }`。
    /// **本项目此前漏移植**——代价是**每条** `system/message` 整条降级 `Unknown`
    ///（2026-09-29 由 `tests/frame_shape.rs` 的真实帧形状对账抓出来：5/5 条 system/message 全降级）。
    ///
    /// 为什么写成「空 struct variant」而不是 unit variant：serde 的 internally-tagged 枚举
    /// 对 **unit** 变体要求剩余字段为空，官方一旦给该来源加字段就会整体解析失败（又一条整消息降级）；
    /// struct variant 默认忽略未知字段，天然抗漂移。
    SystemPrompt {},
    #[serde(rename_all = "camelCase")]
    Tool {
        /// 官方 branded CallId，先用 String。
        call_id: String,
    },
    /// kind 'runtime-context'：DSH 自己注入的**运行时事实**（沙箱策略、审批策略等快照）。
    ///
    /// 官方：`packages/core/agent-loop/src/runtime-context.ts` 的 `declare module`
    ///（`'runtime-context': { kind: 'runtime-context' } & ContextFormed`）。
    /// 真实会话实测形状（0.1.7-rc.2，5/5 条）：
    /// `{ kind:'runtime-context', form:'snapshot', sections:[{name,text},…] }`。
    ///
    /// **本项目此前漏移植**——代价是**每条**这类 `user/message`（role=user）整条降级 `Unknown`。
    /// 为什么两个字段都按可选处理：官方 `ContextFormed` 的 `form` 本身可省略（省略 = 未声明的上下文），
    /// 强制要求会让「只带 kind」的帧整体解析失败；`sections` 同样只在 `form:'snapshot'` 时出现。
    #[serde(rename_all = "camelCase")]
    RuntimeContext {
        #[serde(skip_serializing_if = "Option::is_none")]
        form: Option<MessageSourceForm>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sections: Option<Vec<ContextSnapshotSection>>,
    },
    /// kind 'plan-mode'：计划模式切换时注入的说明（官方 `dsh-plan-mode`）。
    ///
    /// 形状实证（2026-09-29 扫已安装 runtime）：
    /// `packages/dsh-plan-mode/lib/types/index.js` 的
    /// `source: { kind: 'plan-mode', form: 'notice', summary: text }`。
    /// 字段按可选处理：它属官方 `ContextFormed`（`form` 可省略），
    /// 一处形状漂移不该吃掉整条消息（同 `RuntimeContext`）。
    #[serde(rename_all = "camelCase")]
    PlanMode {
        #[serde(skip_serializing_if = "Option::is_none")]
        form: Option<MessageSourceForm>,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// kind 'model-selection'：模型切换的通知类上下文（官方 `dsh-agent`）。
    ///
    /// 形状实证：`packages/dsh-agent/lib/types/model-selection.js` 的
    /// `{ kind: 'model-selection', form: 'notice', summary: boundContextSummary(...) }`。
    /// 合并声明的 `MessageSourceMap` 里也钉着 `{ kind: 'model-selection' } & ContextFormed`。
    #[serde(rename_all = "camelCase")]
    ModelSelection {
        #[serde(skip_serializing_if = "Option::is_none")]
        form: Option<MessageSourceForm>,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// kind 'user-approval'：审批等待期间的注入内容（官方 `dsh-user-approval`）。
    ///
    /// 形状实证：`packages/dsh-user-approval/lib/types/index.js` 的 `source: { kind: 'user-approval' }`
    /// ——wire 上只有 kind 本身（声明为 `{ kind: 'user-approval' } & ContextFormed`，
    /// 但该生产者不写 form/summary）。
    UserApproval {},
    /// kind 'ptc-mode'：PTC（程序化工具调用）模式保留/透传的内容
    /// （官方 `dsh-tools` 与 `dsh-spill-policy`）。
    ///
    /// 形状实证：`additionalContexts.push(createUserMessage({ …, source: { kind: 'ptc-mode' } }))`。
    PtcMode {},
    /// kind 'compact-checkpoint'：一次压缩事务的检查点来源（官方 `dsh-compaction`）。
    ///
    /// 形状实证：`packages/dsh-compaction/lib/types/checkpoint.js` 的
    /// `compactCheckpointSource(compactionId, sourceCommandId)` ——
    /// `{ kind: 'compact-checkpoint', compactionId, sourceCommandId? }`（唯一的构造工厂，
    /// 所以 `compaction_id` 按必填处理）。
    #[serde(rename_all = "camelCase")]
    CompactCheckpoint {
        /// 所属压缩事务的身份。
        compaction_id: String,
        /// 触发这次压缩的手工命令 id（仅手工压缩时有）。
        #[serde(skip_serializing_if = "Option::is_none")]
        source_command_id: Option<String>,
    },
    /// 目标续跑轮次的消息归属（goal 域写入，官方 GoalMessageSource）。
    /// 用在 goal 轮开跑时注入的继续消息。
    #[serde(rename_all = "camelCase")]
    Goal {
        goal_id: String,
        /// 每次目标变更 +1。
        revision: u64,
        /// 已接纳的续跑轮号（≥1）。
        round: u64,
    },
    /// webhook 规则准入的程序化输入（官方 webhook/src/types.ts L71-83 内联对象）。
    #[serde(rename_all = "camelCase")]
    Webhook {
        provider: String,
        source: String,
        delivery_id: String,
        rule_id: String,
        form: MessageSourceForm,
        /// 一行摘要（官方 notice 必带 summary）。
        summary: String,
    },
    /// 会话技能目录发布（官方 SkillCatalogSource，tool-skill/src/index.ts L29-47）。
    /// 目录每次发布替换上一版（catalog-form context）。
    #[serde(rename_all = "camelCase")]
    SkillCatalog {
        form: MessageSourceForm,
        /// 替换发布标记（官方 update?: true，仅非首次出现）。
        #[serde(skip_serializing_if = "Option::is_none")]
        update: Option<bool>,
        entries: Vec<SkillCatalogEntry>,
    },
    /// 用户显式调用技能时注入的指令上下文（官方 SkillInvocationSource，L141-161）。
    #[serde(rename_all = "camelCase")]
    SkillInvocation {
        name: String,
        form: MessageSourceForm,
    },
    /// 工作区指令上下文（官方 AgentInstructionSource，state.ts L34-52）。
    #[serde(rename_all = "camelCase")]
    AgentInstructions {
        form: MessageSourceForm,
        /// 完整启动/恢复基线标记（官方 baseline?: true，非后续增量）。
        #[serde(skip_serializing_if = "Option::is_none")]
        baseline: Option<bool>,
        /// 恢复校验用的发现/优先级/预算标识（官方 baselineIdentity?）。
        #[serde(skip_serializing_if = "Option::is_none")]
        baseline_identity: Option<String>,
        changes: Vec<InstructionChange>,
    },
    /// 跨会话引用上下文（官方 SessionReferenceSource，session-reference/src/types.ts L12-36）。
    #[serde(rename_all = "camelCase")]
    SessionReference {
        form: MessageSourceForm,
        /// 官方字面量 1。
        version: u32,
        references: Vec<SessionReferenceItem>,
    },
    /// 相邻 Agent 之间模型互发的一条消息（官方 AgentMessageSource，continuation.ts L58-65）。
    #[serde(rename_all = "camelCase")]
    AgentMessage {
        form: MessageSourceForm,
        /// 发消息那方 Agent 的 Session id。
        sender_session_id: String,
    },
    /// runtime 对可冷恢复子代理结局的自述（官方 SubagentSettledMessageSource，L74-82）。
    #[serde(rename_all = "camelCase")]
    SubagentSettled {
        form: MessageSourceForm,
        /// 一行结局摘要（官方 notice 必带 summary）。
        summary: String,
        /// 结算的那个子会话 id。
        sender_session_id: String,
    },
    /// 团队成员发给本会话的邮箱消息（官方 TeamMessageSource，agent-team/src/types.ts L114-127）。
    #[serde(rename_all = "camelCase")]
    TeamMessage {
        team_id: String,
        message_id: String,
        sender_id: String,
        sender_name: String,
    },
}

/// 一个 `form:'snapshot'` 上下文里的具名贡献（官方 ContextSnapshotSection）。
/// 官方：packages/llm/llm/src/message.ts 的 ContextSnapshotSection（`{name, text}`，按装配顺序）。
/// 用在 `MessageSource::RuntimeContext.sections`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSnapshotSection {
    /// 贡献该段的子系统名（真实帧示例：`sandbox:policy` / `approval:policy`）。
    pub name: String,
    /// 该段**面向模型的文本**（官方注释：exactly as assembled）。
    pub text: String,
}

/// 技能目录条目（官方 SkillCatalogSource['entries'] 元素，L40：{name, description}）。
/// 用在 MessageSource::SkillCatalog.entries。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillCatalogEntry {
    pub name: String,
    pub description: String,
}

/// 指令变更动作（官方 AgentInstructionChange.action：'set' | 'replace' | 'remove'）。
/// 用在 InstructionChange.action。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstructionChangeAction {
    Set,
    Replace,
    Remove,
}

/// 一条指令文件变更（官方 AgentInstructionChange，agent-instructions/src/render.ts L46-52）。
/// 用在 MessageSource::AgentInstructions.changes。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionChange {
    pub action: InstructionChangeAction,
    /// 逻辑指令作用域键（'user-global' | '.' | 项目相对目录）。
    pub scope: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// 一条被引用会话的记录（官方 SessionReferenceSource.references 元素，types.ts L18-29）。
/// 用在 MessageSource::SessionReference.references。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReferenceItem {
    pub session_id: String,
    pub label: String,
    /// 官方 OptionalSessionSeq（number | null；null = 引到日志末尾），先按 Option 处理。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_through_seq: Option<u64>,
    pub compacted: bool,
    pub original_messages: u64,
    pub retained_messages: u64,
    pub omitted_messages: u64,
    pub omitted_bytes: u64,
    pub truncated: bool,
    pub input_index: u64,
}
