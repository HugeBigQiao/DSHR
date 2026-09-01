//! 模式开关扩展事件族：`plan/mode`、`sandbox/mode`。
//! 官方：packages/plan/plan-mode/src/index.ts、packages/sandbox/sandbox-policy/src/session-mode.ts。
use serde::{Deserialize, Serialize};

/// `plan/mode` 的 data：plan 模式开关（最后写入者胜，无事件 fold 为 inactive）。
/// 官方：packages/plan/plan-mode/src/index.ts 的 SessionEventMap['plan/mode']
/// 用在 plan 模式切换事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanModeData {
    pub active: bool,
}

/// `sandbox/mode` 的 data：沙箱模式（最后一个事件即会话的覆盖值）。
/// 官方：packages/sandbox/sandbox-policy/src/session-mode.ts 的 SessionEventMap['sandbox/mode']
/// 用在沙箱模式切换事件（在会话的下一次受限调用生效）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SandboxModeData {
    pub mode: SandboxMode,
    /// 'delegation' = 子代理继承时出现；缺省 = 运行时切换。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SandboxModeSource>,
}

/// 沙箱模式枚举（官方 SandboxMode）。
/// 用在 SandboxModeData.mode。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

/// 沙箱来源标记（官方 source，当前唯一变体 'delegation'）。
/// 用在 SandboxModeData.source。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxModeSource {
    Delegation,
}
