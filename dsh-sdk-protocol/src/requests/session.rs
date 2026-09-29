//! `session/prompt` 请求：一个用户回合。
//!
//! 主要用途：承载「往某会话发一条消息」的参数（sessionId + contentBlocks）与回执
//!（messageId）；contentBlocks 同时支持普通内容块与内联 base64 图片。
//! 为什么需要：这是运行期唯一的热路径请求，形状直接决定 UI 能发什么（文本/图片）；
//! 单独成文件是因为它引入了两件别处没有的东西——`untagged` 的内容块联合（内联图片
//! 与 attachment 引用的图片在 wire 上都是 `type:"image"`，得靠字段判别）与懒创建会话的
//! 语义（未知 sessionId 由服务端建 session）。把它与 initialize 放一起会混淆两种生命周期。
//! 上接：`requests.rs`（再出口）、`dsh-sdk-client` 的 client.rs::prompt 与 api.rs::run；
//!       `dshr-state` raw 层与 UI 的发送路径。
//! 下接：`content_block::{ContentBlock, ImageMediaType, TextBlock}`（内容块本体）。
//!
//! 官方对应：packages/sdk/protocol/src/types.ts 的 SessionPromptParams / SessionPromptResult
//!（含 `SdkPromptContentBlock` / `SdkEncodedImageBlock`）
//! 用在向会话发消息（未知 sessionId 懒创建 session，响应是 messageId 入队回执）。
use serde::{Deserialize, Serialize};

use crate::content_block::{ContentBlock, ImageMediaType, TextBlock};

/// session/prompt 的参数。
///
/// 为什么需要：`session_id` 是**普通参数**而不是构造函数字段——一个 runtime 内可并存多会话，
/// 每次 prompt 指认目标会话；未知 id 由服务端懒创建 agent+session 对（故无需先建会话）。
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
    ///
    /// # 参数
    /// - `text`：整段文本（`impl Into<String>`，可直接给 `&str`）。
    ///
    /// # 返回
    /// 一个 `Block(ContentBlock::Text)`——wire 上是 `{"type":"text","text":…}`。
    ///
    /// 为什么需要：调用方（api.rs::run、state/UI）最常用的就是发文本，
    /// 免去手写两层嵌套。
    pub fn text(text: impl Into<String>) -> Self {
        Self::Block(ContentBlock::Text(TextBlock { text: text.into() }))
    }

    /// 便捷构造：内联 base64 图片。
    ///
    /// # 参数
    /// - `data`：canonical base64 的栅格字节（**不带** `data:` 前缀）。
    /// - `mime_type`：声明的栅格 MIME 类型，服务端准入时会校验（见 SdkEncodedImageBlock）。
    ///
    /// # 返回
    /// 一个 `EncodedImage`，wire 上是 `{"type":"image","data":…,"mimeType":…}`；
    /// 与 `ContentBlock::Image`（引用已入库 attachment）不同，这里携带原始字节。
    pub fn image(data: impl Into<String>, mime_type: ImageMediaType) -> Self {
        Self::EncodedImage(SdkEncodedImageBlock {
            kind: SdkImageKind::Image,
            data: data.into(),
            mime_type,
        })
    }
}

/// session/prompt 的结果。
///
/// 为什么需要：prompt 是**入队回执**而非完成通知——调用方拿 `message_id` 才能在自己的
/// 事件流里认出「我这条消息被接纳了」（api.rs::run 即以此判定 receipt-seen），
/// 真正的回答要靠 session.event 流。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptResult {
    pub message_id: String,
}
