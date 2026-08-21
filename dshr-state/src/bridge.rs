//! ③ runtime 对接层：state 内唯一 import `dshr-runtime` 的地方。
//!
//! 定义"和 runtime 对接的数据格式"（RtInfo/SendOutcome）+ 对 HarnessClient 的薄封装。

pub mod bridge;

pub use bridge::{Bridge, RtInfo, SendOutcome};
