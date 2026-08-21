//! `ContentBlock` 的 fallback：手写 `Deserialize`。
//!
//! 官方协议是 merge-extensible（插件可注册新块类型），Rust 枚举是封闭集合，
//! 所以反序列化走"通用信封 → 按 type 分发"：已知 5 种 → 类型化变体；
//! 未知 → `Unknown`（原始字段 lossless 保留）。
use serde::Deserialize;
use serde::de::{self, Deserializer};

use super::contentblock::{
    ContentBlock, ImageBlock, ReasoningBlock, TextBlock, ToolCallBlock, ToolResultBlock,
};

/// 通用信封：不判别，先接住一切。
#[derive(Deserialize)]
struct RawBlock {
    #[serde(rename = "type")]
    block_type: String,
    /// 除 type 外的其余字段原样保留。
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // 手写反序列化的核心分发：
        // 接收：任意内容块 JSON。
        // 处理：先解通用信封（type + 其余字段扁平收集），再按 type 分发——
        //       已知 5 种 → 解成对应 Block 结构体（含递归）；
        //       未知 → 字段原样保留进 Unknown（lossless）。
        // 生成：类型化的 ContentBlock。
        let raw = RawBlock::deserialize(d)?;
        // rest 从 Map 转回 Value，按 type 分发到对应块类型（含递归）。
        let rest = serde_json::Value::Object(raw.rest);
        Ok(match raw.block_type.as_str() {
            "text" => {
                let b: TextBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::Text(b)
            }
            "reasoning" => {
                let b: ReasoningBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::Reasoning(b)
            }
            "image" => {
                let b: ImageBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::Image(b)
            }
            "tool-call" => {
                let b: ToolCallBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolCall(b)
            }
            "tool-result" => {
                let b: ToolResultBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolResult(b)
            }
            other => ContentBlock::Unknown {
                block_type: other.to_string(),
                fields: match rest {
                    serde_json::Value::Object(map) => map,
                    _ => serde_json::Map::new(),
                },
            },
        })
    }
}
