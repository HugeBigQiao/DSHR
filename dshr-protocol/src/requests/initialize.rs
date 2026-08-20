//! `initialize` 请求：进程级握手。
//! 官方：types.ts 的 InitializeParams / InitializeResult
//! 用在 runtime 启动后的第一步（serverInfo 校验、provider/model 配置）。
use serde::{Deserialize, Serialize};

/// initialize 的参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub cwd: String,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

/// initialize 的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub server_info: ServerInfo,
}

/// runtime 的服务标识。
/// 官方：types.ts 的 InitializeResult.serverInfo
/// 用在 InitializeResult.server_info（wire 名 "deepseek-harness-sdk-runtime"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}
