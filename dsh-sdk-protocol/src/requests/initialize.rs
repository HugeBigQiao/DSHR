//! `initialize` 请求：进程级握手。
//!
//! 主要用途：承载握手请求参数（cwd / provider / model / reasoningEffort? / maxTokens?）
//! 与结果（serverInfo），是 runtime 拉起后第一个也是唯一的「进程级」配置机会。
//! 为什么需要：provider/model 路由与工作区锁定必须发生在任何会话之前，单独成文件是为了
//! 与会话级请求（session.rs）划清范围：这里的字段是整进程生效的，改它等于改「这个 runtime
//! 是谁」；也避免与会话请求共享结构体后出现「同一字段两处语义」。
//! 上接：`requests.rs`（再出口）、`dsh-sdk-client` 的 client.rs::initialize；
//!       `dshr-state` 的 raw 层/配置层（把 config.json 的 provider/model 填进来）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/sdk/protocol/src/types.ts 的 InitializeParams / InitializeResult
//! 用在 runtime 启动后的第一步（serverInfo 校验、provider/model 配置）。
use serde::{Deserialize, Serialize};

/// initialize 的参数。
///
/// 为什么需要：这三个必填字段就是「这个 runtime 对着谁、在哪个目录干活」的全部定义——
/// 服务端据此校验 provider/model 路由并锁定工作区，写错只会在后续 prompt 才炸，
/// 所以集中成一个类型由调用方一次给全。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub cwd: String,
    pub provider: String,
    pub model: String,
    /// adapter 持有的推理力度（wire 上 `reasoningEffort`；省略 = 模型默认）。
    /// 官方：packages/sdk/protocol/src/types.ts 的 InitializeParams.reasoningEffort（L20-27，
    ///       类型是 packages/llm/llm/src/types.ts 的 ReasoningEffortId）
    /// 服务端消费点：packages/sdk/server/src/server.ts 的 HarnessSdkJsonRpcServer.initialize
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// initialize 的结果。
///
/// 为什么需要：握手必须回一个可校验的凭据；但**不要**用它做 runtime 版本判断——
/// `serverInfo.version` 官方硬编码为 `'0.0.1'`（见 DESIGN §9.1 第 5 条），
/// 版本校验要读已安装包的 package.json。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub server_info: ServerInfo,
}

/// runtime 的服务标识。
/// 官方：packages/sdk/protocol/src/types.ts 的 InitializeResult.serverInfo
/// 用在 InitializeResult.server_info（wire 名 "deepseek-harness-sdk-runtime"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}
