//! `SessionEvent` 的 fallback：手写 `Deserialize`。
//!
//! 官方协议是 merge-extensible（插件可注册新事件、版本会继续涨），Rust 枚举
//! 是封闭集合，所以反序列化走"通用信封 → 按 type 分发"：已知核心 13 种 →
//! 类型化变体；未知（如 rc.8 的 team/*、approval/*）→ `Unknown`（全字段 lossless 保留）。
use serde::Deserialize;
use serde::de::{self, Deserializer};

use super::SessionEvent;

/// 通用信封：不判别，先接住一切（type/seq/time/data + 可选扩展字段）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEvent {
    #[serde(rename = "type")]
    event_type: String,
    seq: u64,
    time: u64,
    // 先不解析，当原始 JSON 存着；分发到已知类型时再解。
    data: serde_json::Value,
    #[serde(default)]
    ignorable: Option<bool>,
    #[serde(default)]
    source_event_seqs: Option<Vec<u64>>,
    #[serde(default)]
    surface_op: Option<serde_json::Value>,
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = RawEvent::deserialize(d)?;
        let seq = raw.seq;
        let time = raw.time;
        // 已知类型：把 data（Value）解成对应的事件 data 结构体。
        // 未知类型：全字段进 Unknown（lossless）。
        Ok(match raw.event_type.as_str() {
            "turn/start" => SessionEvent::TurnStart {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "turn/end" => SessionEvent::TurnEnd {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "step/start" => SessionEvent::StepStart {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "step/end" => SessionEvent::StepEnd {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "user/message" => SessionEvent::UserMessage {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "assistant/chunk" => SessionEvent::AssistantChunk {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "assistant/message" => SessionEvent::AssistantMessage {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "tool/call" => SessionEvent::ToolCall {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "tool/result" => SessionEvent::ToolResult {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "todo/write" => SessionEvent::TodoWrite {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "request/header" => SessionEvent::RequestHeader {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "request/context" => SessionEvent::RequestContext {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            "session/end-seed" => SessionEvent::SessionEndSeed {
                seq,
                time,
                data: serde_json::from_value(raw.data).map_err(de::Error::custom)?,
            },
            other => SessionEvent::Unknown {
                event_type: other.to_string(),
                seq,
                time,
                data: raw.data,
                ignorable: raw.ignorable,
                source_event_seqs: raw.source_event_seqs,
                surface_op: raw.surface_op,
            },
        })
    }
}
