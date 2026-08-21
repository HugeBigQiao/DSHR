//! 命令扩展事件族：`command/run`、`command/done`。
//! 官方：packages/interaction/commands/src/types.ts。
use serde::{Deserialize, Serialize};

/// `command/run` 的 data：一次命令开始。
/// 官方：packages/interaction/commands/src/types.ts 的 SessionEventMap['command/run']
/// 用在命令事件（与 command/done 按 commandId 配对，镜像 tool/call↔tool/result）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRunData {
    /// 单调、带实例 token 前缀（resume 后不重复）。
    pub command_id: String,
    pub name: String,
    /// recordInput:false 时缺省（权威领域事件自己携带输入载荷）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    /// 当前唯一变体 { kind: 'user' }（merge-extensible sum）。
    pub source: CommandSource,
}

/// 命令来源（官方 CommandSource，当前唯一变体 user）。
/// 用在 CommandRunData.source。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CommandSource {
    User,
}

/// `command/done` 的 data：命令结算。
/// 官方：packages/interaction/commands/src/types.ts 的 SessionEventMap['command/done']
/// 用在命令结束事件（抛异常/被 abort 的 handler 以 kind:'error' + 渲染后失败文本结算）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDoneData {
    /// 与 command/run 配对的 id。
    pub command_id: String,
    pub kind: CommandDoneKind,
    /// handler 原样结果；error 时为渲染后的失败文本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 仅成功且 handler 指定时出现；指向权威领域事件 seq。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_event_seq: Option<u64>,
}

/// 命令结算种类（官方 'success' | 'error'）。
/// 用在 CommandDoneData.kind。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandDoneKind {
    Success,
    Error,
}
