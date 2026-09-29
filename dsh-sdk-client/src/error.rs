//! 统一客户端错误。
//!
//! 主要用途：把「进程 I/O、JSON、协议、超时、runtime 退出」五类失败收敛成一个 `Error`，
//! 并用 `From<ParseError>` 把帧层错误转成语义变体。
//! 为什么需要：调用方（dshr-state raw 层、UI 错误提示）需要一个**可穷尽 match** 的错误集，
//! 且必须能区分「可重试的超时」与「runtime 已死」；单列文件而非建 error crate，
//! 是因为分层明确：`ParseError` 属协议层（帧形状），`Error` 属客户端层（调用语义），
//! 两者用 `From` 衔接即可，不值得为两个 crate 再拆一个包（见 DESIGN §6 第 4 条）。
//! 上接：`dsh-sdk-client` 的 process / transport / client / subscription / api（全部返回它）；
//!       `dshr-state` 的 raw 层（按变体决定报错与落库）。
//! 下接：`dsh_sdk_protocol::rpc::ParseError` / `RpcError`（经 `From` 吸收）、thiserror。
//!
//! 分层设计：protocol 的 `ParseError`（帧解析）经 `From` 转成本类型的语义变体；
//!
//! 官方对应：四个协议级变体一一对应官方 client 的四个错误类（packages/sdk/client/src/client.ts）：
//!   `RpcError`        ← `JsonRpcResponseError`（wire error 响应，code+data 保留）
//!   `RequestTimeout`  ← `RequestTimeoutError`（请求超时）
//!   `SdkProtocol`     ← `SdkProtocolError`（响应不符合文档化协议）
//!   `TransportClosed` ← `TransportClosedError`（runtime 已退出，带 exit code + stderr 尾部）
use thiserror::Error;

use dsh_sdk_protocol::rpc::{ParseError, RpcError as WireRpcError};

/// 客户端错误全集（五类）。
///
/// 为什么需要：调用方需要按「能不能继续用这个 client」分流——`RequestTimeout` 可重试、
/// `TransportClosed` 说明 runtime 已死（后续请求必然失败）、`RpcError` 是协议层拒绝；
/// 把这五个变体放在一个封闭枚举里，match 才是穷尽的（新增失败类型会编译报错）。
/// `Io`/`Json` 保留 `#[from]` 以便用 `?` 透传，其余四类对应官方的四个错误类。
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
            ParseError::Rpc(WireRpcError {
                code,
                message,
                data,
            }) => Error::RpcError {
                code,
                message,
                data,
            },
            ParseError::MissingResult => {
                Error::SdkProtocol("响应既无 result 也无 error".to_string())
            }
        }
    }
}
