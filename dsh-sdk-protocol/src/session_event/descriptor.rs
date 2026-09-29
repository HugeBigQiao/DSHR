//! 子代理描述符事件：`subagent/descriptor`。
//!
//! 主要用途：给出 `SubagentDescriptorData`——子代理组成声明（one-shot / continuable
//! 按 mode 判别，含 version / provider / label / 模型路由 / persona / 工具限制）。
//! 为什么需要：这是「这个子会话是按什么配方跑起来的」的持久事实，冷恢复必须逐字校验版本
//!（SUBAGENT_DESCRIPTOR_VERSION）；与 catalog.rs（目录）分属「怎么建的」和「有哪些」，
//! 且版本兼容读（v2 无 agentReasoningEffort）需要专门说明，故单独成文件。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（子会话详情）/ store。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/subagent/subagent/src/descriptor.ts 的
//! `SessionEventMap['subagent/descriptor']`（版本：SUBAGENT_DESCRIPTOR_VERSION = 3，L48）。
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
