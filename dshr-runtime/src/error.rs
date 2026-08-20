//! 统一运行时错误。
//!
//! 分层设计：protocol 的 `ParseError`（帧解析）通过 `From` 吸收进来，
//! state 再通过 `From<Error>` 吸收本类型——"共享"靠转换链，不靠单一类型。
use thiserror::Error;

use dshr_protocol::rpc::ParseError;

#[derive(Debug, Error)]
pub enum Error {
    /// 管道/进程 I/O 失败（spawn、读写 stdin/stdout、kill/wait）。
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    /// 参数序列化失败（serde_json）。
    #[error("JSON 序列化失败: {0}")]
    Json(#[from] serde_json::Error),
    /// 帧解析/协议错误（来自 protocol::rpc::ParseError）。
    #[error("协议错误: {0}")]
    Protocol(#[from] ParseError),
    /// 读循环已关闭（transport 退出，无配对结果）。
    #[error("读循环已关闭")]
    TransportClosed,
    /// runtime 进程提前退出（stdout EOF）。
    #[error("runtime 提前退出")]
    RuntimeExited,
}
