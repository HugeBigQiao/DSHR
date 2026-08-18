//! `dshr-protocol`：DeepSeek Harness SDK 协议的 Rust 类型。
//!
//! 第一个类型 `ContentBlock`：一条 LLM 消息里的"内容块"。
//! 它对应 TS 侧的联合类型：`{type:'text'} | {type:'reasoning'} | ...`，
//! Rust 用 `enum`（判别联合）来表达"多种形态之一"。
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

#[cfg(test)] // cargo 测试模块
mod tests {
    // 把父模块（lib.rs 顶层）的所有内容引入，这里就是拿到 `ContentBlock`。
    use super::*;

    // `#[test]`：标记这是测试函数，`cargo test` 会自动发现并运行它。
    #[test]
    fn roundtrip() {
        let cases: Vec<(ContentBlock, &str)> = vec![
            (
                ContentBlock::Text {
                    text: "Hello, World!".to_string(),
                },
                r#"{"type":"text","text":"Hello, World!"}"#,
            ),
            (
                ContentBlock::Reasoning {
                    text: "思考过程".to_string(),
                },
                r#"{"type":"reasoning","text":"思考过程"}"#,
            ),
            (
                ContentBlock::ToolCall {
                    id: "1".to_string(),
                    name: "tool".to_string(),
                    arguments: "{}".to_string(),
                },
                r#"{"type":"tool-call","id":"1","name":"tool","arguments":"{}"}"#,
            ),
            (
                ContentBlock::ToolResult {
                    tool_call_id: "1".to_string(),
                    content: vec![],
                    is_error: None,
                },
                r#"{"type":"tool-result","toolCallId":"1","content":[]}"#,
            ),
            (
                ContentBlock::ToolResult {
                    tool_call_id: "2".to_string(),
                    content: vec![],
                    is_error: Some(true),
                },
                r#"{"type":"tool-result","toolCallId":"2","content":[],"isError":true}"#,
            ),
            (
                ContentBlock::ToolResult {
                    tool_call_id: "3".to_string(),
                    content: vec![],
                    is_error: None,
                },
                r#"{"type":"tool-result","toolCallId":"3","content":[]}"#,
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(serde_json::to_string(&input).unwrap(), expected);
            println!("input = {input:?}\nexpected = {expected:?}");
        }
    }
}
