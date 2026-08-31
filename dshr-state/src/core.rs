//! ② 处理层：真正的活都在这——配置 / 写库 / 会话状态机 / 数据转接。
//!
//! - [`config`]：配置读取（data/ 下 config.json/secrets.json/cordis.yml，去 .env 决策 12）
//! - [`store`]：调 dshr-data 的薄封装
//! - [`session`]：会话状态机（current_turn/pending_tool 配对/turn_id 回填）
//! - [`transcode`]：形状转换（SessionEvent → UiEvent，Command → bridge 调用）

pub mod config;
pub mod session;
pub mod store;
pub mod transcode;

pub use config::{Config, DshrConfig, Secrets};
