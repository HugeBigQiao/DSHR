//! dshr-state：桌面端 state 层（v3 重建）。
//!
//! 参考旧 dshr-state 的分层：本 crate 是"UI 与 SDK 之间的翻译官 + 调度器"，
//! 分层（DESIGN v3 §9.5 / v4 M3.6）：UI(薄) → dshr-state(中台) → dsh-sdk-client → runtime。
//! 当前实现（M2 前哨 + M3.6 engine 落地）：
//!   config   配置加载（config.json）
//!   engine   常驻会话中台（DESIGN §11.4 落地：spawn/事件循环/fold→快照 + 落库 + WireLog；
//!            原 dshr-ui worker/Machine + real.rs 整体迁入，UI 只搬运命令/事件）
//!   record   全程记录（一个 JSONL：cat=dsh 细到 event / cat=app 分开）
//!   runtime  runtime 获取（锁版本 npm install）
//!   session  全链路运行（full round：spawn → initialize → run → shutdown，独立可执行路径）
//!   store    sqlite 加工库（DESIGN §11：s2 落库，消费 fold 的 SessionSnapshot）
pub mod config;
pub mod engine;
pub mod fold;
pub mod record;
pub mod runtime;
pub mod session;
pub mod snapshot;
pub mod store;
