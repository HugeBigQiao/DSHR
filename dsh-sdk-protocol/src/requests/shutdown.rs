//! `shutdown` 请求：优雅关闭 runtime。
//! 官方：packages/sdk/protocol/src/types.ts 的 HarnessSdkRequestMap['shutdown']
//! 用在收尾：shutdown 没有 params（官方 params: undefined），
//! 所以不需要请求结构体；result 是空对象（Record<string, never>）。
use serde::{Deserialize, Serialize};

/// shutdown 的结果：空对象。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownResult {}
