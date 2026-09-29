//! 模式开关扩展事件族：`plan/mode`、`sandbox/mode`。
//!
//! 主要用途：plan 模式开关与沙箱模式（含 delegation 来源标记）两组事件的 data 类型。
//! 为什么需要：这两个开关直接改变**工具可用性与权限**（沙箱模式决定能写哪），
//! 回放时必须取最后一个事件当作当前值（覆盖式，无增量），与快照/增量事件的折叠方式不同；
//! 单列文件让这条折叠约定只写一次，也便于与 approval/policy 对照（同 turn 会一起被写）。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（模式指示器）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/plan/plan-mode/src/index.ts 的 `SessionEventMap['plan/mode']`、
//! packages/sandbox/sandbox-policy/src/session-mode.ts 的 `SessionEventMap['sandbox/mode']`。
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
