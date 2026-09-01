//! hook 扩展事件族：`hook/invoked`、`hook/result`。
//! 官方：packages/hooks/hook-protocol/src/events.ts。
use serde::{Deserialize, Serialize};

/// `hook/invoked` 的 data：一次 hook 调用开始。
/// 官方：packages/hooks/hook-protocol/src/events.ts 的 SessionEventMap['hook/invoked']
/// 用在 hook 调用事件（与 hook/result 按 handlerId 配对；原生插件不写这组事件）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInvokedData {
    /// 调用所在的 open turn。
    pub turn: u64,
    /// hook 点（'PreToolUse'、'Stop'、…）。
    pub point: String,
    /// 方言：'claude-code' | 'codex'。
    pub dialect: HookDialect,
    /// 关联 invoked/result 对的稳定 id。
    pub handler_id: String,
    /// 选中它的 matcher-group 模式；缺省 = match-all。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
}

/// hook 方言（官方 HookDialect）。
/// 用在 HookInvokedData.dialect。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookDialect {
    ClaudeCode,
    Codex,
}

/// `hook/result` 的 data：hook 调用结算。
/// 官方：packages/hooks/hook-protocol/src/events.ts 的 SessionEventMap['hook/result']
/// 用在 hook 结果事件（decision 推导：优先取解析出的 decision，否则 continue===false → 'stop'，否则 'pass'）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResultData {
    pub turn: u64,
    pub point: String,
    pub handler_id: String,
    /// 'approve' | 'allow' | 'block' | 'deny' | 'ask' | 'stop' | 'pass' 之一。
    pub decision: String,
    /// 进程退出码；进程无法运行时不出现。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// trim 后截断（超长加 '…'）；stderr 为空时不出现。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_summary: Option<String>,
    /// 运行墙钟时长（runHook 起止）。
    pub duration_ms: u64,
}
