//! 统一客户端错误。
//!
//! 分层设计：protocol 的 `ParseError`（帧解析）经 `From` 转成本类型的语义变体；
//! 四个协议级变体一一对应官方 client 的四个错误类（packages/sdk/client/src/client.ts）：
//!   `RpcError`        ← `JsonRpcResponseError`（wire error 响应，code+data 保留）
//!   `RequestTimeout`  ← `RequestTimeoutError`（请求超时）
//!   `SdkProtocol`     ← `SdkProtocolError`（响应不符合文档化协议）
//!   `TransportClosed` ← `TransportClosedError`（runtime 已退出，带 exit code + stderr 尾部）
use thiserror::Error;

use dsh_sdk_protocol::rpc::{ParseError, RpcError as WireRpcError};

#[derive(Debug, Error)]
pub enum Error {
    /// 管道/进程 I/O 失败（spawn、读写 stdin/stdout、kill/wait）。
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 序列化/反序列化失败（serde_json）。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// runtime 返回 JSON-RPC error 响应——官方 `JsonRpcResponseError`（code + data 保留）。
    #[error("RPC 错误 {code}: {message}")]
    RpcError {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    /// 请求超时——官方 `RequestTimeoutError`。
    #[error("{method} 请求超时（{timeout_ms}ms）")]
    RequestTimeout { method: String, timeout_ms: u64 },
    /// 响应不符合文档化协议——官方 `SdkProtocolError`。
    #[error("协议不符合文档: {0}")]
    SdkProtocol(String),
    /// runtime 已退出——官方 `TransportClosedError`（带 exit code + stderr 尾部）。
    #[error("runtime 已退出（exit_code={exit_code:?}）")]
    TransportClosed {
        exit_code: Option<i32>,
        stderr_tail: Vec<String>,
    },
}

impl From<ParseError> for Error {
    /// 帧层错误 → 语义变体：wire error 响应 → RpcError；信封/内容不合法、缺 result → SdkProtocol。
    fn from(error: ParseError) -> Self {
        match error {
            ParseError::Json(e) => Error::SdkProtocol(format!("帧解析失败: {e}")),
            ParseError::Rpc(WireRpcError { code, message, data }) => {
                Error::RpcError { code, message, data }
            }
            ParseError::MissingResult => Error::SdkProtocol("响应既无 result 也无 error".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 官方 `JsonRpcResponseError` 语义：wire error 响应 → Error::RpcError，code + data 保留。
    #[test]
    fn wire_rpc_error_maps_to_rpc_error_variant() {
        let err = ParseError::Rpc(WireRpcError {
            code: -32601,
            message: "method not found".to_string(),
            data: Some(json!({"detail": "x"})),
        });
        match Error::from(err) {
            Error::RpcError { code, message, data } => {
                assert_eq!(code, -32601);
                assert_eq!(message, "method not found");
                assert_eq!(data, Some(json!({"detail": "x"})));
            }
            other => panic!("应映射为 RpcError，实际 {other:?}"),
        }
    }

    /// 官方 `SdkProtocolError` 语义：帧不合法 → Error::SdkProtocol。
    #[test]
    fn malformed_frame_maps_to_sdk_protocol_variant() {
        let err = ParseError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        match Error::from(err) {
            Error::SdkProtocol(_) => {}
            other => panic!("应映射为 SdkProtocol，实际 {other:?}"),
        }
    }
}
