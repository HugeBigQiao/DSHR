//! 帧形状对账（`dsh-sdk-protocol` 契约测试 2/2）。
//!
//! 覆盖 `tests/_conventions.md` 的另外三条关注点：**形状解析**（用真实录制帧，而不是手写
//! JSON——手写的那一份已经在 `dshr-state/tests/engine_flow.rs` 里专门做「必填字段自检」）、
//! **内容块 roundtrip**、**请求面只有 3 个方法**。
//!
//! 为什么用真实帧做形状对账：协议层的容错策略是「data 反序列化失败 → 静默降级 `Unknown`」，
//! 于是**形状写错不报错**，只在上层表现为「事件没反应」。要证明形状仍然对得上，最可靠的
//! 证据是官方真发出来的帧。样本来自 `dshr/data/wire-logs/*.jsonl`（`cat=dsh` 的线级记录），
//! 每种事件类型取第一条，原样入 fixture。
//!
//! 上接：无（测试入口）。
//! 下接：`dsh_sdk_protocol::notifications`（4 种通知的解析入口）、
//!       `dsh_sdk_protocol::content_block`（内容块类型与 lossless 兜底）、
//!       `dsh_sdk_protocol::requests`（3 个请求的形状）、`dsh_sdk_protocol::rpc`（信封层）。

use serde_json::json;

use dsh_sdk_protocol::content_block::{ContentBlock, ImageMediaType};
use dsh_sdk_protocol::notifications::{self, Kind};
use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SdkPromptContentBlock, SessionPromptParams, ShutdownResult,
};
use dsh_sdk_protocol::rpc::{self, Frame, Notification, ParseError};
use dsh_sdk_protocol::session_event::message::MessageSource;
use dsh_sdk_protocol::session_event::message_source::MessageSourceForm;

/// 真实录制的线级记录样本（每种事件类型一行；来源见文件头注释）。
const SAMPLE: &str = include_str!("fixtures/wire-log-sample.jsonl");

/// 样本里覆盖的会话事件类型（排序后）。它同时是「样本没被截断」的护栏：
/// 换 fixture 时同步这里，否则截断会静默降低覆盖率。
const SAMPLE_EVENT_TYPES: &[&str] = &[
    "agent/inbox/spliced",
    "approval/policy",
    "assistant/attempt",
    "assistant/message",
    "permission/preset",
    "request/context",
    "request/header",
    "sandbox/mode",
    "session-log-deepseek/delivery-accepted",
    "session/title",
    "step/end",
    "step/start",
    "system/message",
    "turn/end",
    "turn/start",
    "user/message",
];

/// 逐帧解析真实录制样本：每条都必须是**结构化变体**，而不是降级成 `Unknown`。
#[test]
fn recorded_wire_frames_parse_structurally() {
    // 存 owned 串（每行的 `rec` 在本轮结束就释放，借用活不到最后比对）。
    let mut seen: Vec<String> = Vec::new();
    let mut status_frames = 0usize;

    for (lineno, line) in SAMPLE.lines().enumerate() {
        let rec: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("第 {lineno} 行非 JSON：{e}"));
        assert_eq!(rec["cat"], "dsh", "第 {lineno} 行不是线级记录");
        assert_eq!(rec["kind"], "notification", "第 {lineno} 行不是通知");

        let raw = &rec["raw"];
        let method = raw["method"].as_str().expect("raw.method 应为字符串");
        let notif = Notification {
            method: method.to_string(),
            params: raw["params"].clone(),
        };
        let parsed = notifications::parse(&notif)
            .unwrap_or_else(|e| panic!("第 {lineno} 行（{method}）解析失败：{e}"));

        match parsed {
            Some(Kind::SessionEvent(n)) => {
                let event = &raw["params"]["event"];
                let ty = event["type"].as_str().expect("event.type 应为字符串");
                assert_eq!(
                    n.event.event_type(),
                    ty,
                    "第 {lineno} 行（{ty}）被解析成了别的类型/降级成 Unknown —— \
                     多半是官方改了字段形状，或 dshr 的变体字段与官方不一致"
                );
                assert_eq!(
                    n.event.seq(),
                    event["seq"].as_u64().expect("event.seq"),
                    "第 {lineno} 行 seq 不一致"
                );
                assert_eq!(
                    n.event.time(),
                    event["time"].as_u64().expect("event.time"),
                    "第 {lineno} 行 time 不一致"
                );
                assert_eq!(n.session_id.is_empty(), false, "会话事件必带 sessionId");
                seen.push(ty.to_string());
            }
            Some(Kind::SessionStatus(s)) => {
                status_frames += 1;
                assert!(!s.session_id.is_empty(), "status 帧必带 sessionId");
            }
            Some(other) => panic!("第 {lineno} 行：样本里不该出现 {other:?}"),
            None => panic!("第 {lineno} 行：{method} 应是已知方法（未知方法才返回 None）"),
        }
    }

    seen.sort_unstable();
    seen.dedup();
    let seen: Vec<&str> = seen.iter().map(String::as_str).collect();
    assert_eq!(
        seen.as_slice(),
        SAMPLE_EVENT_TYPES,
        "样本覆盖的事件类型集合变了（fixture 被截断？还是新增了类型却没更新这里？）"
    );
    assert_eq!(status_frames, 1, "样本应含 1 条 session.status");
}

/// 真实会话出现过的**消息来源 kind** 必须都被建模（否则含该 source 的整条消息降级 `Unknown`）。
///
/// 为什么单独一条而不是只靠上面那条通用断言：通用断言只报「降级了」，不报「是哪一种 kind 没建模」。
/// 2026-09-29 就是这条缺口的现场：真实日志里 `{kind:'system-prompt'}`（5 条 system/message）与
/// `{kind:'runtime-context', form:'snapshot', sections:[…]}`（5 条 user/message）全被漏移植。
#[test]
fn real_message_sources_are_modelled() {
    // 样本里实测出现过的来源 kind（2026-09-29 全量 44 个 wire-log 扫描的结论）。
    // 出现新值说明官方加了生产者：先去它的 `declare module` 找形状再补变体，别猜。
    const EXPECTED_KINDS: &[&str] = &["model", "runtime-context", "system-prompt", "user"];

    let mut kinds: Vec<String> = Vec::new();
    for (lineno, line) in SAMPLE.lines().enumerate() {
        let Ok(rec) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if rec["raw"]["method"] != "session.event" {
            continue;
        }
        let event = &rec["raw"]["params"]["event"];
        let ty = event["type"].as_str().unwrap_or_default();
        // 只有这几种事件的载荷里带 message（`data.message`；user/message 的 data 就是 message）。
        let message = match ty {
            "user/message" => &event["data"],
            "system/message" | "assistant/message" | "developer/message" | "tool/result" => {
                &event["data"]["message"]
            }
            _ => continue,
        };
        let source = &message["source"];
        if source.is_null() {
            continue;
        }
        let kind = source["kind"].as_str().expect("source.kind 应为字符串");
        let _parsed: MessageSource = serde_json::from_value(source.clone()).unwrap_or_else(|e| {
            panic!(
                "第 {lineno} 行（{ty}）：source.kind = {kind:?} 没建模 → 整条消息会降级 Unknown。\n\
                 source = {source}\n错误：{e}"
            )
        });
        kinds.push(kind.to_string());
    }

    kinds.sort_unstable();
    kinds.dedup();
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    assert_eq!(
        kinds.as_slice(),
        EXPECTED_KINDS,
        "真实帧里出现的消息来源集合变了（新 kind 必须先补建模，否则含它的消息整条降级）"
    );
}

/// 2026-09-29 补齐的 5 个 kind：形状按**已安装 runtime 的代码/声明**逐条取证。
///
/// 为什么这些必须单独一条测试：它们此前完全没建模，含它们的消息**整条**降级 `Unknown`。
/// 样本不是编的——每条都注明官方出处（见 `message_source.rs` 的变体注释）。
#[test]
fn extended_message_sources_are_modelled() {
    let cases = [
        (
            "plan-mode",
            json!({ "kind": "plan-mode", "form": "notice", "summary": "已切到计划模式" }),
        ),
        (
            "model-selection",
            json!({ "kind": "model-selection", "form": "notice", "summary": "m1 → m2" }),
        ),
        ("user-approval", json!({ "kind": "user-approval" })),
        ("ptc-mode", json!({ "kind": "ptc-mode" })),
        (
            "compact-checkpoint",
            json!({ "kind": "compact-checkpoint", "compactionId": "c-1", "sourceCommandId": "cmd-1" }),
        ),
    ];
    for (kind, case) in cases {
        let src: MessageSource = serde_json::from_value(case.clone())
            .unwrap_or_else(|e| panic!("source.kind = {kind} 应能解析：{e}"));
        match (&src, kind) {
            (MessageSource::PlanMode { form, summary }, "plan-mode") => {
                assert_eq!(form, &Some(MessageSourceForm::Notice));
                assert_eq!(summary.as_deref(), Some("已切到计划模式"));
            }
            (MessageSource::ModelSelection { form, .. }, "model-selection") => {
                assert_eq!(form, &Some(MessageSourceForm::Notice));
            }
            (MessageSource::UserApproval {}, "user-approval") => {}
            (MessageSource::PtcMode {}, "ptc-mode") => {}
            (
                MessageSource::CompactCheckpoint {
                    compaction_id,
                    source_command_id,
                },
                "compact-checkpoint",
            ) => {
                assert_eq!(compaction_id, "c-1");
                assert_eq!(source_command_id.as_deref(), Some("cmd-1"));
            }
            (other, _) => panic!("{kind} 解析成了别的变体：{other:?}"),
        }
    }

    // 漂移容忍：官方 `ContextFormed` 的 form/summary 都可省略、`sourceCommandId` 只在手工压缩时出现
    // ——只写 kind 的帧也必须能解析（否则又是「一处漂移吃掉整条消息」）。
    for kind in [
        "plan-mode",
        "model-selection",
        "user-approval",
        "ptc-mode",
        "compact-checkpoint",
    ] {
        let minimal = if kind == "compact-checkpoint" {
            json!({ "kind": kind, "compactionId": "c-1" })
        } else {
            json!({ "kind": kind })
        };
        serde_json::from_value::<MessageSource>(minimal)
            .unwrap_or_else(|e| panic!("只写必需字段的 {kind} 应能解析：{e}"));
    }
}

/// 官方 7 种内容块 + 旧日志兼容的 `tool-result`：解析 → 序列化必须逐字段一致。
#[test]
fn content_blocks_roundtrip() {
    let cases = [
        json!({ "type": "text", "text": "你好" }),
        json!({ "type": "reasoning", "text": "想一想" }),
        json!({
            "type": "image",
            "attachment": {
                "attachmentId": "att-1",
                "mediaType": "image/png",
                "bytes": 12,
                "width": 4,
                "height": 5,
                "name": "a.png",
            },
            "offloaded": true,
        }),
        json!({
            "type": "file",
            "attachment": { "attachmentId": "att-2", "name": "b.txt", "bytes": 3 },
        }),
        json!({ "type": "tool-call", "id": "call-1", "name": "read_file", "arguments": "{}" }),
        json!({
            "type": "tool-result",
            "toolCallId": "call-1",
            "content": [{ "type": "text", "text": "ok" }],
            "isError": false,
        }),
        json!({ "type": "tool-addition", "toolName": "grep" }),
        json!({ "type": "tool-removal", "toolName": "grep" }),
    ];

    for case in cases {
        let block: ContentBlock = serde_json::from_value(case.clone())
            .unwrap_or_else(|e| panic!("官方块必须能解析：{case} → {e}"));
        assert!(
            !matches!(block, ContentBlock::Unknown { .. }),
            "官方块不该降级成 Unknown：{case}"
        );
        let back = serde_json::to_value(&block).expect("应能序列化");
        assert_eq!(back, case, "序列化应与输入逐字段一致（含 kebab-case 打标）");
    }
}

/// 未知块类型 lossless 保留（插件扩展面：官方 `ContentBlockMap` 是 merge-extensible）。
#[test]
fn unknown_content_block_is_lossless() {
    let case = json!({ "type": "future-block", "x": 1, "nested": { "y": [1, 2] } });
    let block: ContentBlock =
        serde_json::from_value(case.clone()).expect("未知块也必须能解析（lossless）");
    match block {
        ContentBlock::Unknown { block_type, fields } => {
            assert_eq!(block_type, "future-block", "原始 type 串必须保留");
            assert_eq!(fields["x"], json!(1));
            assert_eq!(fields["nested"]["y"], json!([1, 2]), "其余字段必须原样保留");
        }
        other => panic!("期望 Unknown，实际 {other:?}"),
    }
}

/// 请求面（3 个方法）的形状 + 信封层的方向判定与错误分流。
///
/// 为什么把「只有 3 个方法」也写成测试：请求面加第 4 个方法必须同步 `DESIGN.md` §4.1
/// 与官方 `HarnessSdkRequestMap`；这里逐个钉住现有 3 个的形状，改动时至少会在这里被看见。
#[test]
fn request_surface_roundtrips_and_classifies() {
    // ① initialize：camelCase；None 字段不上线（省略而非 null）。
    let params = InitializeParams {
        cwd: "D:/ws".to_string(),
        provider: "deepseek-official".to_string(),
        model: "deepseek-chat".to_string(),
        reasoning_effort: None,
        max_tokens: None,
    };
    let value = serde_json::to_value(&params).expect("应能序列化");
    assert_eq!(
        value,
        json!({ "cwd": "D:/ws", "provider": "deepseek-official", "model": "deepseek-chat" }),
        "None 字段必须省略（官方 wire 上是可选字段，不是 null）"
    );
    assert_eq!(
        serde_json::from_value::<InitializeParams>(value).expect("应能反序列化"),
        params
    );

    // 结果：serverInfo.version 是官方硬编码的 '0.0.1'，不可用于版本校验。
    let result: InitializeResult = serde_json::from_value(json!({
        "serverInfo": { "name": "deepseek-harness-sdk-runtime", "version": "0.0.1" },
    }))
    .expect("应能反序列化");
    assert_eq!(result.server_info.version, "0.0.1");

    // ② session/prompt：文本块与内联图片块（untagged 判别：带 data+mimeType 的才是内联图）。
    let params = SessionPromptParams {
        session_id: "s-1".to_string(),
        content_blocks: vec![
            SdkPromptContentBlock::text("看这张图"),
            SdkPromptContentBlock::image("QUFB", ImageMediaType::Png),
        ],
    };
    let value = serde_json::to_value(&params).expect("应能序列化");
    assert_eq!(value["sessionId"], "s-1");
    assert_eq!(value["contentBlocks"][0]["type"], "text");
    assert_eq!(value["contentBlocks"][1]["type"], "image");
    assert_eq!(value["contentBlocks"][1]["mimeType"], "image/png");
    assert_eq!(
        serde_json::from_value::<SessionPromptParams>(value).expect("应能反序列化"),
        params,
        "contentBlocks 的 untagged 判别必须可逆"
    );

    // ③ shutdown：无 params、结果是空对象。
    assert_eq!(
        serde_json::to_value(ShutdownResult {}).expect("应能序列化"),
        json!({})
    );

    // ④ 信封：有 id = 响应（配对），无 id 有 method = 通知（分发）。
    let line = rpc::build_request("session/prompt", 7, "{}");
    assert_eq!(
        line,
        r#"{"jsonrpc":"2.0","id":7,"method":"session/prompt","params":{}}"#
    );
    assert_eq!(rpc::classify(&line), Some(Frame::Response { id: 7 }));
    assert!(matches!(
        rpc::classify(r#"{"jsonrpc":"2.0","method":"session.event","params":{}}"#),
        Some(Frame::Notification(_))
    ));
    // 非帧输出（官方会往 stdout 插非 JSON 行）：返回 None，调用方继续读循环而不是中断。
    assert_eq!(rpc::classify("这不是 JSON"), None);
    assert_eq!(rpc::classify(r#"{"jsonrpc":"2.0"}"#), None);

    // ⑤ 响应解析：result 优先 → error → 都没有则 MissingResult。
    let out: InitializeResult = rpc::parse(
        r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"n","version":"0.0.1"}}}"#,
    )
    .expect("应能解析 result");
    assert_eq!(out.server_info.name, "n");

    let err = rpc::parse::<InitializeResult>(
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"没有这个方法"}}"#,
    )
    .expect_err("error 响应必须报错");
    assert!(
        matches!(&err, ParseError::Rpc(e) if e.code == -32601 && e.message == "没有这个方法"),
        "错误码与消息应原样保留：{err:?}"
    );

    let missing =
        rpc::parse::<InitializeResult>(r#"{"jsonrpc":"2.0","id":1}"#).expect_err("空响应必须报错");
    assert!(
        matches!(missing, ParseError::MissingResult),
        "既无 result 也无 error 应报 MissingResult：{missing:?}"
    );
}
