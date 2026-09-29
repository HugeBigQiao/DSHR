//! 模型选择类事件族：`model/selection`、`subagent/model-selection-policy`。
//!
//! 主要用途：会话级模型路由选择（完整校验过的 provider/model/reasoningEffort）与
//! 子代理委托可选的精确路由表两组事件的 data 类型。
//! 为什么需要：这两组都是 **log-only**（不进派生模型历史），却决定了「下一次请求发给谁」；
//! 与 request/context（运行时实际路由）语义不同——一个是意图，一个是事实，混在一起会误判；
//! 且两者由不同包注册（session-controller 与 tool-subagent），同步时需分别核对。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（模型指示器）/ store（路由审计）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：`model/selection` 由 api/session-controller 注册（packages/api/session-controller/src/types.ts
//! 的 `declare module '@deepseek-ai/dsh-session/types'`，L35-43）；写入点：
//! packages/api/session-controller/src/agent.ts 的 ModelSelectionManager.selectForNextRequest()
//! （L326-329，`agent.session.append('model/selection', selection)`）与
//! packages/api/session-controller/src/commands.ts 的 selectModel()（L119-145，
//! resolveCallConfig 校验后落事件）。
//! `subagent/model-selection-policy` 由 tool-subagent 注册（packages/subagent/tool-subagent/src/
//! model-selection-state.ts 的 SessionEventMap，L9-22）：该功能的 settings 默认 off——
//! 事件缺省 = 固定路由定义；用户启用后才由 recordSubagentModelSelection（L72-81）在首次
//! 模型请求前 append 一次。两个事件都是 log-only（不进派生模型历史，见官方注释）。
use serde::{Deserialize, Serialize};

/// `model/selection` 的 data：完整校验过的模型路由选择。
/// 官方：packages/api/session-controller/src/types.ts 的 ModelSelection（L81-86）
/// 用在模型选择提交事件（下一次 prompt 组装读取；lastUsed/pending 投影据此折叠）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectionData {
    pub provider: String,
    pub model: String,
    /// adapter 持有的推理力度（省略 = 模型默认）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// 一条允许子代理显式选择的模型路由。
/// 官方：packages/subagent/tool-subagent/src/model-selection.ts 的 AllowedModelRoute
/// 用在 SubagentModelSelectionPolicyData.allowed_models。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllowedModelRoute {
    pub provider: String,
    pub model: String,
}

/// `subagent/model-selection-policy` 的 data：本会话委托工具可显式选择的精确路由表。
/// 官方：packages/subagent/tool-subagent/src/model-selection-state.ts 的 SessionEventMap（L9-22）
/// 用在策略事件（每会话至多一次且非空；事件缺省 = 固定路由，见模块头 settings 默认 off）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentModelSelectionPolicyData {
    pub allowed_models: Vec<AllowedModelRoute>,
}
