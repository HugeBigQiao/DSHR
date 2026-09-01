//! web 搜索扩展事件族：`web/deepseek-search-llm-request`。
//! 官方：packages/web/web-search-deepseek/src/provider.ts。
use serde::{Deserialize, Serialize};

/// `web/deepseek-search-llm-request` 的 data：辅助 DeepSeek search 请求的精确快照（dispatch 前记录）。
/// 官方：packages/web/web-search-deepseek/src/provider.ts 的 SessionEventMap['web/deepseek-search-llm-request']
/// 用在搜索请求事件（secret-free，不含 API key；固定单条 user/text 消息 + 单一 web_search 工具）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekSearchLlmRequestData {
    /// 完全解析的 Messages 端点。
    pub endpoint: String,
    /// anthropic-version header 值。
    pub api_version: String,
    pub body: DeepSeekSearchBody,
}

/// 请求体（Anthropic Messages API 形状，注意字段是 snake_case）。
/// 用在 DeepSeekSearchLlmRequestData.body。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekSearchBody {
    pub model: String,
    pub max_tokens: u64,
    pub messages: Vec<DeepSeekSearchMessage>,
    pub tools: Vec<DeepSeekSearchTool>,
}

/// 请求消息（固定 role:'user' + 单个 text 块）。
/// 用在 DeepSeekSearchBody.messages。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekSearchMessage {
    pub role: DeepSeekSearchRole,
    pub content: Vec<DeepSeekSearchTextBlock>,
}

/// 消息角色（官方固定 'user'）。
/// 用在 DeepSeekSearchMessage.role。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepSeekSearchRole {
    User,
}

/// 单个 text 内容块。
/// 用在 DeepSeekSearchMessage.content。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekSearchTextBlock {
    #[serde(rename = "type")]
    pub block_type: DeepSeekSearchBlockType,
    pub text: String,
}

/// 块类型（官方固定 'text'）。
/// 用在 DeepSeekSearchTextBlock.block_type。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepSeekSearchBlockType {
    Text,
}

/// web_search 工具声明。
/// 用在 DeepSeekSearchBody.tools。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekSearchTool {
    #[serde(rename = "type")]
    pub tool_type: DeepSeekSearchToolType,
    pub name: String,
    pub max_uses: u64,
}

/// 工具类型（官方固定 'web_search_20250305'，带下划线，显式 rename）。
/// 用在 DeepSeekSearchTool.tool_type。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeepSeekSearchToolType {
    #[serde(rename = "web_search_20250305")]
    WebSearch20250305,
}
