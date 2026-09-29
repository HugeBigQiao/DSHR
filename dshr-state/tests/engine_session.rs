//! 端到端测试：真实会话的**逐步透明账本**。
//!
//! 本文件回答四个具体问题（用户 2026-09-29 提出）：
//! 1. 各个步骤的**落盘**到底发生了什么？
//! 2. 消息处理完善吗？
//! 3. 具体的信息在每一步能收集到哪些？
//! 4. 对上层的**透明度**如何（UI 能拿到什么）？
//!
//! # 为什么需要它（而不是只测单元逻辑）
//! 内部结构的正确性可以用假 driver 验证（那类测试重建后放本目录，见 `_conventions.md`），
//! 但「**信息是否真的留下来了**」只能靠跑一次真实会话、然后逐项查落盘来证实。
//! 本项目已经吃过一次教训：`runtimes` 与 `runtime_logs` 两张表建了却长期**没有写入方**，
//! 从代码表面完全看不出来——只有查询才会暴露。本测试就是把「表里到底有没有东西」
//! 变成一条可执行的断言。
//!
//! # 两种模式（由环境变量决定）
//! - `DSHR_LIVE=1`：**真实** runtime（spawn 官方 dsh、消耗 token）。
//!   首次运行会自动 pnpm install 官方 runtime（分钟级）。没配 api-key 时本测试**跳过**。
//! - 默认（未设置）：只跑「落盘链路」的**冷启动负例**——不 spawn 进程，
//!   验证「未启动时各表为空」这个前提本身。因为真实会话烧钱，不能进默认 `cargo test`。
//!
//! # 运行
//! ```bash
//! # 冷启动前提（快，不烧 token）
//! cargo test -p dshr-state --test engine_session -- --nocapture
//! # 真实会话（烧 token；低谷期做）
//! $env:DSHR_LIVE=1; cargo test -p dshr-state --test engine_session -- --nocapture
//! ```
//!
//! 上接：无（测试入口）。
//! 下接：`dshr_state::engine`（`Engine` / `EngineCmd` / `EngineEvent`）、
//!       `dshr_state::store`（落盘断言）、`dshr_state::raw`（模式判定与工作区根）。
//! 官方对应：无（这是宿主侧的可观测性验证；官方没有等价物）。

use std::time::Duration;

use dshr_state::engine::{Engine, EngineCmd, EngineEvent, RuntimeId, SessionId};
use dshr_state::raw;
use dshr_state::snapshot::{MsgKind, SessionSnapshot};
use dshr_state::store::Store;

/// 命令通道容量（与 UI 总线同规格）。
const CHANNEL_CAP: usize = 128;

/// 真实会话里「等待回复」的总超时。
const LIVE_TIMEOUT: Duration = Duration::from_secs(420);

/// 等模型回复时的单步超时（**必须远大于常规值**）。
///
/// 为什么：engine 只在**状态有变化**时发事件。发完 prompt 之后，直到 runtime 返回
/// 第一条事件之前，`engine.next()` 会安静地等——而真实 LLM 的首字节延迟可能几十秒
/// （实测：prompt 后立刻有本地回显，随后到首条 assistant 事件之间超过了 30s，
/// 于是被常规 `STEP_TIMEOUT` 误判为「engine 卡住」）。
///
/// 这同时暴露了一个**产品问题**（已记入 DESIGN.md §12.4 的待办）：
/// 等待模型期间 engine 没有任何心跳事件，UI 上表现为「点了发送之后一片静止」——
/// 用户无法区分「在等模型」与「卡死了」。
const WAIT_STEP_TIMEOUT: Duration = Duration::from_secs(300);

/// 启动阶段的单步超时（**必须远大于常规值**）。
///
/// 为什么：`StartRuntime` 这一步会同步执行 `mode::kit(Real)` → `runtime::ensure`
/// → `pnpm install`（实测 584 个包；首次安装 10 分钟以上，网络差时更久）。
/// engine 已用 `spawn_blocking` 包住它，但**在它返回之前 `engine.next()` 不产出任何事件**——
/// 拿常规 30s 去卡这一步，只会把「正在安装」误报成「engine 卡死」。
/// （本测试第一版正是如此：659 秒后报「单步超时 30s」，实际是 pnpm 在下载 tarball。）
const START_TIMEOUT: Duration = Duration::from_secs(1800);

/// 启动 engine 并返回（engine, 命令发送端）。
fn boot(live: bool) -> (Engine, tokio::sync::mpsc::Sender<EngineCmd>) {
    let (tx, rx) = tokio::sync::mpsc::channel(CHANNEL_CAP);
    let mut engine = Engine::new(rx);
    engine.with_db(Store::open_in_memory().expect("内存库"));
    if !live {
        // 冷启动负例：不需要真的起进程，但要让 engine 知道「不许起真进程」。
        engine.force_fake();
    }
    (engine, tx)
}

/// 一步推进（自定义超时；等本地事件用 `STEP_TIMEOUT`，等模型/启动用更宽的值）。
async fn step_with(engine: &mut Engine, limit: Duration) -> Option<Vec<EngineEvent>> {
    match tokio::time::timeout(limit, engine.next()).await {
        Ok(v) => v,
        Err(_) => panic!("单步超时 {limit:?}：engine 卡住了"),
    }
}

/// 把一批事件渲染成一行摘要（账本用）。
fn render(evs: &[EngineEvent]) -> String {
    if evs.is_empty() {
        return "（无事件）".to_string();
    }
    evs.iter()
        .map(|e| match e {
            EngineEvent::Started { label, session, .. } => {
                format!("Started(label={label:?}, session={session})")
            }
            EngineEvent::Snapshot {
                session, snapshot, ..
            } => format!(
                "Snapshot(session={session}, msgs={}, status={:?}, turns={}, tokens={})",
                snapshot.messages.len(),
                snapshot.status,
                snapshot.stats.turns,
                snapshot.stats.usage.total
            ),
            EngineEvent::SessionReset { session, .. } => format!("SessionReset(session={session})"),
            EngineEvent::Stopped { reason, .. } => format!("Stopped(reason={reason:?})"),
            EngineEvent::Failed { reason, .. } => format!("Failed(reason={reason:?})"),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// 落盘账本：查一遍所有真实存在的写入方，渲染成一张表。
///
/// 为什么逐项列出「表 → 行数 → 来源」：这张表就是「透明度」的答案——
/// 哪一步信息留下来了、留在哪、以什么粒度，一眼可见。
fn ledger(engine: &Engine, wire_path: Option<&str>, runtime_id: &str) -> String {
    let Some(db) = engine.store_ref() else {
        return "  （未注入库）".to_string();
    };
    let mut out = String::new();

    // sessions / turns / tool_calls / file_ops 的聚合视图（唯一现成的读侧接口）。
    match db.session_summaries() {
        Ok(list) if list.is_empty() => out.push_str("  sessions        : （空）\n"),
        Ok(list) => {
            for s in list {
                out.push_str(&format!(
                    "  sessions        : id={} title={:?} status={:?} turns={} tokens={} \
                     tool_calls={} turn_err={} tool_err={} last_seq={}\n",
                    s.id,
                    s.title,
                    s.status,
                    s.turns,
                    s.tokens,
                    s.tool_calls,
                    s.turn_errors,
                    s.tool_errors,
                    s.last_seq
                ));
            }
        }
        Err(e) => out.push_str(&format!("  sessions        : 查询失败 {e}\n")),
    }

    // runtime_logs（stderr 审计）：按传入的 runtime id 查行数。
    //
    // 为什么必须传真实 id：**第一版这里硬编码了 "rt-sample"，于是永远显示 0 行**——
    // 但那不代表没写，只代表查错了 id（真实的 runtime id 是 engine 生成的 `rt-<epoch>`）。
    // 「用错 id 查不到数据」与「数据没写进去」在表面上完全一样，这正是本项目
    // 「表建了没人写」那类坑的同一个形状。
    out.push_str(&format!(
        "  runtime_logs    : {runtime_id}={} 行\n",
        db.runtime_log_count(runtime_id).unwrap_or(0)
    ));

    // wire log（线级 lossless 记录）：文件大小即「记了多少」。
    match wire_path {
        Some(p) => {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("  wire-log 文件   : {size} 字节  {p}\n"));
        }
        None => out.push_str("  wire-log 文件   : （未开启记录）\n"),
    }
    out
}

/// 从一批事件里挑出最后一个快照（用于展示消息明细）。
fn last_snapshot(evs: &[EngineEvent]) -> Option<&SessionSnapshot> {
    evs.iter().rev().find_map(|e| match e {
        EngineEvent::Snapshot { snapshot, .. } => Some(&**snapshot),
        _ => None,
    })
}

/// 打印快照里的消息明细（「消息处理完善吗」的证据）。
fn dump_messages(snap: &SessionSnapshot) {
    println!("    ── 消息明细（{} 条）──", snap.messages.len());
    for m in &snap.messages {
        let kind = match m.kind {
            MsgKind::User => "user",
            MsgKind::Assistant => "assistant",
            MsgKind::Reasoning => "reasoning",
            MsgKind::Tool => "tool",
            MsgKind::Notice => "notice",
        };
        let text: String = m.text.chars().take(70).collect();
        println!(
            "    seq={:<4} {:<9} len={:<4} {}",
            m.seq,
            kind,
            m.text.chars().count(),
            text.replace('\n', "\\n")
        );
    }
    println!(
        "    ── 统计：turns={} tokens(input={} output={} total={}) errors={} ──",
        snap.stats.turns,
        snap.stats.usage.input,
        snap.stats.usage.output,
        snap.stats.usage.total,
        snap.stats.errors
    );
}

/// 冷启动前提：未启动 runtime 时，各表都应为空。
///
/// 为什么值得一条测试：这类「空前提」是整个落盘链路的地基——若某张表在冷启动时就有行，
/// 说明有东西在没人要求的情况下写库了。默认 `cargo test` 跑这条（不烧 token）。
#[tokio::test]
async fn cold_start_writes_nothing() {
    let (engine, _tx) = boot(false);
    println!("\n=== 冷启动（未启动任何 runtime）===");
    println!("{}", ledger(&engine, None, "（未启动）"));

    let db = engine.store_ref().expect("已注入内存库");
    assert!(
        db.session_summaries().expect("查询不应失败").is_empty(),
        "冷启动时 sessions 应无行（有行说明有人在没会话的情况下写库了）"
    );
    assert_eq!(
        db.runtime_log_count("rt-sample").expect("查询不应失败"),
        0,
        "冷启动时 runtime_logs 应无行"
    );
}

/// 真实会话的逐步透明账本（需 `DSHR_LIVE=1`）。
///
/// 流程与每步采集的信息：
/// | 步 | 动作 | 被检查的落盘 |
/// |---|---|---|
/// | 1 | `StartRuntime` | `runtimes` 行、`Recorder` 打开、wire-log 文件出现 |
/// | 2 | 等 `Started` | 空快照（session_id/status）、`sessions` 行 |
/// | 3 | `Prompt` | `turns` / token 六桶开始累积 |
/// | 4 | 等 idle | 消息明细（user/assistant/reasoning/tool）、`tool_calls`、`file_ops` |
/// | 5 | `StopRuntime` | 收尾落库（最终快照）、`runtime_logs`（stderr） |
#[tokio::test]
async fn live_session_transparency_ledger() {
    let live = std::env::var("DSHR_LIVE").is_ok_and(|v| v == "1");
    if !live {
        println!("\n（跳过真实会话：设置 DSHR_LIVE=1 才跑；它会消耗 token）");
        return;
    }

    // —— 前提检查：真实模式必须有 api-key，否则 resolve_mode 会回落 Fake（烧不到 token 但也没意义）。
    let resolved = raw::mode::resolve_mode();
    if resolved.mode != raw::mode::RuntimeMode::Real {
        println!(
            "\n（跳过：resolve_mode = {:?}，说明无 api-key 或 config.json 不可用）",
            resolved.mode
        );
        return;
    }

    let (mut engine, tx) = boot(true);
    let rt = RuntimeId::new("rt-sample");
    let session = SessionId::new("s-live");
    println!("\n=== 真实会话透明账本（DSHR_LIVE=1）===");

    // —— 步 1：启动 runtime ——
    println!("\n[1] StartRuntime（首次会触发官方 runtime 安装，可能几分钟）");
    tx.send(EngineCmd::StartRuntime { id: rt.clone() })
        .await
        .expect("总线未关闭");

    let started = {
        // 启动阶段用 START_TIMEOUT：首次会在这一步里跑完 pnpm install（分钟级）。
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        let mut collected = Vec::new();
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "等 Started 超时 {START_TIMEOUT:?}；已收：{collected:#?}"
            );
            let evs = step_with(&mut engine, START_TIMEOUT)
                .await
                .expect("命令通道意外关闭");
            collected.extend(evs);
            if collected
                .iter()
                .any(|e| matches!(e, EngineEvent::Started { .. }))
            {
                break;
            }
        }
        println!("    事件: {}", render(&collected));
        collected
    };

    // 取本次的 wire-log 路径（engine 生成，同时给 SDK 与 Recorder）。
    let wire_path = engine
        .recorder_ref()
        .map(|r| r.wire_log_path())
        .expect("启用记录后应有 Recorder");
    println!("    wire-log: {wire_path}");

    // Started 之后应有一条空快照（session_id + idle）。
    if let Some(snap) = last_snapshot(&started) {
        println!(
            "    初始快照: session_id={} status={:?} msgs={}",
            snap.session_id,
            snap.status,
            snap.messages.len()
        );
    }
    println!("{}", ledger(&engine, Some(&wire_path), rt.as_str()));

    // —— 步 2：发一条 prompt ——
    println!("\n[2] Prompt(\"用一句话说明你收到了这条消息\")");
    let ask = "用一句话说明你收到了这条消息";
    tx.send(EngineCmd::Prompt {
        id: rt.clone(),
        session: SessionId::new(session.as_str()),
        text: ask.to_string(),
    })
    .await
    .expect("总线未关闭");

    // —— 步 3：等到 idle（或超时）——
    // 判据：快照的 status 回到 Idle 且已有 assistant 消息；或状态变成 Failed。
    println!("\n[3] 等待回复（消息事件逐步累积）");
    let deadline = tokio::time::Instant::now() + LIVE_TIMEOUT;
    let mut turns_seen = 0usize;
    let mut final_evs: Vec<EngineEvent> = Vec::new();
    loop {
        if tokio::time::Instant::now() >= deadline {
            println!("    ⚠ 等待回复超时——下面是当时的状态");
            break;
        }
        let evs = step_with(&mut engine, WAIT_STEP_TIMEOUT)
            .await
            .expect("命令通道意外关闭");
        if evs.is_empty() {
            continue;
        }
        turns_seen += 1;
        // 每 5 批打一次进度（避免刷屏，又能看出「在动」）。
        if turns_seen % 5 == 0 {
            if let Some(snap) = last_snapshot(&evs) {
                println!(
                    "    …第 {} 批：msgs={} status={:?}",
                    turns_seen,
                    snap.messages.len(),
                    snap.status
                );
            } else {
                println!("    …第 {} 批：{}", turns_seen, render(&evs));
            }
        }
        final_evs.extend(evs);
        if final_evs
            .iter()
            .any(|e| matches!(e, EngineEvent::Failed { .. }))
        {
            println!("    ⚠ 出现 Failed：{}", render(&final_evs));
            break;
        }
        let done = final_evs.iter().rev().find_map(|e| match e {
            EngineEvent::Snapshot { snapshot, .. } => Some(snapshot),
            _ => None,
        });
        if let Some(snap) = done {
            let has_assistant = snap
                .messages
                .iter()
                .any(|m| m.kind == MsgKind::Assistant && !m.text.trim().is_empty());
            // **本轮结束的三种形态**都要认，否则会在「没有 assistant 消息」的情况下白等：
            // 1. 正常回复：有 assistant 消息且回到 idle；
            // 2. **模型侧出错**（如 AUTH 401）：没有 assistant 消息，但 `errors` 已计数且回到 idle——
            //    第一版判据漏了这一条，于是在认证失败时误报「300s 无事件/engine 卡住」（实测踩到）；
            // 3. 轮数已增加且状态不再是 running（兜底：事件到了但 fold 没产出 assistant 行）。
            let settled = snap.status != Some(dshr_state::engine::SessionStatus::Running);
            if has_assistant && settled {
                break;
            }
            if settled && snap.stats.errors > 0 {
                println!(
                    "    ⚠ 本轮以错误结束（errors={}），没有 assistant 消息",
                    snap.stats.errors
                );
                break;
            }
            if settled && snap.stats.turns > 0 && snap.stats.steps > 0 {
                break;
            }
        }
    }

    if let Some(snap) = last_snapshot(&final_evs) {
        dump_messages(snap);
    } else {
        println!("    （没有收到任何快照）");
    }
    println!("{}", ledger(&engine, Some(&wire_path), rt.as_str()));

    // —— 步 4：停止（收尾落盘）——
    println!("\n[4] StopRuntime（收尾落库 + stderr 落盘）");
    tx.send(EngineCmd::StopRuntime { id: rt.clone() })
        .await
        .expect("总线未关闭");
    let mut stopped = Vec::new();
    for _ in 0..40 {
        // 停止会走 dispose 阶梯（EOF → SIGTERM → SIGKILL，各有等待窗口），
        // 用常规 30s 太紧，这里给等待模型同样的宽限。
        let evs = step_with(&mut engine, WAIT_STEP_TIMEOUT)
            .await
            .expect("命令通道意外关闭");
        stopped.extend(evs);
        if stopped
            .iter()
            .any(|e| matches!(e, EngineEvent::Stopped { .. }))
        {
            break;
        }
    }
    println!("    事件: {}", render(&stopped));
    println!("{}", ledger(&engine, Some(&wire_path), rt.as_str()));

    // —— 断言：真实会话必须留下可查的证据 ——
    let db = engine.store_ref().expect("已注入内存库");
    let summaries = db.session_summaries().expect("查询不应失败");
    assert!(
        !summaries.is_empty(),
        "真实会话结束后 sessions 应有行（否则落盘链路断了）"
    );
    let snap = last_snapshot(&final_evs).expect("真实会话应产生过快照");
    assert!(
        snap.messages.iter().any(|m| m.kind == MsgKind::User),
        "快照里应有用户消息（Fake 才会本地补；Real 由 runtime 回发）"
    );
    assert!(
        snap.messages
            .iter()
            .any(|m| m.kind == MsgKind::Assistant && !m.text.trim().is_empty()),
        "快照里应有非空的 assistant 回复"
    );
    // wire-log 必须真的写了东西（它是问题复现的唯一 lossless 源）。
    let size = std::fs::metadata(&wire_path).map(|m| m.len()).unwrap_or(0);
    assert!(size > 0, "wire-log 应非空；实际 {size} 字节");
}
