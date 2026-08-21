//! ② 处理层：真正的活都在这——配置 / 写库 / 会话状态机 / 数据转接。
//!
//! - [`config`]：配置读取（.env 的 DSH_DATA_DIR 等）
//! - [`store`]：调 dshr-data 的薄封装（TODO(3f)：下轮和消费循环一起写）
//! - [`session`]：会话状态机（current_turn/pending_tool 配对/turn_id 回填）
//! - [`transcode`]：形状转换（SessionEvent → UiEvent，Command → bridge 调用）

pub mod config;
pub mod session;
pub mod store;
pub mod transcode;

pub use config::Config;
