//! dshr-state：桌面端 state 层。
//!
//! 分层（dshr/DESIGN.md §3；本轮改名与文件风格对齐 Rust 2018+）：
//!
//! ```text
//! dshr-ui（纯页面）
//!    ↓ Cmd / ↑ Event
//! engine   核心数据处理：多 runtime/会话路由 + 脏检测 + 落库节流
//!    ↓ wire 级事件 / ↑
//! raw      与 SDK 沟通：进程生死 + wire 协议 + 订阅 + WireLog
//!    ↓
//! dsh-sdk-client → dsh --profile sdk
//! ```
//!
//! `fold` 与 `snapshot` 是被 engine 调用的**纯数据层**（不参与上面这条流）：
//! `engine → fold::Folder → snapshot::SessionSnapshot`。
//!
//! 模块清单：
//!   config    配置加载（config.json）
//!   raw       与 SDK 沟通（原 engine；`Runtime` 一个实例 = 一个 runtime 子进程）
//!   engine    核心数据处理（类型层 + `engine::Engine`：多 runtime 注册表 / 路由 / 落库）
//!   fold      纯投影：会话事件流 → 内存快照（UI 只消费它产出的 snapshot）
//!   snapshot  fold 的输出类型（UI 模型；engine 只搬运不解释）
//!   record    全程记录（一个 JSONL：cat=dsh 细到 event / cat=app 分开）
//!   runtime   runtime 获取（锁版本 pnpm install）
//!   store     sqlite 加工库（消费 fold 的 SessionSnapshot）
//!   workspace 工作区内文件读写（仅限相对路径）
//!
//! 关于「入口」：本 crate **不提供可执行文件**——程序入口在 `dshr-ui` 的 `main.rs`
//! （由它启动 App 并驱动 engine）。此前的 `src/main.rs` + `src/session.rs`
//!（独立全链路自跑入口）已删除：它的驱动逻辑与 `raw` 重复，且经 UI 启动即可覆盖同一路径。
pub mod config;
pub mod engine;
pub mod fold;
pub mod raw;
pub mod record;
pub mod runtime;
pub mod secrets;
pub mod snapshot;
pub mod store;
pub mod workspace;
