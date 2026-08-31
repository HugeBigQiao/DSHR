//! 未知通知/事件的兜底（协议演进时新方法名不炸，记日志即可）。
//!
//! 对应 dshr-protocol 的 fallback 思路：未知方法跳过，已知方法畸形才报错。

/// 未知方法名的通知：协议演进（官方加了新通知），dshr 还没实现 → 静默跳过。
/// 接收：通知的 method 字符串。
/// 生成：仅 eprintln 日志（不打断事件流，不落库——未知形状无法结构化）。
pub fn handle_unknown_method(method: &str) {
    eprintln!("[dshr-state] 未知通知方法（协议演进，跳过）: {method}");
}
