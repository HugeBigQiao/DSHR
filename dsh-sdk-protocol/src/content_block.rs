//! `ContentBlock` 模块：内容块类型 + 未知块 fallback。
//!
//! 对应官方 `packages/llm/llm/src/types.ts` 的 `ContentBlockMap`（merge-extensible）。
//! 子模块：`contentblock`（类型定义）、`fallback`（手写 Deserialize）。
pub mod contentblock;
pub mod fallback;

pub use contentblock::{
    ContentBlock, ImageAttachmentRef, ImageBlock, ImageMediaType, ReasoningBlock, TextBlock,
    ToolCallBlock, ToolResultBlock,
};
