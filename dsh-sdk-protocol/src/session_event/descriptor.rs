//! 子代理描述符事件：`subagent/descriptor`。
//! 官方：packages/subagent/subagent/src/descriptor.ts。
use serde::{Deserialize, Serialize};

/// `subagent/descriptor` 的 data：子代理组成声明（按 mode 判别的联合）。
/// 官方：packages/subagent/subagent/src/descriptor.ts 的 SessionEventMap['subagent/descriptor']
///      （版本：SUBAGENT_DESCRIPTOR_VERSION = 3，L48）
/// 用在子会话初始 turn 内首次请求前追加恰好一次（fold 取第一个，后来的不能改写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum SubagentDescriptorData {
    /// 不可冷恢复的一次性子代理。
    OneShot {
        /// SUBAGENT_DESCRIPTOR_VERSION = 3，逐字校验。
        version: u32,
        /// ctx.subagents 的 provider 名。
        provider: String,
        /// 初始委托的短 description，作为持久创建标签。
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// 声明了可冷恢复组成的子代理。
    #[serde(rename_all = "camelCase")]
    Continuable {
        /// SUBAGENT_DESCRIPTOR_VERSION = 3（官方 descriptor.ts L48，逐字校验；
        /// v3 起新增 agentReasoningEffort——对 v2 日志的兼容读见下方 Option）。
        version: u32,
        provider: String,
        /// 必填（用于持久枚举）。
        label: String,
        /// 解析后的 child agentOptions.provider。
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_provider: Option<String>,
        /// 解析后的 child agentOptions.model。
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_model: Option<String>,
        /// 解析后的 child agentOptions.reasoningEffort（官方 ReasoningEffortId 字符串）。
        /// 官方：packages/subagent/subagent/src/descriptor.ts 的
        ///      ContinuableSubagentDescriptorData.agentReasoningEffort（L80-81，v3 新增）
        /// 兼容：v2 日志无此字段 → None（跳过序列化）。
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_reasoning_effort: Option<String>,
        /// 恢复时遮蔽部署 persona 的子 persona。
        #[serde(skip_serializing_if = "Option::is_none")]
        persona: Option<String>,
        /// 工具限制（allow/deny 列表）。
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_filter: Option<ToolRestriction>,
    },
}

/// 工具限制（官方 ToolRestriction，packages/core/tools/src/index.ts）。
/// 用在 SubagentDescriptorData::Continuable.tool_filter。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRestriction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::session_event::SessionEvent;

    /// v3 descriptor（SUBAGENT_DESCRIPTOR_VERSION = 3，带 agentReasoningEffort）可解析并 roundtrip。
    #[test]
    fn descriptor_v3_continuable_with_reasoning_effort_roundtrips() {
        let wire = json!({
            "type": "subagent/descriptor",
            "seq": 2,
            "time": 200,
            "data": {
                "mode": "continuable",
                "version": 3,
                "provider": "subagent-spawn-in-process",
                "label": "audit",
                "agentProvider": "deepseek-official",
                "agentModel": "deepseek-v4-flash",
                "agentReasoningEffort": "high",
                "persona": "auditor",
                "toolFilter": {"allow": ["read", "glob"], "deny": ["write"]}
            }
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("v3 descriptor 应可解析");
        match &event {
            SessionEvent::SubagentDescriptor { data, .. } => {
                let SubagentDescriptorData::Continuable {
                    version,
                    label,
                    agent_model,
                    agent_reasoning_effort,
                    persona,
                    tool_filter,
                    ..
                } = data
                else {
                    panic!("应解析为 Continuable，实际 {data:?}");
                };
                assert_eq!(*version, 3);
                assert_eq!(label, "audit");
                assert_eq!(agent_model.as_deref(), Some("deepseek-v4-flash"));
                assert_eq!(agent_reasoning_effort.as_deref(), Some("high"));
                assert_eq!(persona.as_deref(), Some("auditor"));
                assert_eq!(
                    tool_filter.as_ref().unwrap().allow.as_deref(),
                    Some(&["read".to_string(), "glob".to_string()][..])
                );
            }
            other => panic!("应解析为 SubagentDescriptor，实际 {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }

    /// 兼容读：v2 日志的 continuable descriptor 无 agentReasoningEffort → None，仍可解析。
    #[test]
    fn descriptor_v2_without_reasoning_effort_parses() {
        let event: SessionEvent = serde_json::from_value(json!({
            "type": "subagent/descriptor",
            "seq": 3,
            "time": 300,
            "data": {
                "mode": "continuable",
                "version": 2,
                "provider": "p",
                "label": "l",
                "agentModel": "m"
            }
        }))
        .expect("v2 descriptor 应可解析");
        match &event {
            SessionEvent::SubagentDescriptor { data, .. } => {
                let SubagentDescriptorData::Continuable {
                    agent_reasoning_effort,
                    ..
                } = data
                else {
                    panic!("应解析为 Continuable，实际 {data:?}");
                };
                assert_eq!(agent_reasoning_effort, &None);
            }
            other => panic!("应解析为 SubagentDescriptor，实际 {other:?}"),
        }
    }
}
