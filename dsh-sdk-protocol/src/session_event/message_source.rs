//! 消息来源（MessageSource）：一条消息是谁生产的。
//!
//! 官方基座：packages/llm/llm/src/message.ts 的 MessageSourceMap（L98-107：user/plugin/model/tool）
//! 与 ContextFormed（L81-96，form 载荷并入各 kind）；官方类型 = MessageSourceMap[keyof]（L127-128，
//! merge-extensible，插件扩展自己的 kind）。
//! 本文件 port 的扩展 kind（官方引用逐个钉到文件）：
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
//! 基座 4 kind 的官方形状：user = {kind:'user'}；plugin = {kind:'plugin', plugin} & ContextFormed；
//! model = {kind:'model', provider, model, replayState?}（message.ts 的 ModelMessageSource L23-26）；
//! tool = {kind:'tool', callId}。
//!
//! 宽容策略（对齐官方 merge-extensible，勿加 deny_unknown_fields）：未知 kind → 含该 source 的
//! user/message 事件走 fallback.rs 的 known() 降级 Unknown（lossless，与同步前行为一致）；
//! 已知 kind 的新增字段自动忽略。官方把固定字面量 form 写进 source 对象，这里用
//! MessageSourceForm 枚举如实保留（缺此字段的旧日志会降级 Unknown，不丢数据）。
use serde::{Deserialize, Serialize};

/// 官方 ContextFormed 的 form 值（语义词汇，见 message.ts L50-62）。
/// 用在各扩展 kind 的 form 字段（官方各 declare module 把 form 固定成单个字面量）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageSourceForm {
    Instructions,
    Catalog,
    Notice,
    Relay,
    Recall,
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
    #[serde(rename_all = "camelCase")]
    Tool {
        /// 官方 branded CallId，先用 String。
        call_id: String,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::session_event::SessionEvent;

    /// 官方形状的 goal 来源消息可结构化解析（goal 轮继续消息）。
    #[test]
    fn goal_source_message_parses_and_roundtrips() {
        let message = json!({
            "id": "m1",
            "role": "user",
            "content": [{"type": "text", "text": "继续"}],
            "source": {"kind": "goal", "goalId": "g-1", "revision": 2, "round": 1}
        });
        let msg: crate::session_event::message::Message =
            serde_json::from_value(message.clone()).expect("goal 来源消息应可解析");
        assert_eq!(
            msg.source,
            MessageSource::Goal {
                goal_id: "g-1".into(),
                revision: 2,
                round: 1
            }
        );
        // 再序列化应还原官方形状（含 goalId camelCase）。
        assert_eq!(serde_json::to_value(&msg).unwrap(), message);
    }

    /// kind 'model' 的 source 带 provider/model（官方 ModelMessageSource）。
    #[test]
    fn model_source_carries_provider_model() {
        let message = json!({
            "id": "m2",
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "source": {"kind": "model", "provider": "deepseek-official", "model": "deepseek-v4-flash"}
        });
        let msg: crate::session_event::message::Message =
            serde_json::from_value(message.clone()).expect("model 来源消息应可解析");
        match &msg.source {
            MessageSource::Model {
                provider,
                model,
                replay_state,
            } => {
                assert_eq!(provider, "deepseek-official");
                assert_eq!(model, "deepseek-v4-flash");
                assert_eq!(replay_state, &None);
            }
            other => panic!("应解析为 Model，实际 {other:?}"),
        }
        assert_eq!(serde_json::to_value(&msg).unwrap(), message);
    }

    /// user-rpc：kind 仍是 'user'，只加 rpcId/clientTimeZone 字段（官方 session-controller
    /// types.ts L364-369 的 declare module）——User 变体必须容忍额外字段。
    #[test]
    fn user_rpc_keeps_kind_user_with_extra_fields() {
        let message = json!({
            "id": "m3",
            "role": "user",
            "content": [{"type": "text", "text": "hello"}],
            "source": {"kind": "user", "rpcId": "rpc-9", "clientTimeZone": "Asia/Shanghai"}
        });
        let msg: crate::session_event::message::Message =
            serde_json::from_value(message.clone()).expect("user-rpc 来源消息应可解析");
        assert_eq!(
            msg.source,
            MessageSource::User {
                rpc_id: Some("rpc-9".into()),
                client_time_zone: Some("Asia/Shanghai".into()),
            }
        );
        assert_eq!(serde_json::to_value(&msg).unwrap(), message);
        // 普通 user 消息无这些字段 → 序列化仍只出 {"kind":"user"}。
        let plain = serde_json::from_value::<MessageSource>(json!({"kind": "user"})).unwrap();
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"kind": "user"})
        );
    }

    /// 各扩展 kind 官方形状逐一 roundtrip（解析 → 再序列化 = 原 JSON）。
    #[test]
    fn declared_source_kinds_roundtrip_official_shapes() {
        let fixtures = [
            json!({"kind": "webhook", "provider": "github", "source": "primary-github",
                   "deliveryId": "d1", "ruleId": "r1", "form": "notice", "summary": "PR opened"}),
            json!({"kind": "skill-catalog", "form": "catalog", "entries": [
                    {"name": "audit", "description": "逐文件审计"}]}),
            json!({"kind": "skill-catalog", "form": "catalog", "update": true, "entries": []}),
            json!({"kind": "skill-invocation", "name": "audit", "form": "instructions"}),
            json!({"kind": "agent-instructions", "form": "instructions", "baseline": true,
                   "baselineIdentity": "sha1:abc", "changes": [
                    {"action": "set", "scope": ".", "path": "AGENTS.md", "digest": "d"}]}),
            json!({"kind": "session-reference", "form": "recall", "version": 1, "references": [
                    {"sessionId": "s9", "label": "旧会话", "capturedThroughSeq": 42,
                     "compacted": false, "originalMessages": 10, "retainedMessages": 8,
                     "omittedMessages": 2, "omittedBytes": 512, "truncated": true,
                     "inputIndex": 0}]}),
            json!({"kind": "agent-message", "form": "relay", "senderSessionId": "s-parent"}),
            json!({"kind": "subagent-settled", "form": "notice", "summary": "child completed",
                   "senderSessionId": "s-child"}),
            json!({"kind": "team-message", "teamId": "s-root", "messageId": "tm-1",
                   "senderId": "s-a", "senderName": "alpha"}),
        ];
        for fixture in fixtures {
            let source: MessageSource = serde_json::from_value(fixture.clone())
                .unwrap_or_else(|e| panic!("解析失败 {fixture}: {e}"));
            assert_eq!(serde_json::to_value(&source).unwrap(), fixture);
        }
    }

    /// 未知新 kind：与同步前一致——该 user/message 事件降级 Unknown（lossless），不断言崩溃。
    #[test]
    fn unknown_source_kind_degrades_event_to_unknown() {
        let event: SessionEvent = serde_json::from_value(json!({
            "type": "user/message",
            "seq": 1,
            "time": 100,
            "data": {
                "id": "m-x",
                "role": "user",
                "content": [{"type": "text", "text": "hi"}],
                "source": {"kind": "future-plugin-kind", "x": 1}
            }
        }))
        .expect("未知 source kind 应降级 Unknown 而非报错");
        match event {
            SessionEvent::Unknown {
                event_type,
                data,
                seq,
                ..
            } => {
                assert_eq!(event_type, "user/message");
                assert_eq!(seq, 1);
                assert_eq!(
                    data,
                    json!({"id": "m-x", "role": "user", "content": [{"type": "text", "text": "hi"}],
                           "source": {"kind": "future-plugin-kind", "x": 1}})
                );
            }
            other => panic!("应降级为 Unknown，实际 {other:?}"),
        }
    }
}
