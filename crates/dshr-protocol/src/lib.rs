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
    // 这个变体携带一个字段 `text: String`，JSON 里就是 `{"type":"text","text":"..."}`。
    Text { text: String },
}

#[cfg(test)] // cargo 测试模块
mod tests {
    // 把父模块（lib.rs 顶层）的所有内容引入，这里就是拿到 `ContentBlock`。
    use super::*;

    // `#[test]`：标记这是测试函数，`cargo test` 会自动发现并运行它。
    #[test]
    fn text_roundtrip() {
        // 构造一个 Text 变体的值。
        // `"...".to_string()`：把字符串字面量（&str）转成拥有所有权的 String。
        let block = ContentBlock::Text {
            text: "Hello, World!".to_string(),
        };
        // 序列化：把 Rust 值变成 JSON 字符串。
        // `.unwrap()`：忽略可能的错误，直接取出结果（测试里失败就该 panic）。
        let json = serde_json::to_string(&block).unwrap();
        // 断言序列化结果**精确等于**这段 JSON。
        // `r#"..."#`：raw string，里面的引号不用转义。
        assert_eq!(json, r#"{"type":"text","text":"Hello, World!"}"#);
        // 反序列化：把 JSON 字符串变回 Rust 值，`: ContentBlock` 指定目标类型。
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        // 断言"序列化→反序列化"后和原值相等（往返一致）。
        // 能通过是因为上面 derive 了 PartialEq。
        assert_eq!(back, block);
    }
}
