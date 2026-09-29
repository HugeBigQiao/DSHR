//! JSON-RPC 帧层：信封类型 + 帧判断 + 请求构造 + 响应解析（纯函数，无 I/O）。
//!
//! 主要用途：定义换行分隔 JSON-RPC 的四个动作——`classify`（这行是请求还是通知）、
//! `build_request`（构造请求行）、`parse`（响应行 → 类型化 result），外加 `RpcResponse` /
//! `RpcError` / `Notification` / `Frame` / `ParseError` 五种形状；是「帧长什么样」的唯一出处。
//! 为什么需要：帧逻辑必须与管道 I/O 分开——混在一起就没法不起进程地测协议形状，而且
//! 「官方发版改帧」与「本地读写管道」两类改动会挤进同一文件。本文件保持零 I/O、零 tokio
//! 依赖（只 serde/serde_json），dsh-sdk-client 的错误分层（`ParseError` → `Error`）与
//! dshr-state 的 WireLog 才能直接复用它。
//! 上接：`dsh-sdk-client` 的 transport.rs（读循环调 `classify`、发请求调 `build_request`）
//!       与 client.rs / api.rs（调 `parse`）；`notifications::parse` 消费 `classify` 产出的 `Notification`。
//! 下接：无（只依赖 serde / serde_json）。
//!
//! 官方对应：packages/sdk/protocol/src/transport.ts 的全部帧逻辑——
//! `JsonRpcLineTransport`（换行分隔信封 + 请求/响应配对）与 `JsonRpcResponseError`（= `RpcError`）；
//! 注意本文件**不**对应那里的流/socket 管理。零外部依赖（仅 serde/serde_json），
//! runtime 的 transport 层只管管道 I/O，帧的形状在这里定。
use serde::Deserialize;
use serde::de::DeserializeOwned;

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
/// 官方：packages/sdk/protocol/src/transport.ts 的 JsonRpcResponseError（code + data 保留）
/// 用在 RpcResponse.error（有 error 时 result 为空）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// 错误附带的任意载荷（官方 client 原样保留 data）。
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// 帧解析错误（零依赖，手写 Display 保持 protocol 纯净）。
///
/// 为什么需要：帧层必须能报错却**不能**依赖客户端错误类型（否则 protocol 反向依赖
/// client，分层崩塌），所以自带一个三种失败的最小枚举（反序列化 / wire error / 空响应），
/// 由 dsh-sdk-client 的 `From<ParseError> for Error` 吸收成语义变体；
/// 手写 `Display` 是为了零依赖（不引 thiserror）。
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
///
/// 为什么需要：通知的 params 形状随 method 有 4 种，把「method 字符串 + 原始 params」
/// 作为统一载体穿过广播通道，才能让帧层（本文件）完全不认识通知类型、
/// 而由 `notifications::parse` 一处做类型化——两层解耦的接缝就是这个结构体。
/// 官方：packages/sdk/protocol/src/transport.ts 的通知信封；params 再由 state 按 method 解析成对应通知类型。
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: serde_json::Value,
}

/// 一帧的分类结果。
///
/// 为什么需要：读循环只需按「有没有 id」分流到两条互不相干的路径（配对挂起请求 /
/// 广播事件），把这条规则固化成枚举可避免调用方各自解析 JSON 而读错方向。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// 响应：id 用于配对挂起请求。
    Response { id: u64 },
    /// 通知：method + params 用于分发。
    Notification(Notification),
}

/// 判断一行 JSON-RPC 帧的类型：有 `id` = 响应；无 `id` 有 `method` = 通知。
///
/// # 参数
/// - `line`：stdout 上读到的一整行原始文本（不含换行；空行/心跳也会进来）。
///
/// # 返回
/// `Frame::Response { id }`（响应，用 id 配对挂起请求）、`Frame::Notification`（通知，
/// 带 method + params）；`None` = 不是合法 JSON，或既无 id 也无 method——调用方应记日志
/// 并继续读循环，**不要**据此中断（官方会插入非帧输出）。
///
/// 为什么需要：方向判定规则只有这一条，集中在此才能让 transport 的读循环保持极薄、
/// 不含任何协议知识，也方便用纯字符串单测帧形状。
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
///
/// # 参数
/// - `method`：协议方法名（本 crate 只用到 `initialize` / `session/prompt` / `shutdown`）。
/// - `id`：自增请求号，由 transport 分配并登记进 pending 表。
/// - `params`：已序列化的 JSON 文本；无参数的方法传 `"{}"`（官方 wire 上也是 `{}`）。
///
/// # 返回
/// 可直接写进 stdin 的完整请求行（**不含**结尾换行，由 transport 补）。
///
/// 注意：`method` 是 `format!` 直拼进 JSON 字符串的，调用方只能传协议里的字面量方法名，
/// 不可传外部输入（否则会破坏 JSON 结构）。
pub fn build_request(method: &str, id: u64, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

/// 从响应行取 result。
///
/// # 参数
/// - `line`：读循环按 id 配对后回传的原始响应行。
///
/// # 返回
/// 反序列化成功的 `T`（各请求的 result 类型，见 requests/）。
///
/// # 错误
/// - `ParseError::Json`：信封或 result 内容不合法；
/// - `ParseError::Rpc`：runtime 回了 JSON-RPC error（code/message/data 原样保留）；
/// - `ParseError::MissingResult`：既没有 result 也没有 error。
///
/// 为什么需要：把「result 优先、否则报 error」这条官方语义固定在一处，
/// 类型化方法（client.rs 的 initialize/prompt/shutdown）才能只做三行委托。
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
