//! `ContentBlock` 的 fallback：手写 `Deserialize`。
//!
//! 主要用途：为 `ContentBlock` 提供唯一的手写反序列化入口——先解通用信封
//!（type + 其余字段扁平收集），再按 type 分发到类型化变体或 `Unknown`。
//! 为什么需要：官方协议是 merge-extensible（插件可注册新块类型），Rust 枚举是封闭集合，
//! 派生 `Deserialize` 会让任何未知块（甚至已知块的未来新增字段组合）**整体解析失败**，
//! 进而让整条消息/整个事件丢失。放在独立文件是为了让「宽容策略」只有一处：
//! 官方发版加块类型时，只动这里的分发表，不动类型定义。
//! 上接：serde（`ContentBlock` 的 `Deserialize` 由本文件实现）、经 `notifications::parse`
//!       与 `session_event` 各族的路径间接触发（消息/工具结果/流记录）。
//! 下接：`contentblock.rs` 的各 Block 结构体。
//!
//! 官方对应：packages/llm/llm/src/types.ts 的 `ContentBlockMap`（解析端；
//! 未知块保留原始字段的宽容行为对齐官方 merge-extensible 语义）。
//!
//! 官方协议是 merge-extensible（插件可注册新块类型），Rust 枚举是封闭集合，
//! 所以反序列化走"通用信封 → 按 type 分发"：已知 8 种（含 0.1.7-rc.2 新增的
//! tool-addition / tool-removal）→ 类型化变体；未知 → `Unknown`（原始字段
//! lossless 保留）。`tool-result` 官方已移除，保留分发仅为读旧日志。
use serde::Deserialize;
use serde::de::{self, Deserializer};

use super::contentblock::{
    ContentBlock, FileBlock, ImageBlock, ReasoningBlock, TextBlock, ToolAdditionBlock,
    ToolCallBlock, ToolRemovalBlock, ToolResultBlock,
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
        //       已知 6 种 → 解成对应 Block 结构体（含递归）；
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
            "file" => {
                let b: FileBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::File(b)
            }
            "tool-call" => {
                let b: ToolCallBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolCall(b)
            }
            "tool-result" => {
                let b: ToolResultBlock = serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolResult(b)
            }
            "tool-addition" => {
                let b: ToolAdditionBlock =
                    serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolAddition(b)
            }
            "tool-removal" => {
                let b: ToolRemovalBlock =
                    serde_json::from_value(rest).map_err(de::Error::custom)?;
                ContentBlock::ToolRemoval(b)
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
