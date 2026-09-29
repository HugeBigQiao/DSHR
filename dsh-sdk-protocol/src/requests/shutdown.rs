//! `shutdown` 请求：优雅关闭 runtime。
//!
//! 主要用途：只提供结果类型 `ShutdownResult`（空对象），作为协议层「优雅关闭」这一步的形状。
//! 为什么需要：shutdown 没有 params（官方 params: undefined），所以**不需要**请求结构体；
//! 但结果类型必须存在且可解析，否则 client.rs::shutdown 无法用统一的 `rpc::parse` 走完
//! 「请求 → 校验 → 关 stdin → dispose 阶梯」。单列一个文件是为了不让「空请求 + 空结果」
//! 的例外形状污染其余请求文件。
//! 上接：`requests.rs`（再出口）、`dsh-sdk-client` 的 client.rs::shutdown。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：packages/sdk/protocol/src/types.ts 的 HarnessSdkRequestMap['shutdown']
//! 用在收尾：shutdown 没有 params（官方 params: undefined），
//! 所以不需要请求结构体；result 是空对象（Record<string, never>）。
use serde::{Deserialize, Serialize};

/// shutdown 的结果：空对象。
///
/// 为什么需要：只为了让 shutdown 复用与其他请求同一条解析路径（`rpc::parse::<ShutdownResult>`）；
/// 它不携带任何信息，官方语义是 `Record<string, never>`——**不要**往里加字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownResult {}
