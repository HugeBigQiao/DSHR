//! engine headless 集成测试（自 dshr-ui worker.rs 的 Machine 测试迁入 + s2 落库断言）：
//! 不依赖 GUI，直接驱动 Engine 跑 Fake runtime 全链路——Start → Started+空快照 →
//! Prompt → 本地回显用户行 → SDK 通知折叠 → assistant "hello from fake" + usage
//! 入桶 + idle → 落库断言（with_db 注入的内存库）→ Stop → Stopped。
//!
//! 语义与原 Machine 测试逐条对应（force_fake 接缝跳过 config.json 判定）；新增
//! 持久化接缝 `with_db(Store::open_in_memory())`，测试机不写默认 data/dshr.db。
//! 注意：WireLog 接线（data/wire-logs/<label>-<epoch>.jsonl）在测试里同样生效，
//! 每次本测试会给 workspace data/wire-logs/ 追加一条 fake 全程 JSONL
//! （data/ 已 .gitignore；如需完全隔离可后续给 Engine 加 workspace 注入接缝）。

use std::time::Duration;

use dsh_sdk_protocol::notifications::SessionStatus;
use tokio::sync::mpsc;

use super::{Engine, EngineCmd, EngineEvent};
use crate::snapshot::{MsgKind, SessionSnapshot};
use crate::store::Store;

/// 命令通道容量（与 UI 总线同规格）。
const CHANNEL_CAP: usize = 128;

/// 轮询 next() 直到出现满足 expect 的事件（超时 8s 防挂死；已收事件随 panic 打印）。
async fn poll_until(
    engine: &mut Engine,
    expect: impl Fn(&EngineEvent) -> bool,
) -> Vec<EngineEvent> {
    poll_until_long(engine, expect, 8).await
}

/// poll_until 的长超时变体（真实 API/首次装 runtime 用）。
async fn poll_until_long(
    engine: &mut Engine,
    expect: impl Fn(&EngineEvent) -> bool,
    secs: u64,
) -> Vec<EngineEvent> {
    let mut got = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if got.iter().any(&expect) {
            return got;
        }
        match tokio::time::timeout(Duration::from_millis(200), engine.next()).await {
            Ok(Some(events)) => got.extend(events),
            Ok(None) => panic!("命令通道提前关闭"),
            Err(_) => {} // 通知未到：继续等
        }
    }
    panic!("等待超时；已收事件：\n{got:#?}");
}

/// 发命令 + poll_until。
async fn drive(
    engine: &mut Engine,
    tx: &mut mpsc::Sender<EngineCmd>,
    cmd: EngineCmd,
    expect: impl Fn(&EngineEvent) -> bool,
) -> Vec<EngineEvent> {
    tx.send(cmd).await.expect("总线未关闭");
    poll_until(engine, expect).await
}

fn first_snapshot<'a>(events: &'a [EngineEvent]) -> Option<&'a SessionSnapshot> {
    events.iter().find_map(|e| match e {
        EngineEvent::Snapshot(s) => Some(s.as_ref()),
        _ => None,
    })
}

fn has_user(s: &SessionSnapshot, text: &str) -> bool {
    s.messages
        .iter()
        .any(|m| m.kind == MsgKind::User && m.text.trim() == text)
}

fn has_fake_reply(s: &SessionSnapshot) -> bool {
    s.messages
        .iter()
        .any(|m| m.kind == MsgKind::Assistant && m.text.contains("hello from fake"))
}

/// 全链路回归（原 fake_runtime_full_round_drives_machine 迁入 + 落库断言）：
/// Start → 空快照 → Prompt "hi" → 本地回显 + running → SDK 通知 → assistant
/// "hello from fake" + usage(input1/output2) + idle → 落库 1 行 → Stop → Stopped。
#[tokio::test]
async fn fake_runtime_full_round_drives_engine_and_persists() {
    let (mut tx, rx) = mpsc::channel(CHANNEL_CAP);
    let mut engine = Engine::new(rx);
    engine.force_fake();
    engine.with_db(Store::open_in_memory().expect("内存库"));

    // 1) Start：Started（Fake label）+ 空会话快照（idle）。
    let evs = drive(&mut engine, &mut tx, EngineCmd::Start, |e| {
        matches!(e, EngineEvent::Started { .. })
    })
    .await;
    assert!(
        evs.iter()
            .any(|e| matches!(e, EngineEvent::Started { label, .. } if label.contains("Fake"))),
        "Started 应带 Fake label"
    );
    let sid = engine.session_id.clone();
    assert!(evs.iter().any(|e| matches!(
        e, EngineEvent::Started { session_id, .. } if *session_id == sid
    )));
    let snap = first_snapshot(&evs).expect("Start 后应发空快照");
    assert_eq!(snap.status, Some(SessionStatus::Idle));
    assert!(snap.messages.is_empty());

    // 2) Prompt：命令批内先见本地回显用户行（fake 不回发 user/message）。
    let evs = drive(
        &mut engine,
        &mut tx,
        EngineCmd::Prompt {
            text: "hi".to_string(),
        },
        |e| matches!(e, EngineEvent::Snapshot(s) if has_user(s, "hi")),
    )
    .await;
    let snap = first_snapshot(&evs).expect("Prompt 后应发快照");
    assert_eq!(snap.status, Some(SessionStatus::Running));

    // 3) 等 SDK 通知流：assistant "hello from fake" + usage 入桶 + idle 回落。
    let evs = poll_until(&mut engine, |e| {
        matches!(e, EngineEvent::Snapshot(s) if has_fake_reply(s) && s.status == Some(SessionStatus::Idle))
    })
    .await;
    let snap = first_snapshot(&evs).expect("应收到含回复的快照");
    let reply = snap
        .messages
        .iter()
        .find(|m| m.kind == MsgKind::Assistant)
        .expect("应有一条 assistant 消息");
    assert!(
        reply.text.contains("hello from fake"),
        "回复文本：{}",
        reply.text
    );
    // fixture 的 assistant/message usage = input 1 / output 2。
    assert_eq!(snap.stats.usage.input, 1);
    assert_eq!(snap.stats.usage.output, 2);
    assert_eq!(snap.stats.messages, 2); // User + Assistant 各一

    // 4) 落库断言：注入的内存库已随「快照有变化即整发」同步 persist（s2 接线）。
    //    同一会话重复事件幂等：全程 UPSERT/替换，sessions 应只有 1 行。
    let db = engine.store.as_ref().expect("with_db 注入的内存库");
    let sums = db.session_summaries().expect("聚合查询");
    assert_eq!(sums.len(), 1, "同一会话应只有 1 行（替换语义）");
    let s = &sums[0];
    assert_eq!(s.id, sid, "落库会话 id = engine 生成的会话 id");
    assert_eq!(s.title, None); // fake runtime 无 session/title 事件
    assert_eq!(s.status.as_deref(), Some("idle"));
    assert_eq!(s.last_seq, 2); // 消息 seq：本地回显 1 + assistant 2
    // fake runtime 不产 turn/start..end → 轮表空；tokens 聚合只来自 turns 行，
    // 故 turns/tokens 均为 0——「正确」= 与 fold 语义一致（有轮事件时的求和已由
    // store.rs 自己的单测覆盖）。usage 在快照 stats（内存侧），落库侧无轮可归。
    assert_eq!(s.turns, 0);
    assert_eq!(s.tokens, 0);
    assert_eq!(s.tool_calls, 0);
    assert_eq!(s.errors(), 0);
    assert!(s.created_at <= s.updated_at && s.updated_at > 0);

    // 5) Stop：收尾落库（幂等：行数不变）→ 协议 shutdown → Stopped。
    let evs = drive(&mut engine, &mut tx, EngineCmd::Stop, |e| {
        matches!(e, EngineEvent::Stopped { .. })
    })
    .await;
    assert!(evs.iter().any(|e| matches!(e, EngineEvent::Stopped { .. })));
    let db = engine.store.as_ref().expect("注入的内存库仍在");
    let sums = db.session_summaries().expect("聚合查询");
    assert_eq!(sums.len(), 1, "Stop 收尾补 persist 后行数不变（幂等）");
    assert_eq!(sums[0].last_seq, 2);
    assert_eq!(sums[0].status.as_deref(), Some("idle"));
}

/// 命令通道关闭（App 退出）→ next() 返回 None → 总线流结束（bus 循环退出）。
#[tokio::test]
async fn engine_ends_when_command_channel_closes() {
    let (tx, rx) = mpsc::channel(CHANNEL_CAP);
    let mut engine = Engine::new(rx);
    engine.force_fake();
    drop(tx);
    assert!(engine.next().await.is_none(), "通道关闭后 next 应返回 None");
}

/// 真实 DeepSeek API 全链路（手动门控测试，不跑常规套件）：
/// 需要 workspace 根 config.json 配好 api-key 且可连 api.deepseek.com；首次运行会经
/// runtime::ensure 自动 pnpm 安装 dsh runtime（分钟级）。走 Real 分支（不 force_fake）：
/// 真 dsh → 真 API → 模型真实回复 → 折叠/落库（内存库断言）→ WireLog（data/wire-logs）。
/// 手动执行：cargo test -p dshr-state engine::tests::real_api_full_round -- --ignored
#[tokio::test]
#[ignore = "真实 API：需 config.json 配 api-key 与网络；手动 --ignored 执行"]
async fn real_api_full_round_drives_engine_and_persists() {
    let ws = super::mode::workspace_root();
    assert!(
        ws.join("config.json").exists(),
        "真实链路需要 DSHR/config.json（api-key/provider/model/dsh-version）"
    );
    let (mut tx, rx) = mpsc::channel(CHANNEL_CAP);
    let mut engine = Engine::new(rx);
    engine.with_db(Store::open_in_memory().expect("内存库"));
    // 不 force_fake：resolve_mode 读到 config.json 的 api-key → Real。

    // 1) Start（可能含首次 pnpm 安装 dsh runtime / sdk profile 准备，分钟级）。
    //    注意：不能复用 drive/poll_until——它们的 200ms 超时会反复取消正在长跑的
    //    next()（initialize 半途被 cancel 会留孤儿进程），这里单次长等待。
    tx.send(EngineCmd::Start).await.expect("总线未关闭");
    let evs = match tokio::time::timeout(Duration::from_secs(300), engine.next()).await {
        Ok(Some(evs)) => evs,
        Ok(None) => panic!("命令通道提前关闭"),
        Err(_) => panic!("Start（含 dsh 安装/初始化）300s 超时"),
    };
    assert!(
        !evs.iter().any(|e| matches!(e, EngineEvent::Failed { .. })),
        "Start 不应报错：{evs:#?}"
    );
    assert!(
        evs.iter().any(
            |e| matches!(e, EngineEvent::Started { label, .. } if label.contains("dsh runtime"))
        ),
        "Started 应带 dsh runtime label"
    );
    let sid = engine.session_id.clone();
    let snap = first_snapshot(&evs).expect("Start 后应发空快照");
    assert_eq!(snap.status, Some(SessionStatus::Idle));

    // 2) Prompt：真实 runtime 自己回发 user/message 与后续事件流。
    drive(
        &mut engine,
        &mut tx,
        EngineCmd::Prompt {
            text: "hi".to_string(),
        },
        |e| matches!(e, EngineEvent::Snapshot(s) if has_user(s, "hi")),
    )
    .await;

    // 3) 等真实模型回复（agent 一轮可能几十秒；title/子代理等事件照常折叠跳过）。
    let evs = poll_until_long(
        &mut engine,
        |e| {
            matches!(e, EngineEvent::Snapshot(s)
                if s.status == Some(SessionStatus::Idle)
                && s.messages.iter().any(|m| m.kind == MsgKind::Assistant && !m.text.trim().is_empty()))
        },
        300,
    )
    .await;
    let snap = evs
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Snapshot(s) => Some(s.as_ref()),
            _ => None,
        })
        .find(|s| {
            s.messages
                .iter()
                .any(|m| m.kind == MsgKind::Assistant && !m.text.trim().is_empty())
        })
        .expect("应收到含真实回复的快照");
    let reply = snap
        .messages
        .iter()
        .find(|m| m.kind == MsgKind::Assistant && !m.text.trim().is_empty())
        .expect("应有一条非空 assistant 消息");
    assert!(
        !reply.text.trim().is_empty(),
        "真实回复文本：{}",
        reply.text
    );
    // 真实 provider 会带 usage；至少输入侧应非零。
    assert!(
        snap.stats.usage.input > 0 || snap.stats.usage.total > 0,
        "真实请求应产生 token 用量（input={} total={}）",
        snap.stats.usage.input,
        snap.stats.usage.total
    );

    // 4) 落库断言：会话 1 行、状态 idle、last_seq 随消息推进；真实链路有 turn 事件。
    let db = engine.store.as_ref().expect("注入的内存库");
    let sums = db.session_summaries().expect("聚合查询");
    assert_eq!(sums.len(), 1, "同一会话应只有 1 行（替换语义）");
    assert_eq!(sums[0].id, sid);
    assert_eq!(sums[0].status.as_deref(), Some("idle"));
    assert!(sums[0].last_seq >= 2, "至少用户+助手各一事件");
    assert!(sums[0].turns >= 1, "真实 agent 一轮应产生 turn/start..end");

    // 5) Stop：收尾落库（幂等）→ 协议 shutdown → Stopped。
    let evs = drive(&mut engine, &mut tx, EngineCmd::Stop, |e| {
        matches!(e, EngineEvent::Stopped { .. })
    })
    .await;
    assert!(evs.iter().any(|e| matches!(e, EngineEvent::Stopped { .. })));
    let sums = engine
        .store
        .as_ref()
        .expect("注入的内存库仍在")
        .session_summaries()
        .expect("聚合查询");
    assert_eq!(sums.len(), 1, "Stop 收尾补 persist 后行数不变（幂等）");
}
