//! 落盘数据导出（CSV）：纯函数契约 + 真跑入口。
//!
//! 为什么需要（用户 2026-09-29 提出）：库里的事实表（sessions/turns/tool_calls/file_ops/
//! runtime_logs）与 wire-log 里的逐条历史，目前在 UI 上还没有地方看（监控页未做）。
//! 先用 CSV 把它们「倒出来」看：同一组函数将来直接接监控页的历史导出，所以这里测的不是
//! 一个临时脚本，而是**监控页数据出口的契约**。
//!
//! 两种模式：
//! - 默认：纯函数契约（CSV 转义、库表覆盖、**跨会话回放分组**）——不碰真实数据、毫秒级；
//! - `DSHR_EXPORT=1`：真跑一次全量导出到 `data/exports/<epoch>-<pid>/`，
//!   打印每个文件的行数与几条样本（要看数据就加这个环境变量）。
//!
//! ```bash
//! cargo test -p dshr-state --test export_csv                        # 契约
//! $env:DSHR_EXPORT='1'; cargo test -p dshr-state --test export_csv -- --nocapture   # 真跑导出
//! ```
//!
//! 上接：无（测试入口）。
//! 下接：`dshr_state::export`（导出函数）、`dshr_state::store`（事实表）、
//!       `dshr_state::fold::Folder`（回放折叠）。
//! 官方对应：无（dsh 自己的会话日志是 `.jsonl.zstd`，与这里的加工库导出不同用途）。

use std::path::PathBuf;

use dsh_sdk_protocol::notifications::SessionStatus;
use serde_json::json;

use dshr_state::export::{self, CsvFile};
use dshr_state::snapshot::{
    FileDiff, MsgItem, MsgKind, RequestView, SessionSnapshot, SessionStats, ToolItem, TurnStat,
    UsageAgg,
};
use dshr_state::store::Store;

/// 事件时间基准（与其它测试一致，便于跨文件对照）。
const TIME_BASE: u64 = 1_700_000_000_000;

// ————————————————————— 契约（默认跑） —————————————————————

/// CSV 转义：含逗号/引号/换行的字段必须按 RFC 4180 加引号，且能被 Excel 正确读回。
#[test]
fn csv_escaping_follows_rfc4180() {
    let file = CsvFile {
        name: "t".to_string(),
        headers: vec!["plain".to_string(), "tricky".to_string()],
        rows: vec![
            vec!["a".to_string(), "有,逗号".to_string()],
            vec!["b".to_string(), "有\"引号\"".to_string()],
            vec!["c".to_string(), "有\n换行".to_string()],
            vec!["d".to_string(), String::new()],
        ],
    };
    let csv = file.to_csv();
    let lines: Vec<&str> = csv.lines().collect();

    assert_eq!(lines[0], "plain,tricky");
    assert_eq!(lines[1], r#"a,"有,逗号""#);
    assert_eq!(
        lines[2], r#"b,"有""引号""""#,
        "内部引号要翻倍（否则 Excel 解析错位）"
    );
    // 含换行的字段会把一行拆成两行——这是 CSV 的固有行为，用引号包住即可。
    assert!(csv.contains("\"有\n换行\""));
    assert_eq!(file.row_count(), 4, "行数按数据行计（不含表头）");
}

/// 库表导出：六张事实表都出列，行数与落库一致（**包含还没写入方的两张**）。
#[test]
fn store_tables_export_every_fact_table() {
    let store = Store::open_in_memory().expect("内存库");
    store
        .persist_snapshot(&sample_snapshot("s-export"))
        .expect("落库");

    let files = export::store_tables(&store).expect("导出不应失败");
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "runtimes",
            "sessions",
            "messages",
            "turns",
            "tool_calls",
            "file_ops",
            "requests",
            "runtime_logs"
        ],
        "八张表都要导出（顺序固定，便于跨版本比对）"
    );
    let by = |name: &str| {
        files
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("应有 {name}"))
    };
    assert_eq!(
        by("messages").row_count(),
        2,
        "逐条消息也要能导出（用户要求落库细致到每轮对话）"
    );
    assert_eq!(by("sessions").row_count(), 1);
    assert_eq!(by("turns").row_count(), 1);
    assert_eq!(by("tool_calls").row_count(), 1);
    assert_eq!(by("file_ops").row_count(), 1, "diffs 应展开成文件变更行");
    assert_eq!(by("runtimes").row_count(), 0, "未注入 runtime 时为空表");
    assert_eq!(by("runtime_logs").row_count(), 0, "未注入 stderr 时为空表");
    assert!(
        by("turns").headers.contains(&"reason".to_string()),
        "表头取自主键/列名，不能丢"
    );
}

/// 跨会话回放：一个 wire-log 里混着两个会话的帧，必须**按会话分组**重建，不能混成一份。
///
/// 为什么这条是核心断言：`Folder` 是单会话状态机，直接顺序喂会把两个会话搅在一起
/// （消息交错、工具配对错乱）。分组是「跨会话历史」唯一正确的做法。
#[test]
fn replay_splits_by_session() {
    let dir = temp_dir("replay");
    std::fs::create_dir_all(&dir).expect("临时目录");
    let log = dir.join("1700000000000-1.jsonl");

    // 刻意交错：A 的用户消息、B 的用户消息、A 的助手消息、B 的助手消息。
    let lines = vec![
        event_line("s-a", "user/message", 1, user_data("m-a1", "A 的问题")),
        event_line("s-b", "user/message", 1, user_data("m-b1", "B 的问题")),
        event_line(
            "s-a",
            "assistant/message",
            2,
            assistant_data("m-a2", "A 的回答"),
        ),
        event_line(
            "s-b",
            "assistant/message",
            2,
            assistant_data("m-b2", "B 的回答"),
        ),
    ];
    // 夹入一行 app 轨迹（必须被跳过，不该影响任何会话）。
    let app = json!({ "cat": "app", "kind": "recorder.opened", "data": {} }).to_string();
    let mut body = lines.join("\n");
    body.push('\n');
    body.push_str(&app);
    body.push('\n');
    std::fs::write(&log, body).expect("写 wire-log");

    let sessions = export::replay_sessions(&dir).expect("回放不应失败");
    assert_eq!(sessions.len(), 2, "应认出两个会话：{sessions:?}");

    let a = &sessions["s-a"];
    assert_eq!(a.session_id, "s-a");
    assert_eq!(a.messages.len(), 2);
    assert_eq!(a.messages[0].text, "A 的问题");
    assert_eq!(a.messages[1].text, "A 的回答", "B 的消息不能串进 A");

    let b = &sessions["s-b"];
    assert_eq!(b.messages.len(), 2);
    assert_eq!(b.messages[0].text, "B 的问题");
    assert_eq!(b.messages[1].text, "B 的回答");

    // 会话历史导出：每个会话一份 messages + 一份 turns。
    let files = export::session_history(a);
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["s-a.messages", "s-a.turns"]);
    assert_eq!(files[0].row_count(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 全量导出：库表 + 会话历史都落到磁盘（`export_all` 的端到端契约）。
#[test]
fn export_all_writes_everything() {
    let dir = temp_dir("all");
    std::fs::create_dir_all(&dir).expect("临时目录");
    let wire = dir.join("wire-logs");
    std::fs::create_dir_all(&wire).expect("wire-logs");
    std::fs::write(
        wire.join("1700000000000-1.jsonl"),
        format!(
            "{}\n",
            event_line("s-1", "user/message", 1, user_data("m-1", "你好"))
        ),
    )
    .expect("写 wire-log");

    let store = Store::open_in_memory().expect("内存库");
    store
        .persist_snapshot(&sample_snapshot("s-1"))
        .expect("落库");

    let out = dir.join("exports");
    let report = export::export_all(Some(&store), &wire, &out).expect("导出不应失败");
    assert_eq!(report.sessions, 1, "一个会话");
    // 8 张库表 + 1 会话 × 2 份历史 = 10 个文件。
    assert_eq!(report.files.len(), 10, "{:?}", report.files);
    for (path, _) in &report.files {
        assert!(path.exists(), "应真的写出：{}", path.display());
        assert!(path.extension().is_some_and(|e| e == "csv"));
    }
    let messages = std::fs::read_to_string(out.join("s-1.messages.csv")).expect("读回 messages");
    assert!(
        messages.starts_with('\u{feff}'),
        "落盘的 CSV 要带 UTF-8 BOM（否则 Excel/PowerShell 会把中文读成乱码）"
    );
    assert!(
        messages.contains("seq,time,turn,step,kind,source,text,reasoning,error"),
        "表头应稳定（逐条对话 + 轮/步归属 + 失败原因）：{messages}"
    );
    assert!(messages.contains("你好"), "内容应可读：{messages}");

    let _ = std::fs::remove_dir_all(&dir);
}

// ————————————————————— 真跑（DSHR_EXPORT=1） —————————————————————

/// 把真实 `data/dshr.db` 与 `data/wire-logs/` 全量导出到 `data/exports/<epoch>-<pid>/`。
///
/// 门控理由：读的是用户真实数据（可能几 MB），不该进默认 `cargo test`；
/// 但它本身不烧 token、不起进程，所以随时可跑。
#[test]
fn export_real_data_when_requested() {
    if std::env::var("DSHR_EXPORT").is_err() {
        println!("（跳过真实导出：设 DSHR_EXPORT=1 才会跑）");
        return;
    }

    let db_path = dshr_state::store::default_db_path();
    let data_dir: PathBuf = db_path.parent().expect("data 目录").to_path_buf();
    let wire_dir = data_dir.join("wire-logs");
    let out = data_dir
        .join("exports")
        .join(format!("{}-{}", now_epoch_ms(), std::process::id()));

    let store = if db_path.exists() {
        Some(Store::open(&db_path).expect("打开真实库"))
    } else {
        println!("（没有 {}：库表部分跳过）", db_path.display());
        None
    };

    let report = export::export_all(store.as_ref(), &wire_dir, &out).expect("导出不应失败");
    println!("\n=== 导出到 {} ===", out.display());
    println!("回放出的会话数：{}", report.sessions);
    for (path, rows) in &report.files {
        println!("  {:>6} 行  {}", rows, path.display());
    }

    // 复原抽查：从库里读回一个会话（同时验证老库的列级迁移可读、这条路径真能用）。
    if let Some(store) = store.as_ref() {
        let ids = store.load_session_ids().expect("列会话");
        if let Some(first) = ids.first() {
            match store.load_snapshot(first).expect("读回快照") {
                Some(snap) => println!(
                    "复原抽查：{first} → 消息 {} 条 / 轮 {} / 标题 {:?} / plan={} sandbox={:?}",
                    snap.messages.len(),
                    snap.turns.len(),
                    snap.title,
                    snap.plan_mode,
                    snap.sandbox_mode
                ),
                None => println!("复原抽查：{first} 读回为 None（不该发生）"),
            }
        } else {
            println!("复原抽查：库里还没有会话（先跑一次 dshr-ui 会话）");
        }
    }

    // 至少要有库表文件；没有真实数据时给一句明确说明而不是静默通过。
    if report.files.is_empty() {
        println!("⚠️ 既没有库也没有 wire-log 可导（先跑一次 dshr-ui 会话就会有）");
    }
    // 抽一份会话历史的前几行打出来，省得再开文件（这是「用来看数据」的入口）。
    if let Some((path, _)) = report
        .files
        .iter()
        .find(|(p, _)| p.to_string_lossy().ends_with(".messages.csv"))
    {
        let text = std::fs::read_to_string(path).expect("读回历史");
        println!("\n--- {} 前 5 行 ---", path.display());
        for line in text.lines().take(5) {
            println!("{line}");
        }
    }
}

// ————————————————————— 构造与工具 —————————————————————

/// 临时目录（带进程号与用途后缀，避免并行测试互相踩）。
fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dshr-export-{}-{tag}-{}",
        std::process::id(),
        now_epoch_ms()
    ))
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 一行 wire-log 记录（`cat="dsh"` 的 `session.event` 通知）。
fn event_line(session: &str, kind: &str, seq: u64, data: serde_json::Value) -> String {
    json!({
        "cat": "dsh",
        "kind": "notification",
        "method": "session.event",
        "eventType": kind,
        "raw": {
            "jsonrpc": "2.0",
            "method": "session.event",
            "params": {
                "sessionId": session,
                "event": { "type": kind, "seq": seq, "time": TIME_BASE + seq, "data": data },
            },
        },
    })
    .to_string()
}

/// `user/message` 的 data（data 即 Message 本身；source 必须是 kind=user 才会折成 User 行）。
fn user_data(id: &str, text: &str) -> serde_json::Value {
    json!({
        "id": id,
        "role": "user",
        "content": [{ "type": "text", "text": text }],
        "source": { "kind": "user" },
    })
}

/// `assistant/message` 的 data（必填字段齐全，否则会静默降级成 Unknown）。
fn assistant_data(id: &str, text: &str) -> serde_json::Value {
    json!({
        "turn": 1,
        "step": 1,
        "message": {
            "id": id,
            "role": "assistant",
            "content": [{ "type": "text", "text": text }],
            "source": { "kind": "model", "provider": "deepseek-official", "model": "deepseek-chat" },
        },
    })
}

/// 一个可落库的完整快照（1 轮 / 2 消息 / 1 工具 / 1 个文件变更）。
fn sample_snapshot(session: &str) -> SessionSnapshot {
    SessionSnapshot {
        session_id: session.to_string(),
        title: Some("导出样本".to_string()),
        status: Some(SessionStatus::Idle),
        plan_mode: false,
        sandbox_mode: None,
        last_request: RequestView {
            started_at: Some(TIME_BASE + 1),
            seq: Some(1),
            reason: Some("initial".to_string()),
            tools: Some(12),
            provider: Some("deepseek-official".to_string()),
            model: Some("deepseek-chat".to_string()),
            context_window: Some(64_000),
        },
        messages: vec![
            MsgItem {
                kind: MsgKind::User,
                text: "帮我改一下".to_string(),
                reasoning: None,
                usage: None,
                stream: None,
                tool: None,
                time: TIME_BASE + 1,
                seq: 1,
                turn: Some(1),
                step: Some(1),
                source: "user".to_string(),
                error: None,
            },
            MsgItem {
                kind: MsgKind::Tool,
                text: String::new(),
                reasoning: None,
                usage: None,
                stream: None,
                tool: Some(ToolItem {
                    call_id: "c1".to_string(),
                    name: "edit_file".to_string(),
                    arguments: "{\"path\":\"src/lib.rs\"}".to_string(),
                    duration_ms: 12,
                    is_error: false,
                    result: Some("已改好".to_string()),
                    error: None,
                    meta: None,
                    diffs: vec![FileDiff {
                        path: "src/lib.rs".to_string(),
                        added: 3,
                        removed: 2,
                    }],
                }),
                time: TIME_BASE + 2,
                seq: 2,
                turn: Some(1),
                step: Some(1),
                source: "model".to_string(),
                error: None,
            },
        ],
        turns: vec![TurnStat {
            turn: 1,
            start_time: Some(TIME_BASE + 1),
            end_time: Some(TIME_BASE + 3),
            reason: Some("completed".to_string()),
            usage: UsageAgg {
                input: 100,
                output: 20,
                total: 120,
                ..Default::default()
            },
        }],
        stats: SessionStats {
            turns: 1,
            steps: 1,
            messages: 1,
            tool_calls: 1,
            usage: UsageAgg::default(),
            llm_ms: 0,
            tool_ms: 0,
            errors: 0,
        },
    }
}
