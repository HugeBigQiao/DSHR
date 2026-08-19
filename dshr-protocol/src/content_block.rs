use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// - `tag = "type"`：序列化时自动插入一个 `"type"` 字段，值为变体名。
//   这是 JSON 里"判别联合"的标准写法，TS 侧就是靠 `type` 字段区分变体的。
// - `rename_all = "kebab-case"`：把 Rust 变体名 `ToolCall` 自动转成 JSON 的 `"tool-call"`。
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_call_id: String,
        content: Vec<ContentBlock>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}
