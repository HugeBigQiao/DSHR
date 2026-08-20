//! `dshr-runtime`：管理单个 runtime 进程（sidecar）。
//!
//! 分层：`process`（进程生死）→ `transport`（管道对话）→ `client`（总装师）。
pub mod client;
pub mod error;
pub mod process;
pub mod transport;
