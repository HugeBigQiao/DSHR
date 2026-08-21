//! 子代理描述符事件：`subagent/descriptor`。
//! 官方：packages/subagent/subagent/src/descriptor.ts。
use serde::{Deserialize, Serialize};

/// `subagent/descriptor` 的 data：子代理组成声明（按 mode 判别的联合）。
/// 官方：packages/subagent/subagent/src/descriptor.ts 的 SessionEventMap['subagent/descriptor']
/// 用在子会话初始 turn 内首次请求前追加恰好一次（fold 取第一个，后来的不能改写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum SubagentDescriptorData {
    /// 不可冷恢复的一次性子代理。
    OneShot {
        /// SUBAGENT_DESCRIPTOR_VERSION = 2，逐字校验。
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
