//! store 的值转换：rusqlite 整数列（i64）↔ u64、时间戳、状态串。
//!
//! 主要用途：`store/write.rs` 写库与 `store.rs` 读库时，把 Rust 的 u64（时间戳 ms / 行数 /
//! token 数）与 SQLite 的 i64 互相转换；并把 `SessionStatus` 落成存储文本。
//! 为什么需要：SQLite 的 INTEGER 是**有符号** 64 位，而领域模型里的计数与时间戳都是 u64。
//! 把这一处不安全的 `as` 转换集中在一个文件，才能只在此处 `#[allow(clippy::cast_possible_wrap)]`
//! 并书面说明「值域远小于 i64::MAX」的理由——散落各处会掩盖真实的溢出风险。
//! 上接：`store/write.rs`（`sql_i64`/`sql_i64o`）、`store.rs`（`sql_u64`/`now_ms`/`status_of`）。
//! 下接：无（纯函数，只依赖 `dsh_sdk_protocol` 的状态枚举）。
//! 官方对应：无（dshr 自己的持久化细节）。
use std::time::{SystemTime, UNIX_EPOCH};

use dsh_sdk_protocol::notifications::SessionStatus;

/// u64 → i64（写库前）。
///
/// 为什么需要：SQLite 无无符号整型；时间戳/行数/token 数都远小于 `i64::MAX`，
/// 所以 `as` 截断无实际风险。集中在此处并显式 allow，避免这个假设蔓延到别处。
/// 入参 `v`：任意 u64 计数或时间戳。返回：同值的 i64 位表示。
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn sql_i64(v: u64) -> i64 {
    v as i64
}

/// `Option<u64>` → `Option<i64>`（写库前，用于可空列）。
///
/// 为什么需要：多数统计列在「无数据」时是 SQL NULL 而非 0（例如没有 token 账目的轮），
/// 所以要保留 Option 语义而不是先 unwrap_or(0)。`None` 原样传递为 NULL。
/// 入参 `v`：可空的 u64。返回：可空的 i64。语义同 [`sql_i64`]。
#[allow(clippy::cast_possible_wrap)]
pub(crate) fn sql_i64o(v: Option<u64>) -> Option<i64> {
    v.map(|x| x as i64)
}

/// i64 → u64（读库后）。
///
/// 为什么需要：读回来的 i64 要还原成领域模型的 u64，否则调用方到处写 `as u64`。
/// 入参 `v`：库中读出的 i64 计数/时间戳。返回：同值的 u64（值域假设同 [`sql_i64`]）。
pub(crate) fn sql_u64(v: i64) -> u64 {
    v as u64
}

/// 当前 Unix epoch 毫秒。
///
/// 为什么需要：会话的 `created_at`/`updated_at` 在「快照里没有消息」时（纯 status/title 的会话）
/// 退回到落库时刻，需要一个统一的取时入口（便于将来替换成可注入的时钟以便测试）。
/// 返回：epoch 毫秒；系统时钟异常（早于 epoch）时返回 0 而非 panic。
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 会话状态 → 存储文本。
///
/// 为什么需要：库里存字符串而非枚举序号，可读且跨版本稳定；取值与 wire 的
/// `session/status` 通知一致（kebab-case），使监控页可直接展示、无需再映射。
/// 入参 `s`：协议层的会话状态。返回：`"idle"` 或 `"running"` 的 `'static` 串。
/// 官方对应：`packages/sdk/protocol/src/types.ts` 的 `SessionStatusNotification.status`。
pub(crate) fn status_of(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Running => "running",
    }
}
