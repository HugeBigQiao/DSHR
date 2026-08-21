//! `ContentBlock` 类型定义：枚举 + 官方 5 种块 + 图片支持类型。
//!
//! 对应官方 `packages/llm/llm/src/types.ts` 的 `ContentBlockMap`。
use serde::{Deserialize, Serialize};

/// 一条 LLM 消息里的"内容块"（`type` 打标的判别联合）。
/// 官方：packages/llm/llm/src/types.ts 的 ContentBlockMap（5 变体）
/// 用在 prompt 的 contentBlocks 与各消息的 content（双向）。
/// 变体是 newtype：字段形状在下方各 Block 结构体（与官方同名）；
/// 反序列化由 fallback.rs 手工实现（未知类型 → Unknown），`Deserialize` 从 derive 移除。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text(TextBlock),
    Reasoning(ReasoningBlock),
    Image(ImageBlock),
    ToolCall(ToolCallBlock),
    ToolResult(ToolResultBlock),
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

/// 官方 ImageBlock：{ type:'image', attachment }。用在 ContentBlock::Image。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageBlock {
    pub attachment: ImageAttachmentRef,
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
