//! `ContentBlock` 类型定义：枚举 + 官方块类型 + 附件/图片支持类型。
//!
//! 主要用途：给出 8 种已知块（含保留的 tool-result）与 1 个 Unknown 兜底变体的字段形状，
//! 以及它们引用的附件/图片元数据类型。
//! 为什么需要：类型定义与宽容解析（`fallback.rs` 的手写 `Deserialize`）必须分开——
//! 本文件是「官方现在长什么样」的声明，fallback 是「读到没见过的东西怎么办」的策略；
//! 官方发版时通常只改前者（加变体/加字段），策略不变，分开才能一眼看出改动性质。
//! 上接：`content_block.rs`（再出口）、`requests/session.rs`、`session_event/*`（消息/工具）、
//!       `llm.rs`（StreamChunk::BlockEnd）。
//! 下接：无（只依赖 serde；枚举的解析由兄弟模块 `fallback` 提供）。
//!
//! 官方对应：`packages/llm/llm/src/types.ts` 的 `ContentBlockMap`——
//! 0.1.7-rc.2 起为 7 种（text / reasoning / image / file / tool-call /
//! tool-addition / tool-removal，移除了 tool-result）。本模块额外保留
//! `ToolResult`（旧日志读取兼容）与 `Unknown`（插件扩展面 lossless 兜底）。
use serde::{Deserialize, Serialize};

/// 一条 LLM 消息里的"内容块"（`type` 打标的判别联合）。
/// 官方：packages/llm/llm/src/types.ts 的 ContentBlockMap
/// 用在 prompt 的 contentBlocks 与各消息的 content（双向）。
/// 变体是 newtype：字段形状在下方各 Block 结构体（与官方同名）；
/// 反序列化由 fallback.rs 手工实现（未知类型 → Unknown），`Deserialize` 从 derive 移除。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text(TextBlock),
    Reasoning(ReasoningBlock),
    Image(ImageBlock),
    File(FileBlock),
    ToolCall(ToolCallBlock),
    /// 官方 ContentBlockMap 自 0.1.7-rc.2 起**移除了 `tool-result`**：工具结果
    /// 改由 `tool/result` 事件的消息承载，不再是内容块。这里保留该变体仅作
    /// 旧日志读取兼容（读端宽容），新日志不会再产出。
    ToolResult(ToolResultBlock),
    /// 激活被引用的 request/header 里的某个工具定义（官方 0.1.7-rc.2 新增）。
    ToolAddition(ToolAdditionBlock),
    /// 记录某工具的显式移除（官方 0.1.7-rc.2 新增，按会话内工具名）。
    ToolRemoval(ToolRemovalBlock),
    /// 未知块类型（插件扩展面，lossless 保留原始字段）。
    /// 序列化暂用 derive（会输出 type="unknown"），需要转发时 v2 手写 Serialize。
    Unknown {
        /// 原始 type 字符串。
        block_type: String,
        /// 其余字段原样保留。
        fields: serde_json::Map<String, serde_json::Value>,
    },
}

/// 官方 TextBlock：{ type:'text', text }。用在 ContentBlock::Text。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

/// 官方 ReasoningBlock：{ type:'reasoning', text }。用在 ContentBlock::Reasoning。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningBlock {
    pub text: String,
}

/// 官方 ImageBlock：{ type:'image', attachment, offloaded? }。用在 ContentBlock::Image。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageBlock {
    pub attachment: ImageAttachmentRef,
    /// 该图片已被持久化的 image-offload 决策卸载（或由消息重写保留）：
    /// 所有路由改发占位文本（图片名 + 只读路径），不再发图片字节。
    /// 官方 `packages/llm/llm/src/types.ts` 的 ImageBlock.offloaded（0.1.7-rc.2 新增）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offloaded: Option<bool>,
}

/// 官方 FileBlock：{ type:'file', attachment }。用在 ContentBlock::File。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileBlock {
    pub attachment: FileAttachmentRef,
}

/// 官方 ToolCallBlock：{ type:'tool-call', id, name, arguments }。用在 ContentBlock::ToolCall。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallBlock {
    /// 官方 branded CallId，先用 String。
    pub id: String,
    pub name: String,
    /// 模型产出的原始 JSON 字符串，保持不解析。
    pub arguments: String,
}

/// 官方 ToolResultBlock：{ type:'tool-result', toolCallId, content, isError? }。
/// 用在 ContentBlock::ToolResult（camelCase：toolCallId）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultBlock {
    pub tool_call_id: String,
    /// 递归：结果里可以再含内容块。
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// 官方 ToolAdditionBlock：{ type:'tool-addition', toolName, tool?='never' }。
/// 用在 ContentBlock::ToolAddition（只出现在 developer/message 里，官方注释：
/// "Tool-change blocks belong to developer messages"）。
/// 官方 `packages/llm/llm/src/types.ts` 的 ToolAdditionBlock（0.1.7-rc.2 新增）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAdditionBlock {
    /// 被引用的历史 request/header 中恰好一个工具的名字（schema 归 header 所有）。
    pub tool_name: String,
    // 官方另有 `tool?: never`（标 @persistenceReserved）：内联定义被保留禁用，
    // 该字段在 wire 上永不出现，因此不建对应 Rust 字段。本结构体未开
    // deny_unknown_fields，官方若放宽该字段也不会导致解析失败。
}

/// 官方 ToolRemovalBlock：{ type:'tool-removal', toolName }。
/// 用在 ContentBlock::ToolRemoval；按会话内工具名记录显式移除。
/// 官方 `packages/llm/llm/src/types.ts` 的 ToolRemovalBlock（0.1.7-rc.2 新增）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRemovalBlock {
    /// 被移除工具在会话内的名字。
    pub tool_name: String,
}

/// 图片的持久化元数据（官方 packages/attachment/attachment/src/types.ts 的 ImageAttachmentRef）。
/// 用在 ImageBlock.attachment。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAttachmentRef {
    pub attachment_id: String, // 官方 branded AttachmentId
    pub media_type: ImageMediaType,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 应用 EXIF 方向后、归一化缩放前的输入尺寸（官方 originalDimensions?，
    /// attachment/src/types.ts L24-31）；仅当归一化确实缩小了图片时才出现。
    /// 2026-09-02 大同步新增（0.1.2-alpha.5 起官方会写该字段）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_dimensions: Option<OriginalImageDimensions>,
}

/// 文件的持久化引用（官方 packages/attachment/attachment/src/types.ts 的 FileAttachmentRef）。
/// 用在 FileBlock.attachment。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachmentRef {
    pub attachment_id: String,
    pub name: String,
    pub bytes: u64,
}

/// 原图尺寸（官方 ImageAttachmentRef.originalDimensions 内联对象 {width, height}，
/// packages/attachment/attachment/src/types.ts L24-31）。
/// 用在 ImageAttachmentRef.original_dimensions。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginalImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// 接受的图片格式（官方 ImageMediaType = 'image/png' | 'image/jpeg' | 'image/webp' | 'image/gif'）。
/// 用在 ImageAttachmentRef.media_type（值带斜杠，逐个显式 rename）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageMediaType {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
    #[serde(rename = "image/gif")]
    Gif,
}
