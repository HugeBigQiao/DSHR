//! `ContentBlock` 模块：内容块类型 + 未知块 fallback。
//!
//! 主要用途：定义一条 LLM 消息里「内容块」的全部类型（文本/推理/图片/文件/工具调用/
//! 工具增删），并统一再出口给消息、流记录与 prompt 参数使用。
//! 为什么需要：内容块是**被复用最广**的协议类型——prompt 参数、user/assistant/tool 消息、
//! `StreamChunk::BlockEnd`、`subagent.finished` 的 lastAssistantMessage 全都引用它；不抽出来
//! 就会出现「同一个块的形状在多处各写一遍」。另一个存在理由是**扩展面**：官方 ContentBlockMap
//! 是 merge-extensible（插件可注册新块），Rust 枚举是封闭集合，所以类型定义（本模块）与
//! 宽容解析（fallback）必须成对出现。
//! 上接：`requests/session.rs`（prompt 的 contentBlocks）、`session_event/message.rs` 与
//!       `session_event/tool.rs`（消息与工具结果）、`llm.rs`（StreamChunk::BlockEnd）、
//!       `notifications.rs`（subagent.finished 的最后助手消息）、`dshr-state` 的 fold/渲染层。
//! 下接：子模块 `contentblock`（类型定义）与 `fallback`（手写 Deserialize）。
//!
//! 官方对应：`packages/llm/llm/src/types.ts` 的 `ContentBlockMap`（merge-extensible），
//! 0.1.7-rc.2 起官方新增 `tool-addition` / `tool-removal`，并移除 `tool-result`
//!（后者仅为旧日志读取兼容而保留变体）。
pub mod contentblock;
pub mod fallback;

pub use contentblock::{
    ContentBlock, FileAttachmentRef, FileBlock, ImageAttachmentRef, ImageBlock, ImageMediaType,
    OriginalImageDimensions, ReasoningBlock, TextBlock, ToolAdditionBlock, ToolCallBlock,
    ToolRemovalBlock, ToolResultBlock,
};
