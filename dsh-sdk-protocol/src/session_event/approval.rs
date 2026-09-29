//! 审批类扩展事件族：`approval/asked`、`approval/decided`、`approval/policy`、`permission/preset`。
//!
//! 主要用途：审批询问/结果（按 id 配对）、会话审批策略开关、权限预设选择四组事件的 data 类型。
//! 为什么需要：这是「模型想干危险动作，谁批准的」的唯一审计链，涉及两个插件包且语义独立于
//! 工具事件（同一个 tool call 可能被拒而不产生 tool/result）；单列文件也让已知限制有出处——
//! SDK 通知面固定 4 种、**没有**审批转发通道（见 DESIGN §10），故这里只做记录与展示。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（审批时间线）/ store（审计事实）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/interaction/user-approval/src/index.ts（asked/decided/policy）、
//! packages/interaction/permission-presets/src/index.ts（preset）。
use serde::{Deserialize, Serialize};

/// `approval/asked` 的 data：一次审批询问。
/// 官方：packages/interaction/user-approval/src/index.ts 的 SessionEventMap['approval/asked']
/// 用在审批流（与 approval/decided 按 id 配对；必须位于开着的 turn 内）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalAskedData {
    /// 每次 request 由 randomUUID() 新鲜生成。
    pub id: String,
    /// 被询问的工具名。
    pub tool_name: String,
    /// 关联已展示的 tool call（不重复 arguments）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// asker 的人类可读解释（如 hook 的权限决定理由）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `approval/decided` 的 data：审批结果。
/// 官方：packages/interaction/user-approval/src/index.ts 的 SessionEventMap['approval/decided']
/// 用在审批结算（每个 ask 恰好一条；'unavailable' = 无应答者 fail-closed）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecidedData {
    /// 与 approval/asked 配对的 id。
    pub id: String,
    pub outcome: ApprovalOutcome,
}

/// 审批结果枚举（官方 ApprovalOutcome）。
/// 用在 ApprovalDecidedData.outcome。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalOutcome {
    AllowedOnce,
    Rejected,
    Cancelled,
    Unavailable,
}

/// `approval/policy` 的 data：会话审批策略开关（最后写入者胜）。
/// 官方：packages/interaction/user-approval/src/index.ts 的 SessionEventMap['approval/policy']
/// 用在策略切换事件（'ask' 是缺省，'never' 确定性拒绝）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalPolicyData {
    pub policy: ApprovalPolicy,
    /// 'delegation' = 子会话委托时种入的覆盖；缺省 = 运行时切换。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// 审批策略枚举（官方 ApprovalPolicy）。
/// 用在 ApprovalPolicyData.policy。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    Ask,
    Never,
}

/// `permission/preset` 的 data：用户选中的权限预设（最后写入者胜，本身不是旋钮）。
/// 官方：packages/interaction/permission-presets/src/index.ts 的 SessionEventMap['permission/preset']
/// 用在预设选择事件（应用时同 turn 还会写 sandbox/mode 和 approval/policy 两个旋钮事件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionPresetData {
    /// 预设表键（如 'strict'、'custom' 之外的开关项）。
    pub preset: String,
}
