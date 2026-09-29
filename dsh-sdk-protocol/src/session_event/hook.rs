//! hook 扩展事件族：`hook/invoked`、`hook/result`。
//!
//! 主要用途：一次 hook 调用开始与结算（按 handlerId 配对）两组事件的 data 类型，
//! 含方言（claude-code / codex）与决策字符串。
//! 为什么需要：hook 是「外部脚本插手 agent 行为」的审计链，其决策（approve/deny/…）
//! 与退出码/耗时是排查「为什么这个工具被拦」的唯一证据；官方把它放在独立的
//! hook-protocol 包，且原生插件不写这组事件——这一反直觉点需要文件级说明。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（hook 时间线）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/hooks/hook-protocol/src/events.ts 的
//! `SessionEventMap['hook/invoked']` 与 `SessionEventMap['hook/result']`。
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
