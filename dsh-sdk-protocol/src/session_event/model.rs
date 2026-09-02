//! 模型选择类事件族：`model/selection`、`subagent/model-selection-policy`。
//!
//! `model/selection` 由 api/session-controller 注册（packages/api/session-controller/src/types.ts
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::session_event::SessionEvent;

    /// 官方形状的 model/selection 事件（含 reasoningEffort）可解析并 roundtrip。
    #[test]
    fn model_selection_event_roundtrips() {
        let wire = json!({
            "type": "model/selection",
            "seq": 7,
            "time": 700,
            "data": {
                "provider": "deepseek-official",
                "model": "deepseek-v4-flash",
                "reasoningEffort": "high"
            }
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("model/selection 应可解析");
        match &event {
            SessionEvent::ModelSelection { seq, time, data } => {
                assert_eq!((*seq, *time), (7, 700));
                assert_eq!(data.provider, "deepseek-official");
                assert_eq!(data.model, "deepseek-v4-flash");
                assert_eq!(data.reasoning_effort.as_deref(), Some("high"));
            }
            other => panic!("应解析为 ModelSelection，实际 {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }

    /// reasoningEffort 缺省时序列化不带该键（官方省略 = 模型默认）。
    #[test]
    fn model_selection_without_effort_omits_key() {
        let data: ModelSelectionData = serde_json::from_value(json!({
            "provider": "p", "model": "m"
        }))
        .expect("无 reasoningEffort 应可解析");
        assert_eq!(data.reasoning_effort, None);
        assert_eq!(
            serde_json::to_value(&data).unwrap(),
            json!({"provider": "p", "model": "m"})
        );
    }

    /// 官方形状的 subagent/model-selection-policy 事件可解析并 roundtrip。
    #[test]
    fn subagent_model_selection_policy_roundtrips() {
        let wire = json!({
            "type": "subagent/model-selection-policy",
            "seq": 8,
            "time": 800,
            "data": {
                "allowedModels": [
                    {"provider": "deepseek-official", "model": "deepseek-v4-flash"},
                    {"provider": "deepseek-official", "model": "deepseek-reasoner"}
                ]
            }
        });
        let event: SessionEvent =
            serde_json::from_value(wire.clone()).expect("model-selection-policy 应可解析");
        match &event {
            SessionEvent::SubagentModelSelectionPolicy { data, .. } => {
                assert_eq!(data.allowed_models.len(), 2);
                assert_eq!(data.allowed_models[0].provider, "deepseek-official");
                assert_eq!(data.allowed_models[1].model, "deepseek-reasoner");
            }
            other => panic!("应解析为 SubagentModelSelectionPolicy，实际 {other:?}"),
        }
        assert_eq!(serde_json::to_value(&event).unwrap(), wire);
    }
}
