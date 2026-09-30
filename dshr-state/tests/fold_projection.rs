//! fold 投影语义（`dshr-state` 契约测试 2/3）。
//!
//! 依据：`../DESIGN.md` §12.4「重建优先级」第 3 条与 §3.4（fold 必须**纯**，
//! 纯才能用「事件 JSON → 快照相等」直接断言）。本文件只依赖 `Folder` 的公开接口：
//! `push_notification` / `push_wire_line` / `snapshot`。
//!
//! 为什么值得单独一个文件：fold 是**唯一能被完全单测覆盖**的层，也是协议漂移的第一道闸
//!（字段改名/新事件全靠它兜住而不炸 UI）。它的语义一旦错了，UI 上表现为「内容不对」而不是报错。
//!
//! 上接：无（测试入口）。
//! 下接：`dshr_state::fold::Folder`（折叠状态机）、`dshr_state::snapshot`（快照类型）、
//!       `dsh_sdk_protocol::notifications`（通知解析——engine 走的同一条路）。
//!
//! 约定：本文件里的所有帧都**先经 `notifications::parse`** 再喂 `Folder`，与 engine
//!（`SessionState::feed`）完全同一条路径——否则「测试里的折叠」和「生产里的折叠」会分叉。

use dsh_sdk_protocol::notifications::{self, SessionStatus};
use dsh_sdk_protocol::rpc::Notification;
use serde_json::json;

use dshr_state::fold::Folder;
use dshr_state::snapshot::{MsgKind, SessionSnapshot};

/// 本文件用的会话 id。
const SESSION: &str = "s-fold";

/// 事件时间基准（固定值 → 断言可逐字段钉死，不依赖真实时钟）。
const TIME_BASE: u64 = 1_700_000_000_000;

/// 一条 `session.event` 通知帧（wire 信封：type / seq / time / data）。
fn frame(kind: &str, seq: u64, data: serde_json::Value) -> Notification {
    Notification {
        method: "session.event".to_string(),
        params: json!({
            "sessionId": SESSION,
            "event": { "type": kind, "seq": seq, "time": TIME_BASE + seq, "data": data },
        }),
    }
}

/// 一条 `session.status` 通知帧。
fn status_frame(state: &str) -> Notification {
    Notification {
        method: "session.status".to_string(),
        params: json!({ "sessionId": SESSION, "status": state }),
    }
}

/// 喂一条通知（解析 → 折叠，与 engine 同一路径）。
fn feed(folder: &mut Folder, notif: &Notification) {
    match notifications::parse(notif).expect("帧应能解析") {
        Some(kind) => folder.push_notification(&kind),
        None => panic!("未知通知方法：{notif:?}"),
    }
}

/// `user/message` 的 data（data 就是 Message 本身）。
fn user_message(id: &str, text: &str) -> serde_json::Value {
    json!({
        "id": id,
        "role": "user",
        "content": [{ "type": "text", "text": text }],
        "source": { "kind": "user" },
    })
}

/// `assistant/message` 的 data（必填字段齐全，含 reasoning 正文两块的用法）。
fn assistant_message(id: &str, text: &str, reasoning: Option<&str>) -> serde_json::Value {
    let mut content = Vec::new();
    if let Some(r) = reasoning {
        content.push(json!({ "type": "reasoning", "text": r }));
    }
    content.push(json!({ "type": "text", "text": text }));
    json!({
        "turn": 1,
        "step": 1,
        "message": {
            "id": id,
            "role": "assistant",
            "content": content,
            "source": { "kind": "model", "provider": "deepseek-official", "model": "deepseek-chat" },
        },
        "usage": { "inputTokens": 100, "outputTokens": 20, "totalTokens": 120 },
        "stream": [{
            "type": "text-chunks",
            "time0": TIME_BASE + 4,
            "index": 0,
            "dt": [0, 5],
            "texts": ["你", "好"],
        }],
    })
}

/// `tool/result` 的 data（content 里唯一的 tool-result 块是配对键）。
fn tool_result(call_id: &str, text: &str, is_error: bool) -> serde_json::Value {
    json!({
        "turn": 1,
        "step": 1,
        "message": {
            "id": "m-tool-1",
            "role": "tool",
            "content": [{
                "type": "tool-result",
                "toolCallId": call_id,
                "content": [{ "type": "text", "text": text }],
                "isError": is_error,
            }],
            "source": { "kind": "tool", "callId": call_id },
        },
        "meta": { "diffs": [{ "path": "src/lib.rs", "oldText": "a\nb", "newText": "a\nb\nc" }] },
    })
}

/// 完整一轮的投影：消息流顺序、思考、工具配对、token 账、轮结算、状态。
///
/// 这是 fold 的主契约（其余测试都是它的边界情况）。
#[test]
fn full_turn_projection() {
    // 方法：把**一整轮的 9 条真实帧**（轮开 → 步开 → 用户 → assistant → 工具 call → 工具 result
    // → assistant → 轮结 → 状态）按顺序喂进 `Folder`，再对产出的快照做逐项断言。
    // 为什么用「整轮」而不是逐事件单测：折叠的价值恰恰在**跨事件的关联**（工具配对、轮结算、
    // token 归属），单事件测只能证明「没崩」；而这一条能证明「关联算对了」。
    // 目的：这是 fold 的主契约——其余测试都是在它的基础上各挑一个边界情况（挂起/孤儿/截断/错误）。
    let mut folder = Folder::new();
    let seqs = [
        frame("turn/start", 1, json!({ "turn": 1 })),
        frame("step/start", 2, json!({ "turn": 1, "step": 1 })),
        frame("user/message", 3, user_message("m-u1", "帮我看一下")),
        frame(
            "assistant/message",
            4,
            assistant_message("m-a1", "好的", Some("让我想想")),
        ),
        frame(
            "tool/call",
            5,
            json!({
                "turn": 1, "step": 1, "callId": "c1",
                "name": "edit_file", "arguments": "{\"path\":\"src/lib.rs\"}",
            }),
        ),
        frame("tool/result", 6, tool_result("c1", "已改好", false)),
        frame(
            "assistant/message",
            7,
            assistant_message("m-a2", "改完了", None),
        ),
        frame(
            "turn/end",
            8,
            json!({ "turn": 1, "reason": { "kind": "completed" } }),
        ),
        status_frame("idle"),
    ];
    for notif in &seqs {
        feed(&mut folder, notif);
    }

    let snap = folder.snapshot();
    assert_eq!(snap.session_id, SESSION, "session_id 来自通知");
    assert_eq!(snap.status, Some(SessionStatus::Idle));

    // —— 消息流：顺序 = 事件序，Tool 行在原位 ——
    let kinds: Vec<MsgKind> = snap.messages.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MsgKind::User,
            MsgKind::Assistant,
            MsgKind::Tool,
            MsgKind::Assistant
        ],
        "消息流顺序必须与事件序一致（Tool 行插在 call 的位置）"
    );
    assert_eq!(snap.messages[0].text, "帮我看一下");
    assert_eq!(snap.messages[1].text, "好的");
    assert_eq!(
        snap.messages[1].reasoning.as_deref(),
        Some("让我想想"),
        "思考文本折进同行（由 UI 决定是否展开）"
    );
    assert_eq!(
        snap.messages[1].usage.as_ref().map(|u| u.input_tokens),
        Some(100),
        "token 账目挂在 assistant 行上"
    );
    let stream = snap.messages[1].stream.expect("应折出流摘要");
    assert_eq!(stream.chunks, 2, "text-chunks 的 dt 展开成 2 个 chunk");
    assert_eq!(stream.text_chars, 2, "「你」「好」两个字符");
    assert_eq!(stream.first_token_time, Some(TIME_BASE + 4));

    // —— 工具卡片：call↔result 配对后的字段 ——
    let tool = snap.messages[2].tool.as_ref().expect("Tool 行应带卡片");
    assert_eq!(tool.call_id, "c1");
    assert_eq!(tool.name, "edit_file");
    assert_eq!(tool.duration_ms, 1, "duration = result.time − call.time");
    assert!(!tool.is_error);
    assert_eq!(tool.result.as_deref(), Some("已改好"));
    assert_eq!(tool.diffs.len(), 1, "meta.diffs 应折成 file_ops 的数据源");
    assert_eq!(tool.diffs[0].path, "src/lib.rs");
    assert_eq!((tool.diffs[0].added, tool.diffs[0].removed), (3, 2));

    // —— 统计 ——
    assert_eq!(snap.stats.turns, 1);
    assert_eq!(snap.stats.steps, 1);
    assert_eq!(snap.stats.messages, 3, "只有 User/Assistant 行占消息数");
    assert_eq!(snap.stats.tool_calls, 1);
    assert_eq!(snap.stats.errors, 0);
    assert_eq!(snap.stats.usage.input, 200, "两条 assistant 各 100");
    assert_eq!(snap.stats.usage.total, 240, "total 桶单独累加");

    // —— 轮结算 ——
    assert_eq!(snap.turns.len(), 1);
    assert_eq!(snap.turns[0].turn, 1);
    assert_eq!(snap.turns[0].reason.as_deref(), Some("completed"));
    assert_eq!(snap.turns[0].start_time, Some(TIME_BASE + 1));
    assert_eq!(snap.turns[0].end_time, Some(TIME_BASE + 8));
    assert_eq!(snap.turns[0].usage.input, 200, "轮内 token 独立累计");
}

/// 工具配对的两类异常：**挂起**（call 无 result）与**孤儿**（result 无 call）。
#[test]
fn pending_and_orphan_tools_are_handled() {
    let mut folder = Folder::new();
    feed(
        &mut folder,
        &frame(
            "tool/call",
            1,
            json!({ "turn": 1, "step": 1, "callId": "c1", "name": "read_file", "arguments": "{}" }),
        ),
    );
    // 挂起态：卡片在、结果为空。
    let snap = folder.snapshot();
    assert_eq!(snap.messages.len(), 1);
    let tool = snap.messages[0].tool.as_ref().expect("应留下挂起卡片");
    assert!(tool.result.is_none());
    assert_eq!(tool.duration_ms, 0);
    assert!(!tool.is_error);

    // 孤儿 result（callId 无人认领）→ 忽略，不新增行、不改已有行。
    feed(
        &mut folder,
        &frame("tool/result", 2, tool_result("c-ghost", "无关结果", false)),
    );
    let snap = folder.snapshot();
    assert_eq!(snap.messages.len(), 1, "孤儿 result 不该产行");
    assert!(snap.messages[0].tool.as_ref().unwrap().result.is_none());
    assert_eq!(snap.stats.tool_calls, 1, "工具计数只按 call 记");
    assert_eq!(snap.stats.errors, 0);
}

/// 截断日志：新轮开始时前一轮没有 `turn/end`，必须被强制结算（而不是丢轮或永远开着）。
#[test]
fn truncated_turn_is_forced_closed() {
    let mut folder = Folder::new();
    feed(&mut folder, &frame("turn/start", 1, json!({ "turn": 1 })));
    feed(&mut folder, &frame("turn/start", 2, json!({ "turn": 2 })));

    let snap = folder.snapshot();
    assert_eq!(snap.turns.len(), 2, "前一轮要被补一条结算记录");
    assert_eq!(snap.turns[0].turn, 1);
    assert_eq!(snap.turns[0].end_time, None, "截断轮没有结束时刻");
    assert_eq!(snap.turns[0].reason, None);
    assert_eq!(snap.turns[1].turn, 2);
    assert_eq!(snap.turns[1].end_time, None, "进行中轮：end 为空");

    // 补上 turn/end 后，进行中轮结算。
    feed(
        &mut folder,
        &frame(
            "turn/end",
            3,
            json!({ "turn": 2, "reason": { "kind": "max-tokens" } }),
        ),
    );
    let snap = folder.snapshot();
    assert_eq!(snap.turns[1].reason.as_deref(), Some("max-tokens"));
    assert_eq!(snap.turns[1].end_time, Some(TIME_BASE + 3));
}

/// 错误口径：轮级错误与工具级错误各记一次，不重复计。
#[test]
fn errors_are_counted_per_cause() {
    let mut folder = Folder::new();
    feed(
        &mut folder,
        &frame(
            "tool/call",
            1,
            json!({ "turn": 1, "step": 1, "callId": "c1", "name": "run", "arguments": "{}" }),
        ),
    );
    feed(
        &mut folder,
        &frame("tool/result", 2, tool_result("c1", "失败了", true)),
    );

    let snap = folder.snapshot();
    assert_eq!(snap.stats.errors, 1, "工具级错误");
    assert!(snap.messages[0].tool.as_ref().unwrap().is_error);

    feed(
        &mut folder,
        &frame(
            "turn/end",
            3,
            json!({
                "turn": 1,
                "reason": {
                    "kind": "error",
                    "error": { "message": "上游 500", "code": "HTTP_500" },
                },
            }),
        ),
    );
    let snap = folder.snapshot();
    assert_eq!(snap.stats.errors, 2, "轮级错误另计一次");
    assert_eq!(
        snap.turns[0].reason.as_deref(),
        Some("error/HTTP_500: 上游 500"),
        "结束原因折成一行文本"
    );
}

/// **同源同巡**：在线（SDK 通知）与离线（WireLog JSONL 回放）必须折叠出**完全相同**的快照。
///
/// 为什么这条必须存在：离线回放是「不烧 token 的回归手段」（读历史日志复现 UI 问题），
/// 一旦两条路径的语义分叉，回放出来的现象就与线上不一致——那会让所有离线排查都失去意义。
#[test]
fn online_and_wire_replay_agree() {
    let seqs = [
        frame("turn/start", 1, json!({ "turn": 1 })),
        frame("step/start", 2, json!({ "turn": 1, "step": 1 })),
        frame("user/message", 3, user_message("m-u1", "回放我")),
        frame(
            "assistant/message",
            4,
            assistant_message("m-a1", "回放结果", Some("想一想")),
        ),
        frame(
            "tool/call",
            5,
            json!({ "turn": 1, "step": 1, "callId": "c1", "name": "write_file", "arguments": "{}" }),
        ),
        frame("tool/result", 6, tool_result("c1", "写好了", false)),
        frame(
            "turn/end",
            7,
            json!({ "turn": 1, "reason": { "kind": "completed" } }),
        ),
        status_frame("idle"),
    ];

    // 在线路径：通知 → 折叠。
    let mut online = Folder::new();
    for notif in &seqs {
        feed(&mut online, notif);
    }

    // 离线路径：把同样的帧包成线级记录行 → `push_wire_line`。
    let mut replay = Folder::new();
    for notif in &seqs {
        let event_type = notif.params["event"]["type"].as_str();
        let line = json!({
            "cat": "dsh",
            "kind": "notification",
            "method": notif.method,
            "eventType": event_type,
            "raw": { "jsonrpc": "2.0", "method": notif.method, "params": notif.params },
        })
        .to_string();
        replay
            .push_wire_line(&line)
            .unwrap_or_else(|e| panic!("回放这一行应成功：{e}\n{line}"));
    }

    assert_eq!(
        online.snapshot(),
        replay.snapshot(),
        "在线与回放的快照必须逐字段相同（同源同巡）"
    );
}

/// 回放只认线级通知行：app 轨迹、请求/响应行必须被跳过（而不是报错）。
#[test]
fn replay_skips_non_notification_lines() {
    let mut folder = Folder::new();
    let app = json!({ "cat": "app", "kind": "recorder.opened", "data": { "path": "x.jsonl" } });
    folder
        .push_wire_line(&app.to_string())
        .expect("app 轨迹应跳过");
    let request = json!({ "cat": "dsh", "kind": "request", "method": "initialize" });
    folder
        .push_wire_line(&request.to_string())
        .expect("请求行应跳过");

    let snap = folder.snapshot();
    assert!(snap.messages.is_empty());
    assert_eq!(snap.stats.turns, 0);

    // 非 JSON 行必须报错（别静默吞掉损坏的日志）。
    assert!(folder.push_wire_line("{不是 JSON").is_err());
}

/// 落库保真（用户 2026-09-29 要求「细致到每轮对话，能记多少记多少」）：
/// 每行都要带 turn/step/source，注入类消息与未提交的尝试也要成行，工具失败要带**原因**。
#[test]
fn rows_carry_turn_step_source_and_failures() {
    let mut folder = Folder::new();
    feed(&mut folder, &frame("turn/start", 1, json!({ "turn": 3 })));
    feed(
        &mut folder,
        &frame("step/start", 2, json!({ "turn": 3, "step": 2 })),
    );
    // 程序化注入（官方把 runtime-context 写成 role=user 的输入）。
    feed(
        &mut folder,
        &frame(
            "user/message",
            3,
            json!({
                "id": "m-inj",
                "role": "user",
                "content": [{ "type": "text", "text": "沙箱策略：workspace-write" }],
                "source": { "kind": "runtime-context", "form": "snapshot", "sections": [] },
            }),
        ),
    );
    // 人类输入。
    feed(
        &mut folder,
        &frame("user/message", 4, user_message("m-u1", "你好")),
    );
    // 未提交的模型尝试（只有 stream，没有 message）。
    feed(
        &mut folder,
        &frame(
            "assistant/attempt",
            5,
            json!({
                "turn": 3,
                "step": 2,
                "stream": [{ "type": "text-chunks", "time0": TIME_BASE + 5, "index": 0, "dt": [0], "texts": ["嗯"] }],
            }),
        ),
    );
    // 工具失败：带原因与 meta（含 diffs 正文）。
    feed(
        &mut folder,
        &frame(
            "tool/call",
            6,
            json!({ "turn": 3, "step": 2, "callId": "c1", "name": "run", "arguments": "{}" }),
        ),
    );
    feed(
        &mut folder,
        &frame(
            "tool/result",
            7,
            json!({
                "turn": 3,
                "step": 2,
                "message": {
                    "id": "m-t",
                    "role": "tool",
                    "content": [{ "type": "tool-result", "toolCallId": "c1", "content": [], "isError": true }],
                    "source": { "kind": "tool", "callId": "c1" },
                },
                "error": { "name": "ToolError", "code": "EPERM", "reason": "权限不足" },
                "meta": { "diffs": [{ "path": "a.rs", "oldText": "x", "newText": "x\ny" }] },
            }),
        ),
    );

    let snap = folder.snapshot();
    let kinds: Vec<MsgKind> = snap.messages.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MsgKind::Injected,
            MsgKind::User,
            MsgKind::Attempt,
            MsgKind::Tool
        ],
        "注入与尝试都要成行（**显示策略归 UI**，记录必须全）"
    );
    assert_eq!(snap.messages[0].source, "runtime-context", "来源要落库");
    assert_eq!(snap.messages[1].source, "user");
    assert_eq!(snap.messages[2].source, "model");
    for m in &snap.messages {
        assert_eq!(m.turn, Some(3), "每行都要有轮归属：{m:?}");
        assert_eq!(m.step, Some(2), "每行都要有步归属：{m:?}");
    }
    assert_eq!(
        snap.messages[2].stream.expect("尝试应带流摘要").chunks,
        1,
        "未提交的尝试也要留下 token/流证据"
    );

    let tool = snap.messages[3].tool.as_ref().expect("工具卡");
    assert_eq!(tool.error.as_deref(), Some("权限不足"), "失败原因要留下");
    assert_eq!(
        snap.messages[3].error.as_deref(),
        Some("权限不足"),
        "行级 error 与卡片一致（导出/查询只认一列）"
    );
    assert!(
        tool.meta.is_some(),
        "原样 meta 要留（库里存全文，不止行数摘要）"
    );
    assert_eq!(tool.diffs[0].added, 2, "diffs 仍按行数折叠（meta 的投影）");

    // 统计口径不变：只有 User/Assistant 占「消息数」（注入/尝试不算对话消息）。
    assert_eq!(snap.stats.messages, 1);
}

/// 重试折成 Notice，并且**带失败原因**（用户要「失败原因也记」）。
#[test]
fn retry_notice_carries_failure_reason() {
    let mut folder = Folder::new();
    feed(
        &mut folder,
        &frame(
            "llm/retry",
            1,
            json!({
                "mode": "normal",
                "retryId": "r1",
                "turn": 1,
                "step": 1,
                "provider": "deepseek-official",
                "policyKey": "k",
                "retry": 1,
                "maxRetries": 5,
                "delayMs": 2000,
                "failure": { "message": "上游 503", "code": "HTTP_503" },
            }),
        ),
    );
    let snap = folder.snapshot();
    let row = &snap.messages[0];
    assert_eq!(row.kind, MsgKind::Notice);
    assert!(row.text.contains("重试"), "文案：{}", row.text);
    assert_eq!(
        row.error.as_deref(),
        Some("HTTP_503: 上游 503"),
        "失败原因要单独一列（可筛可导出）"
    );
}

/// 等待可见：`request/header` / `request/context` 必须折进快照。
///
/// 为什么这条是契约而不是细节：发完 prompt 到首个模型产出之间可以安静几十秒，
/// 而 engine 只在**快照有变化**时发事件。这两个事件此前都折成 `{}` → 快照无变化 → UI 收不到
/// 任何东西（用户看到「点了发送之后一片静止」，分不清在等模型还是卡死了）。
/// 折进来之后，「等待」这段没有事件的时间在数据层就有迹可循。
#[test]
fn model_request_is_visible_in_snapshot() {
    let mut folder = Folder::new();

    // 路由元数据（provider/model/上下文窗口）：不随每次请求重发，来了就留住。
    feed(
        &mut folder,
        &frame(
            "request/context",
            1,
            json!({
                "provider": "deepseek-official",
                "model": "deepseek-v4-flash",
                "contextWindow": 128000,
            }),
        ),
    );
    let snap = folder.snapshot();
    assert_eq!(
        snap.last_request.provider.as_deref(),
        Some("deepseek-official")
    );
    assert_eq!(
        snap.last_request.model.as_deref(),
        Some("deepseek-v4-flash")
    );
    assert_eq!(snap.last_request.context_window, Some(128_000));
    assert_eq!(
        snap.last_request.started_at, None,
        "只收到路由元数据时不能假装「正在等待」"
    );

    // 请求发起（request/header）：起点时刻 / seq / 原因 / 工具数。
    feed(
        &mut folder,
        &frame(
            "request/header",
            2,
            json!({
                "header": { "config": {}, "tools": [{ "name": "read" }, { "name": "edit" }] },
                "reason": "initial",
            }),
        ),
    );
    let snap = folder.snapshot();
    assert_eq!(snap.last_request.started_at, Some(TIME_BASE + 2));
    assert_eq!(snap.last_request.seq, Some(2));
    assert_eq!(snap.last_request.reason.as_deref(), Some("initial"));
    assert_eq!(snap.last_request.tools, Some(2));

    // 后续请求只更新「本次请求」的字段：路由元数据留着，工具数按本次如实重算。
    feed(
        &mut folder,
        &frame(
            "request/header",
            5,
            json!({ "header": { "config": {} }, "reason": "series" }),
        ),
    );
    let snap = folder.snapshot();
    assert_eq!(snap.last_request.seq, Some(5));
    assert_eq!(snap.last_request.reason.as_deref(), Some("series"));
    assert_eq!(snap.last_request.started_at, Some(TIME_BASE + 5));
    assert_eq!(
        snap.last_request.tools, None,
        "本次请求头没带 tools → 留 None（不能沿用上一次的值，否则误导）"
    );
    assert_eq!(
        snap.last_request.model.as_deref(),
        Some("deepseek-v4-flash"),
        "路由元数据属于会话级，不该被请求级更新清掉"
    );
}

/// 空快照的基线：`Folder::new()` 产出的快照没有会话 id、没有状态——engine 靠它判断「有没有接过通知」。
#[test]
fn empty_snapshot_baseline() {
    let snap: SessionSnapshot = Folder::new().snapshot();
    assert!(snap.session_id.is_empty());
    assert_eq!(snap.status, None);
    assert!(snap.messages.is_empty());
    assert_eq!(snap.stats.turns, 0);
}
