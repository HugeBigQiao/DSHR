//! `session/prompt` 请求：一个用户回合。
//! 官方：types.ts 的 SessionPromptParams / SessionPromptResult
//! 用在向会话发消息（未知 sessionId 懒创建 session，响应是 messageId 入队回执）。
use serde::{Deserialize, Serialize};

use crate::content_block::ContentBlock;

/// session/prompt 的参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptParams {
    pub session_id: String,
    pub content_blocks: Vec<ContentBlock>,
}

/// session/prompt 的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptResult {
    pub message_id: String,
}
