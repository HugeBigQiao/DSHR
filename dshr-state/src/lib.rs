//! dshr-state：桌面端 state 层（v3 重建）。
//!
//! 参考旧 dshr-state 的分层：本 crate 是"UI 与 SDK 之间的翻译官 + 调度器"，
//! 未来桌面端在此之上加 ui/ 层。当前实现（M2 前哨）：
//!   config   配置加载（config.json）
//!   record   全程记录（一个 JSONL：cat=dsh 细到 event / cat=app 分开）
//!   runtime  runtime 获取（锁版本 npm install）
//!   session  全链路运行（full round：spawn → initialize → run → shutdown）
pub mod config;
pub mod record;
pub mod runtime;
pub mod session;
