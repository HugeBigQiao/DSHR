//! dshr-state 契约测试：engine 主链路（注入假 driver，不起进程、不烧 token）。
//!
//! 依据：`../DESIGN.md` §12.4「重建优先级」第 1 条——engine 主链路
//!（prompt → 通知 → 折叠 → 落库）、多 runtime 路由隔离、stderr / 退村落盘。
//! 本 crate 的测试约定与接缝清单见 [`_conventions.md`](_conventions.md)。
//!
//! 为什么全部用注入式假 driver：engine 的价值在「按会话路由 + 脏检测 + 落库」，
//! 这些必须能**不起 node 子进程**地验证；真实会话（烧 token）在 `engine_session.rs`，
//! 需 `DSHR_LIVE=1`。
//!
//! 上接：无（测试入口）。
//! 下接：`dshr_state::engine`（`Engine` / `EngineCmd` / `EngineEvent`）、
//!       `dshr_state::raw`（`SessionDriver` 接缝、`RuntimeEvent` 事实）、
//!       `dshr_state::store`（落库断言）、`dsh_sdk_protocol`（帧形状）。
//!
//! 三条铁律（`_conventions.md` 记过、重建时踩过）：
//! 1. `assistant/message` 帧缺必填字段**不报错**，只会静默降级成 `Unknown`
//!    （表现是「事件没反应、等超时」）→ 本文件保留 `frame_shape_self_check` 专门钉形状；
//! 2. 假 driver 自己持有广播发送端克隆，**drop 测试手里的发送端关不掉流** →
//!    模拟进程退出只能用 `mark_runtime_exited_for_test`；
//! 3. 一条测试只验一件事（测试助手的推进循环有副作用，会互相吃掉状态）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use dsh_sdk_client::client::Error as ClientError;
use dsh_sdk_client::process::RuntimeStatus;
use dsh_sdk_protocol::content_block::ContentBlock;
use dsh_sdk_protocol::notifications::{self, Kind, SessionStatus};
use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SdkPromptContentBlock, ServerInfo, SessionPromptParams,
    SessionPromptResult,
};
use dsh_sdk_protocol::rpc::Notification;
use dsh_sdk_protocol::session_event::SessionEvent;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

use dshr_state::engine::{Engine, EngineCmd, EngineEvent, RuntimeId, SessionId};
use dshr_state::raw::SessionDriver;
use dshr_state::raw::driver::BoxFuture;
use dshr_state::snapshot::{MsgKind, SessionSnapshot};
use dshr_state::store::{SessionSummary, Store};

/// 通道容量（与 UI 总线同规格）。
const CAP: usize = 64;

/// 单步超时：本文件全部走内存与假通道，毫秒级就该有结果。
const STEP: Duration = Duration::from_secs(5);

/// 事件时间基准（固定值 → 快照可逐字段断言，不依赖真实时钟）。
const TIME_BASE: u64 = 1_700_000_000_000;

// ——————————————————————————— 假 driver ———————————————————————————

/// 假 driver：只回 `Ok(...)`，把通知流的发送端交给测试用来造事件。
///
/// 为什么需要它（而不是直接用 `HarnessClient`）：`HarnessClient` 唯一能碰进程与管道，
/// 用它做测试就等于每次 `cargo test` 都拉一个 node 子进程——`SessionDriver` 抽象的价值
/// 正是在这里兑现（见 `DESIGN.md` §3.5）。
struct FakeDriver {
    /// 通知流发送端（测试侧也持有一份克隆，用来推事件）。
    tx: broadcast::Sender<Notification>,
    /// 进程退出状态（无人设置 → 默认未退出；退出场景走 `mark_exited_for_test`）。
    status: Arc<RuntimeStatus>,
    /// 收到的 prompt 参数（断言「命令真的走到了进程层」）。
    prompts: Arc<Mutex<Vec<SessionPromptParams>>>,
}

impl SessionDriver for FakeDriver {
    fn initialize<'a>(
        &'a mut self,
        _params: &'a InitializeParams,
    ) -> BoxFuture<'a, Result<InitializeResult, ClientError>> {
        Box::pin(async move {
            Ok(InitializeResult {
                server_info: ServerInfo {
                    name: "fake-runtime".to_string(),
                    // 官方硬编码 '0.0.1'（不可用于版本校验，见 DESIGN §9.1）。
                    version: "0.0.1".to_string(),
                },
            })
        })
    }

    fn prompt<'a>(
        &'a mut self,
        params: &'a SessionPromptParams,
    ) -> BoxFuture<'a, Result<SessionPromptResult, ClientError>> {
        let params = params.clone();
        let prompts = self.prompts.clone();
        Box::pin(async move {
            let message_id = format!("m-{}", params.session_id);
            prompts.lock().expect("prompt 记录锁").push(params);
            Ok(SessionPromptResult { message_id })
        })
    }

    fn events(&mut self) -> broadcast::Receiver<Notification> {
        self.tx.subscribe()
    }

    fn stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        // 注入路径的 stderr 由 `Runtime::inject_stderr_for_test` 单独提供；
        // 这里给一个已关闭的空通道（`with_driver_for_test` 不会取用它）。
        let (tx, rx) = mpsc::unbounded_channel();
        drop(tx);
        rx
    }

    fn runtime_status(&mut self) -> Arc<RuntimeStatus> {
        self.status.clone()
    }

    fn shutdown(&mut self) -> BoxFuture<'_, Result<(), ClientError>> {
        Box::pin(async { Ok(()) })
    }
}

// ——————————————————————————— 装载与推进 ———————————————————————————

/// 起一个 engine（内存库，不落真实文件）。
fn boot() -> (Engine, mpsc::Sender<EngineCmd>) {
    let (tx, rx) = mpsc::channel(CAP);
    let mut engine = Engine::new(rx);
    engine.with_db(Store::open_in_memory().expect("内存库应能打开"));
    (engine, tx)
}

/// 注入一个「已就绪」的 runtime（不 spawn 进程），返回通知发送端与 prompt 记录。
fn inject(
    engine: &mut Engine,
    id: &str,
    session: &str,
) -> (
    broadcast::Sender<Notification>,
    Arc<Mutex<Vec<SessionPromptParams>>>,
) {
    let (tx, rx) = broadcast::channel(CAP);
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let driver: Box<dyn SessionDriver> = Box::new(FakeDriver {
        tx: tx.clone(),
        status: Arc::new(RuntimeStatus::default()),
        prompts: prompts.clone(),
    });
    engine.register_injected_runtime(RuntimeId::new(id), driver, SessionId::new(session), rx);
    (tx, prompts)
}

/// 同上，但额外注入 stderr 流（验证「stderr → `runtime_logs` 表」整条链路）。
fn inject_with_stderr(
    engine: &mut Engine,
    id: &str,
    session: &str,
) -> (
    broadcast::Sender<Notification>,
    mpsc::UnboundedSender<String>,
) {
    let (tx, rx) = broadcast::channel(CAP);
    let (stderr_tx, stderr_rx) = mpsc::unbounded_channel();
    let driver: Box<dyn SessionDriver> = Box::new(FakeDriver {
        tx: tx.clone(),
        status: Arc::new(RuntimeStatus::default()),
        prompts: Arc::new(Mutex::new(Vec::new())),
    });
    engine.register_injected_runtime_with_stderr(
        RuntimeId::new(id),
        driver,
        SessionId::new(session),
        rx,
        Some(stderr_rx),
    );
    (tx, stderr_tx)
}

/// 推进一轮事件循环（超时即失败：说明 engine 卡住或该醒没醒）。
async fn step(engine: &mut Engine) -> Vec<EngineEvent> {
    match tokio::time::timeout(STEP, engine.next()).await {
        Ok(Some(evs)) => evs,
        Ok(None) => panic!("命令通道意外关闭"),
        Err(_) => panic!("单步超时 {STEP:?}：engine 未产出任何事件"),
    }
}

/// 发一条命令并推进一轮（`send().await` 返回即入队，故 `try_recv` 必能取到）。
async fn send(
    engine: &mut Engine,
    cmds: &mpsc::Sender<EngineCmd>,
    cmd: EngineCmd,
) -> Vec<EngineEvent> {
    cmds.send(cmd).await.expect("命令通道应可用");
    step(engine).await
}

/// 从一批事件里取某个会话的快照。
fn snapshot_of<'a>(evs: &'a [EngineEvent], session: &str) -> Option<&'a SessionSnapshot> {
    evs.iter().find_map(|e| match e {
        EngineEvent::Snapshot {
            session: s,
            snapshot,
            ..
        } if s.as_str() == session => Some(&**snapshot),
        _ => None,
    })
}

/// 取某个会话在库里的聚合行。
fn summary_of(engine: &Engine, session: &str) -> SessionSummary {
    let db = engine.store_ref().expect("已注入内存库");
    db.session_summaries()
        .expect("聚合查询不应失败")
        .into_iter()
        .find(|s| s.id == session)
        .unwrap_or_else(|| panic!("库里应有会话 {session} 的行"))
}

// ——————————————————————————— 帧构造 ———————————————————————————

/// 一条 `session.event` 通知帧（wire 信封：type / seq / time / data）。
fn event_frame(session: &str, kind: &str, seq: u64, data: serde_json::Value) -> Notification {
    Notification {
        method: "session.event".to_string(),
        params: json!({
            "sessionId": session,
            "event": { "type": kind, "seq": seq, "time": TIME_BASE + seq, "data": data },
        }),
    }
}

/// 一条 `session.status` 通知帧。
fn status_frame(session: &str, status: &str) -> Notification {
    Notification {
        method: "session.status".to_string(),
        params: json!({ "sessionId": session, "status": status }),
    }
}

/// `assistant/message` 的 data（**必填字段齐全**：turn / step / message.id / role / content /
/// source，且 `source.kind = "model"` 必须带 provider 与 model——缺一个就静默降级）。
fn assistant_data(text: &str) -> serde_json::Value {
    json!({
        "turn": 1,
        "step": 1,
        "message": {
            "id": "m-a1",
            "role": "assistant",
            "content": [{ "type": "text", "text": text }],
            "source": { "kind": "model", "provider": "deepseek", "model": "deepseek-chat" },
        },
        "usage": {
            "inputTokens": 100,
            "outputTokens": 20,
            "totalTokens": 120,
            "reasoningTokens": 5,
        },
    })
}

/// 解析一条通知帧里的会话事件（形状不对时给出明确失败信息）。
fn parsed_event(n: &Notification) -> SessionEvent {
    match notifications::parse(n).expect("帧应能解析") {
        Some(Kind::SessionEvent(e)) => e.event,
        other => panic!("期望 session.event，实际 {other:?}"),
    }
}

// ——————————————————————————— 测试 ———————————————————————————

/// 帧形状自检：把本文件用到的帧逐条解析，断言 `event_type()` 等于预期字符串。
///
/// 为什么必须留着它：协议层的容错策略是「data 反序列化失败 → lossless 降级 `Unknown`」，
/// 于是**写错帧形状不会报错**，只会在上层表现为「事件没反应、测试等超时」。
/// 这条测试把形状问题钉在它自己的失败信息里（见 `_conventions.md` 坑 1）。
#[test]
fn frame_shape_self_check() {
    let cases = [
        (
            event_frame("s-1", "turn/start", 1, json!({ "turn": 1 })),
            "turn/start",
        ),
        (
            event_frame("s-1", "step/start", 2, json!({ "turn": 1, "step": 1 })),
            "step/start",
        ),
        (
            event_frame("s-1", "assistant/message", 3, assistant_data("回复")),
            "assistant/message",
        ),
        (
            event_frame(
                "s-1",
                "turn/end",
                4,
                json!({ "turn": 1, "reason": { "kind": "completed" } }),
            ),
            "turn/end",
        ),
    ];
    for (frame, expected) in cases {
        let ev = parsed_event(&frame);
        assert_eq!(
            ev.event_type(),
            expected,
            "帧形状与预期不符（多半是少了必填字段 → 静默降级成 Unknown）：{frame:?}"
        );
    }

    // 未知类型必须 lossless 降级（官方加事件不该炸消费侧）。
    let unknown = parsed_event(&event_frame("s-1", "future/thing", 9, json!({ "x": 1 })));
    assert_eq!(
        unknown.event_type(),
        "unknown",
        "未知事件应降级 Unknown 而非解析失败"
    );

    // session.status 走另一个变体。
    match notifications::parse(&status_frame("s-1", "running")).expect("应能解析") {
        Some(Kind::SessionStatus(n)) => {
            assert_eq!(n.session_id, "s-1");
            assert_eq!(n.status, SessionStatus::Running);
        }
        other => panic!("期望 session.status，实际 {other:?}"),
    }
}

/// 主链路：prompt → 通知 → 折叠 → 落库。
///
/// 这条覆盖 engine 的**全部核心职责**：命令路由到 raw（driver 真的收到文本）、
/// Fake 模式本地回显、脏检测（有变化才发快照）、折叠成快照、以及每次发快照前落库。
#[tokio::test]
async fn main_chain_prompt_fold_persist() {
    let (mut engine, cmds) = boot();
    let (events, prompts) = inject(&mut engine, "rt-main", "s-main");

    // —— 1. Prompt：文本必须真的走到 driver（trim 后）——
    let evs = send(
        &mut engine,
        &cmds,
        EngineCmd::Prompt {
            id: RuntimeId::new("rt-main"),
            session: SessionId::new("s-main"),
            text: "  你好，帮我看一下  ".to_string(),
        },
    )
    .await;

    let snap = snapshot_of(&evs, "s-main").expect("Prompt 应产出快照");
    assert_eq!(snap.session_id, "s-main");
    assert_eq!(snap.status, Some(SessionStatus::Running), "乐观 running");
    assert_eq!(snap.messages.len(), 1, "Fake 模式应本地回显一行用户消息");
    assert_eq!(snap.messages[0].kind, MsgKind::User);
    assert_eq!(snap.messages[0].text, "你好，帮我看一下", "应已 trim");

    {
        let seen = prompts.lock().expect("prompt 记录锁");
        assert_eq!(seen.len(), 1, "driver 应收到一次 prompt");
        assert_eq!(seen[0].session_id, "s-main", "sessionId 必须是命令里那个");
        match seen[0].content_blocks.as_slice() {
            [SdkPromptContentBlock::Block(ContentBlock::Text(t))] => {
                assert_eq!(t.text, "你好，帮我看一下");
            }
            other => panic!("期望单个文本块，实际 {other:?}"),
        }
    }

    // —— 2. 通知链：turn/start → step/start → assistant/message → turn/end → status idle ——
    let frames = [
        event_frame("s-main", "turn/start", 1, json!({ "turn": 1 })),
        event_frame("s-main", "step/start", 2, json!({ "turn": 1, "step": 1 })),
        event_frame(
            "s-main",
            "assistant/message",
            3,
            assistant_data("好的，我看一下。"),
        ),
        event_frame(
            "s-main",
            "turn/end",
            4,
            json!({ "turn": 1, "reason": { "kind": "completed" } }),
        ),
        status_frame("s-main", "idle"),
    ];
    let mut last: Option<SessionSnapshot> = None;
    for frame in frames {
        events.send(frame).expect("engine 侧接收端应在");
        if let Some(s) = snapshot_of(&step(&mut engine).await, "s-main") {
            last = Some(s.clone());
        }
    }

    let snap = last.expect("通知链应产出快照");
    assert_eq!(snap.status, Some(SessionStatus::Idle));
    assert_eq!(snap.stats.turns, 1, "turn/start + turn/end 应结算一轮");
    assert_eq!(snap.stats.steps, 1);
    assert_eq!(snap.stats.messages, 2, "user + assistant 两行占消息数");
    assert_eq!(snap.stats.usage.input, 100);
    assert_eq!(snap.stats.usage.output, 20);
    assert_eq!(snap.stats.usage.reasoning, 5);
    let assistant = snap
        .messages
        .iter()
        .find(|m| m.kind == MsgKind::Assistant)
        .expect("应有 assistant 行");
    assert_eq!(assistant.text, "好的，我看一下。");
    assert_eq!(assistant.seq, 3, "seq 取自事件信封");

    // —— 3. 落库：快照发出去之前必须已经持久（先落库再发）——
    let sum = summary_of(&engine, "s-main");
    assert_eq!(sum.turns, 1);
    assert_eq!(
        sum.tokens, 125,
        "六桶中五项之和（input 100 + output 20 + reasoning 5）"
    );
    assert_eq!(sum.status.as_deref(), Some("idle"));
    assert_eq!(sum.last_seq, 3, "增量书签 = 最大消息 seq");
}

/// 多 runtime 路由隔离：通知只落到它自己的会话，未知会话的通知被丢弃。
#[tokio::test]
async fn multi_runtime_routing_isolation() {
    let (mut engine, _cmds) = boot();
    let (ev_a, _) = inject(&mut engine, "rt-a", "s-a");
    let (ev_b, _) = inject(&mut engine, "rt-b", "s-b");

    // —— B 的通知只应产出 B 的快照 ——
    ev_b.send(event_frame(
        "s-b",
        "assistant/message",
        1,
        assistant_data("B 的回复"),
    ))
    .expect("engine 侧接收端应在");
    let evs = step(&mut engine).await;
    assert!(
        !evs.iter().any(
            |e| matches!(e, EngineEvent::Snapshot { session, .. } if session.as_str() == "s-a")
        ),
        "A 不该收到 B 的通知：{evs:?}"
    );
    let (id, snap) = evs
        .iter()
        .find_map(|e| match e {
            EngineEvent::Snapshot {
                id,
                session,
                snapshot,
            } if session.as_str() == "s-b" => Some((id, &**snapshot)),
            _ => None,
        })
        .expect("B 应产出快照");
    assert_eq!(id.as_str(), "rt-b", "快照事件必须带 runtime 标");
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.messages[0].text, "B 的回复");

    // —— 不属于任何已知会话的通知：丢弃（不出事件、不报错）——
    ev_a.send(event_frame(
        "s-ghost",
        "assistant/message",
        1,
        assistant_data("幽灵"),
    ))
    .expect("engine 侧接收端应在");
    assert!(step(&mut engine).await.is_empty(), "未知会话的通知应被丢弃");

    // —— A 自己的通知只落到 A ——
    ev_a.send(event_frame(
        "s-a",
        "assistant/message",
        1,
        assistant_data("A 的回复"),
    ))
    .expect("engine 侧接收端应在");
    let evs = step(&mut engine).await;
    let snap = snapshot_of(&evs, "s-a").expect("A 应产出快照");
    assert_eq!(snap.messages.len(), 1, "A 只应有自己那一条：{snap:?}");
    assert_eq!(snap.messages[0].text, "A 的回复");
    assert!(
        summary_of(&engine, "s-b").turns == 0 && summary_of(&engine, "s-a").turns == 0,
        "两个会话都不该凭空多出轮次"
    );
}

/// stderr 落盘：进程信号（不走协议通知）必须进 `runtime_logs` 表。
///
/// 为什么值得一条测试：`runtime_logs` 表建成后曾长期**没有写入方**——
/// 这种「表建了没人写」在代码表面完全看不出来，只有查询才会暴露（见 `_conventions.md`）。
#[tokio::test]
async fn stderr_lines_land_in_runtime_logs() {
    let (mut engine, _cmds) = boot();
    let (_events, stderr_tx) = inject_with_stderr(&mut engine, "rt-err", "s-err");

    stderr_tx
        .send("第一行现场".to_string())
        .expect("engine 侧接收端应在");
    stderr_tx
        .send("第二行现场".to_string())
        .expect("engine 侧接收端应在");

    // stderr 是进程级事实，不进会话折叠管道 → 每轮只消费不产事件。
    assert!(step(&mut engine).await.is_empty(), "stderr 不该产 UI 事件");
    assert!(step(&mut engine).await.is_empty());

    let db = engine.store_ref().expect("已注入内存库");
    assert_eq!(
        db.runtime_log_count("rt-err").expect("查询不应失败"),
        2,
        "两行 stderr 都应落进 runtime_logs（按 runtime id 归位）"
    );
}

/// 进程退出：报 `Failed`、收尾落库、并把 runtime 从注册表摘掉。
#[tokio::test]
async fn runtime_exit_reports_failed_and_flushes() {
    let (mut engine, cmds) = boot();
    let (_events, _prompts) = inject(&mut engine, "rt-die", "s-die");

    // 先造一点状态（有消息才有「收尾落库」的内容）。
    send(
        &mut engine,
        &cmds,
        EngineCmd::Prompt {
            id: RuntimeId::new("rt-die"),
            session: SessionId::new("s-die"),
            text: "在吗".to_string(),
        },
    )
    .await;

    // 模拟进程退出（不能靠 drop 发送端：假 driver 自己持有一份克隆）。
    engine.mark_runtime_exited_for_test(&RuntimeId::new("rt-die"));
    let evs = step(&mut engine).await;

    let reason = evs
        .iter()
        .find_map(|e| match e {
            EngineEvent::Failed { id, reason } if id.as_str() == "rt-die" => Some(reason.clone()),
            _ => None,
        })
        .expect("进程退出应报 Failed");
    assert!(
        reason.contains("退出"),
        "失败原因应说明是进程退出：{reason}"
    );

    // 收尾落库：会话最终态必须已经在库里（flush 即使无变化也写一次）。
    // `last_seq` = 快照内最大消息 seq，非 0 即证明那条本地回显的用户行也落了库。
    let sum = summary_of(&engine, "s-die");
    assert_eq!(
        sum.status.as_deref(),
        Some("running"),
        "收尾时状态停在最后已知态"
    );
    assert!(
        sum.last_seq >= 1,
        "最后一条快照（含用户行）应已落库：{sum:?}"
    );

    // runtime 已被摘掉：再等一轮不会重复报错，也不会永久挂住。
    engine.mark_runtime_exited_for_test(&RuntimeId::new("rt-die"));
    let again = tokio::time::timeout(Duration::from_millis(200), engine.next()).await;
    assert!(
        again.is_err(),
        "runtime 摘除后应回到「等命令」而不是空转产出事件"
    );
}

/// 没有任何 runtime 时，engine 必须**等命令**，而不是立刻返回空批。
///
/// 为什么这条是契约而不是细节：总线循环（`dshr-ui/src/bridge.rs`）是
/// `loop { engine.next().await ... }`——只要 `next()` 立刻返回，循环就空转，
/// 在 iced 的执行器线程上烧掉一个核，而**界面上没有任何异常**。
/// 这正是本项目最贵的那类 bug（静默失败）的形态。
#[tokio::test]
async fn no_runtime_waits_for_command() {
    let (mut engine, _cmds) = boot();
    let r = tokio::time::timeout(Duration::from_millis(200), engine.next()).await;
    assert!(
        r.is_err(),
        "无 runtime 时 next() 立刻返回了（{r:?}）→ 总线会空转烧 CPU"
    );
}

/// 协议漂移可见：**已知类型解析失败** → app 轨迹留一条 `event.degraded`。
///
/// 为什么必须把这条钉住：协议层的容错策略是「data 解析失败 → lossless 降级 `Unknown`」，
/// 它不报错、不崩溃——2026-09-29 就是因为**没有回执**，直到人工扫日志才发现
/// `system/message` 全量降级（漏了 `system-prompt` 这个 kind，10 条消息只剩原始 JSON）。
/// 这条测试同时钉住另一半：**类型本身就未知**（插件自注册，merge-extensible 的预期行为）
/// 不该记进告警，否则正常演进会把真实漂移淹没。
#[tokio::test]
async fn degraded_events_are_recorded_in_app_trajectory() {
    let dir = std::env::temp_dir().join(format!("dshr-engine-degraded-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录应能创建");
    let log = dir.join("wire.jsonl");
    let _ = std::fs::remove_file(&log);

    let (mut engine, _cmds) = boot();
    engine
        .with_recorder_for_test(log.clone())
        .expect("记录器应能打开");
    let (events, _prompts) = inject(&mut engine, "rt-deg", "s-deg");

    // ① 已知类型（assistant/message）但 data 缺必填字段 → 解析失败 → 降级，要留痕。
    events
        .send(event_frame(
            "s-deg",
            "assistant/message",
            1,
            json!({ "turn": 1 }),
        ))
        .expect("engine 侧接收端应在");
    step(&mut engine).await;

    // ② 类型本身不在 59 种里 → 预期内，不留痕。
    events
        .send(event_frame("s-deg", "future/thing", 2, json!({ "x": 1 })))
        .expect("engine 侧接收端应在");
    step(&mut engine).await;

    let text = std::fs::read_to_string(&log).expect("记录文件应可读");
    let degraded: Vec<&str> = text
        .lines()
        .filter(|l| l.contains(r#""kind":"event.degraded""#))
        .collect();
    assert_eq!(
        degraded.len(),
        1,
        "只应对「已知类型解析失败」记一条（未知类型不算）——实际记录：\n{text}"
    );
    assert!(
        degraded[0].contains("assistant/message"),
        "回执要带原始事件类型（不然不知道漂移发生在哪个事件上）：{}",
        degraded[0]
    );
    assert!(
        degraded[0].contains("s-deg"),
        "回执要带会话 id：{}",
        degraded[0]
    );

    // 事件本身仍然 lossless 折叠（降级不等于丢弃）：快照里会有这条 Unknown 的足迹吗？
    // fold 对 Unknown 不产行，但会话仍在、状态仍可读——这里断言「没崩、没丢会话」。
    assert_eq!(
        summary_of(&engine, "s-deg").status.as_deref(),
        Some("idle"),
        "降级事件不该影响会话状态"
    );

    let _ = std::fs::remove_file(&log);
}
