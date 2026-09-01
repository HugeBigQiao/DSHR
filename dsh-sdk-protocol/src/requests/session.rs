//! `session/prompt` 请求：一个用户回合。
//! 官方：packages/sdk/protocol/src/types.ts 的 SessionPromptParams / SessionPromptResult
//! 用在向会话发消息（未知 sessionId 懒创建 session，响应是 messageId 入队回执）。
use serde::{Deserialize, Serialize};

use crate::content_block::{ContentBlock, ImageMediaType, TextBlock};

/// session/prompt 的参数。
/// 官方：types.ts 的 SessionPromptParams（contentBlocks 类型见 L40-52）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptParams {
    pub session_id: String,
    pub content_blocks: Vec<SdkPromptContentBlock>,
}

/// 内联栅格图片块（未入 runtime attachment 库的原始字节，base64）。
/// 官方：types.ts 的 SdkEncodedImageBlock（L40-47）；
/// 服务端准入点：packages/sdk/server/src/server.ts 的 durablePromptContent → admitEncodedImages。
/// 与 ContentBlock::Image 的区别：后者引用已入库的 durable attachment，本类型带原始字节。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkEncodedImageBlock {
    #[serde(rename = "type")]
    pub kind: SdkImageKind,
    /// canonical base64 栅格字节。
    pub data: String,
    /// 声明的栅格 MIME 类型（准入时校验）。
    pub mime_type: ImageMediaType,
}

/// 内联图片块的 type 打标（固定 "image"）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SdkImageKind {
    #[serde(rename = "image")]
    Image,
}

/// session/prompt 的 contentBlocks 元素：普通 ContentBlock 或内联图片。
/// 官方：types.ts 的 SdkPromptContentBlock（L49-52）= ContentBlock | SdkEncodedImageBlock。
/// untagged：带 data+mimeType 的 image 块 → EncodedImage；其余（含 attachment 引用的 image）→ Block。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SdkPromptContentBlock {
    EncodedImage(SdkEncodedImageBlock),
    Block(ContentBlock),
}

impl SdkPromptContentBlock {
    /// 便捷构造：纯文本块。
    pub fn text(text: impl Into<String>) -> Self {
        Self::Block(ContentBlock::Text(TextBlock { text: text.into() }))
    }

    /// 便捷构造：内联 base64 图片。
    pub fn image(data: impl Into<String>, mime_type: ImageMediaType) -> Self {
        Self::EncodedImage(SdkEncodedImageBlock {
            kind: SdkImageKind::Image,
            data: data.into(),
            mime_type,
        })
    }
}

/// session/prompt 的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptResult {
    pub message_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// wire 形状：内联图片块带 type:"image"（camelCase mimeType），普通文本块照旧。
    #[test]
    fn sdk_prompt_content_block_wire_shape() {
        let encoded = SdkPromptContentBlock::image("aGVsbG8=", ImageMediaType::Png);
        assert_eq!(
            serde_json::to_value(&encoded).unwrap(),
            json!({"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"})
        );
        let text = SdkPromptContentBlock::text("hi");
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            json!({"type": "text", "text": "hi"})
        );
    }
}
