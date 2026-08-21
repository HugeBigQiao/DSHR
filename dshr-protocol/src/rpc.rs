//! JSON-RPC 帧层：信封类型 + 帧判断 + 请求构造 + 响应解析（纯函数，无 I/O）。
//!
//! 对应官方 transport.ts 的全部帧逻辑。零外部依赖（仅 serde/serde_json），
//! runtime 的 transport 层只管管道 I/O，帧的形状在这里定。
use serde::de::DeserializeOwned;
use serde::Deserialize;

/// 响应信封：`id` + 可选 `result`/`error`（二者有其一）。
/// 官方：packages/sdk/protocol/src/transport.ts 的 JsonRpcResponse
/// 用在读循环按 id 配对后解析（`jsonrpc` 字段反序列化时自动忽略）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RpcResponse<T> {
    pub id: u64,
    #[serde(rename = "result")]
    pub result: Option<T>,
    pub error: Option<RpcError>,
}

/// JSON-RPC 错误对象。
/// 官方：packages/sdk/protocol/src/transport.ts 的 JsonRpcResponseError
/// 用在 RpcResponse.error（有 error 时 result 为空）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// 帧解析错误（零依赖，手写 Display 保持 protocol 纯净）。
#[derive(Debug)]
pub enum ParseError {
    /// 反序列化失败（信封或 result 内容不合法）。
    Json(serde_json::Error),
    /// runtime 返回了 JSON-RPC error。
    Rpc(RpcError),
    /// 响应里既没有 result 也没有 error。
    MissingResult,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "响应解析失败: {e}"),
            ParseError::Rpc(e) => write!(f, "rpc error {}: {}", e.code, e.message),
            ParseError::MissingResult => write!(f, "rpc error: 无 result"),
        }
    }
}

impl std::error::Error for ParseError {}

/// 通知帧：method + params（JSON，未解析的具体形状）。
/// 官方：packages/sdk/protocol/src/transport.ts 的通知信封；params 再由 state 按 method 解析成对应通知类型。
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: serde_json::Value,
}

/// 一帧的分类结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// 响应：id 用于配对挂起请求。
    Response { id: u64 },
    /// 通知：method + params 用于分发。
    Notification(Notification),
}

/// 判断一行 JSON-RPC 帧的类型：有 `id` = 响应；无 `id` 有 `method` = 通知。
pub fn classify(line: &str) -> Option<Frame> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if let Some(id) = v.get("id").and_then(serde_json::Value::as_u64) {
        Some(Frame::Response { id })
    } else if let Some(method) = v.get("method").and_then(serde_json::Value::as_str) {
        Some(Frame::Notification(Notification {
            method: method.to_string(),
            params: v.get("params").cloned().unwrap_or(serde_json::Value::Null),
        }))
    } else {
        None
    }
}

/// 构造一行请求（信封）。`params` 为已序列化的 JSON（无 params 时传 `"{}"`）。
pub fn build_request(method: &str, id: u64, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

/// 从响应行取 result。
pub fn parse<T: DeserializeOwned>(line: &str) -> Result<T, ParseError> {
    let resp: RpcResponse<T> = serde_json::from_str(line).map_err(ParseError::Json)?;
    match resp.result {
        Some(result) => Ok(result),
        None => Err(match resp.error {
            Some(e) => ParseError::Rpc(e),
            None => ParseError::MissingResult,
        }),
    }
}
