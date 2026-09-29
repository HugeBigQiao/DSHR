//! 请求侧 wire 类型（dshr → dsh）。
//!
//! 主要用途：承接 3 个**请求**方法的参数与结果类型，并统一再出口，供 client 类型化方法使用。
//! 为什么需要：请求与通知必须分文件、分方向——请求是「你发的」，改它会影响调用方；
//! 通知是「dsh 发的」，改它影响解析宽容度。两者混在一个文件里会让「我改坏了哪一侧」
//! 无法从文件名判断；这里也约束了请求面**只有 3 个方法**（面试图加第 4 个必须同步 DESIGN §4.1）。
//! 上接：`dsh-sdk-client` 的 client.rs（initialize / prompt / shutdown）与 api.rs（run）；
//!       `dshr-state` raw 层构造 prompt 参数。
//! 下接：本模块的子模块 `initialize` / `session` / `shutdown`（各自的官方形状与注释）。
//!
//! 官方对应：packages/sdk/protocol/src/types.ts 的 HarnessSdkRequestMap（3 个请求方法：
//! `initialize` / `session/prompt` / `shutdown`）。
//! 方向：这些是"你发的"，和 notifications.rs（dsh 发的）相对。
pub mod initialize;
pub mod session;
pub mod shutdown;

pub use initialize::{InitializeParams, InitializeResult, ServerInfo};
pub use session::{
    SdkEncodedImageBlock, SdkImageKind, SdkPromptContentBlock, SessionPromptParams,
    SessionPromptResult,
};
pub use shutdown::ShutdownResult;
