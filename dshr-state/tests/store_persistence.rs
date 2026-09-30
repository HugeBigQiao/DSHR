//! store 落库契约（`dshr-state` 契约测试 3/3）。
//!
//! 依据：`../DESIGN.md` §12.4「重建优先级」第 3 条（store 幂等）与 §8.2/§8.3（表集与统计域）。
//!
//! **落库语义 = 会话整体重放**：`persist_snapshot` 在一个事务里 UPSERT `sessions` +
//! DELETE+INSERT `turns` / `tool_calls` / `file_ops`。因此两条必须成立的契约是：
//! 1. **幂等**：同一个快照重复 persist，行数与聚合值不变（engine 每次快照变化都会 persist，
//!    收尾时还会再 flush 一次——不幂等就会重复计数）；
//! 2. **替换而非累加**：后一个快照是**全量视角**，它必须盖掉前一个（fold 每次从事件流重建全量，
//!    若落库是累加，删掉的轮会永远留在库里）。
//!
//! 为什么用内存库：`Store::open_in_memory()` 是公开 API，且不让测试碰 `data/dshr.db`
//!（那是用户真实数据）。
//!
//! 上接：无（测试入口）。
//! 下接：`dshr_state::store::{Store, SessionSummary}`、`dshr_state::snapshot`（快照类型）。

use dsh_sdk_protocol::llm::TokenUsage;
use dsh_sdk_protocol::notifications::SessionStatus;

use dshr_state::snapshot::{
    FileDiff, MsgItem, MsgKind, RequestView, SessionSnapshot, SessionStats, ToolItem, TurnStat,
    UsageAgg,
};
use dshr_state::store::Store;

/// 一条 User 消息行。
fn user_row(seq: u64, text: &str) -> MsgItem {
    MsgItem {
        kind: MsgKind::User,
        text: text.to_string(),
        reasoning: None,
        usage: None,
        stream: None,
        tool: None,
        time: 1_700_000_000_000 + seq,
        seq,
        turn: Some(1),
        step: Some(1),
        source: "user".to_string(),
        error: None,
    }
}

/// 一条 Tool 行（带卡片与逐文件 diff）。
fn tool_row(seq: u64, call_id: &str, name: &str, is_error: bool) -> MsgItem {
    MsgItem {
        kind: MsgKind::Tool,
        text: String::new(),
        reasoning: None,
        usage: None,
        stream: None,
        tool: Some(ToolItem {
            call_id: call_id.to_string(),
            name: name.to_string(),
            arguments: "{\"path\":\"src/lib.rs\"}".to_string(),
            duration_ms: 12,
            is_error,
            result: Some("已改好".to_string()),
            // 失败原因：即便这一次是成功的示例，也把字段填上以覆盖落库/复原往返。
            error: if is_error {
                Some("工具执行失败：permission denied".to_string())
            } else {
                None
            },
            // 原样 meta（库里存全文，diffs 只是它的投影——复原时由它重算，见 load_snapshot）。
            meta: Some(serde_json::json!({
                "diffs": [{ "path": "src/lib.rs", "oldText": "a\nb", "newText": "a\nb\nc" }],
            })),
            diffs: vec![FileDiff {
                path: "src/lib.rs".to_string(),
                added: 3,
                removed: 2,
            }],
        }),
        time: 1_700_000_000_000 + seq,
        seq,
        turn: Some(1),
        step: Some(1),
        source: "model".to_string(),
        // 行级 error 与卡片里的 error 由 fold 同时写入（`on_tool_result`），这里保持一致。
        error: if is_error {
            Some("工具执行失败：permission denied".to_string())
        } else {
            None
        },
    }
}

/// 一条 Assistant 行（带 token 账）。
fn assistant_row(seq: u64, text: &str, usage: TokenUsage) -> MsgItem {
    MsgItem {
        kind: MsgKind::Assistant,
        text: text.to_string(),
        reasoning: None,
        usage: Some(usage),
        stream: None,
        tool: None,
        time: 1_700_000_000_000 + seq,
        seq,
        turn: Some(1),
        step: Some(1),
        source: "model".to_string(),
        error: None,
    }
}

/// 单轮快照（1 轮 / 3 条消息 / 1 个工具 / 1 次工具错误）。
fn snapshot_v1(session: &str) -> SessionSnapshot {
    let usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 20,
        total_tokens: Some(120),
        cache_read_tokens: Some(5),
        cache_write_tokens: None,
        reasoning_tokens: Some(5),
    };
    let messages = vec![
        user_row(1, "帮我改一下"),
        assistant_row(2, "好的", usage.clone()),
        tool_row(3, "c1", "edit_file", true),
    ];
    let turn_usage = UsageAgg {
        input: 100,
        output: 20,
        cache_read: 5,
        reasoning: 5,
        total: 120,
        cache_write: 0,
    };
    SessionSnapshot {
        session_id: session.to_string(),
        title: Some("第一个会话".to_string()),
        status: Some(SessionStatus::Idle),
        stats: SessionStats {
            turns: 1,
            steps: 1,
            messages: 2,
            tool_calls: 1,
            usage: turn_usage.clone(),
            llm_ms: 0,
            tool_ms: 0,
            errors: 1,
        },
        // 复原要覆盖的会话级事实：模式开关与最近一次模型请求（含 provider/model）。
        plan_mode: true,
        sandbox_mode: Some("workspace-write".to_string()),
        last_request: RequestView {
            started_at: Some(1_700_000_000_001),
            seq: Some(1),
            reason: Some("initial".to_string()),
            tools: Some(12),
            provider: Some("deepseek-official".to_string()),
            model: Some("deepseek-chat".to_string()),
            context_window: Some(128_000),
        },
        turns: vec![TurnStat {
            turn: 1,
            start_time: Some(1_700_000_000_001),
            end_time: Some(1_700_000_000_003),
            reason: Some("completed".to_string()),
            usage: turn_usage,
        }],
        messages,
    }
}

/// 同一会话的**缩减**快照（0 轮 / 1 条消息 / 无工具）：用来验证「替换」语义。
fn snapshot_v2(session: &str) -> SessionSnapshot {
    SessionSnapshot {
        session_id: session.to_string(),
        title: Some("改名后的会话".to_string()),
        status: Some(SessionStatus::Running),
        stats: SessionStats {
            turns: 0,
            steps: 0,
            messages: 1,
            tool_calls: 0,
            usage: UsageAgg::default(),
            llm_ms: 0,
            tool_ms: 0,
            errors: 0,
        },
        last_request: Default::default(),
        plan_mode: false,
        sandbox_mode: None,
        turns: Vec::new(),
        messages: vec![user_row(9, "只剩这一条")],
    }
}

/// 取库里唯一那条摘要。
fn the_summary(store: &Store, session: &str) -> dshr_state::store::SessionSummary {
    let all = store.session_summaries().expect("聚合查询不应失败");
    assert_eq!(all.len(), 1, "只该有一个会话：{all:?}");
    assert_eq!(all[0].id, session);
    all[0].clone()
}

/// 幂等：同一快照 persist 两次，行数与聚合值都不变。
#[test]
fn persist_is_idempotent() {
    let store = Store::open_in_memory().expect("内存库");
    let snap = snapshot_v1("s-idem");

    store.persist_snapshot(&snap).expect("首次落库");
    let first = the_summary(&store, "s-idem");

    store
        .persist_snapshot(&snap)
        .expect("重复落库（engine 收尾还会 flush 一次）");
    let second = the_summary(&store, "s-idem");

    assert_eq!(first, second, "同一快照重复落库必须完全一致（不重复计数）");
    assert_eq!(second.turns, 1, "turns 是行数：重复落库不该变成 2");
    assert_eq!(second.tool_calls, 1, "tool_calls 同理");
    assert_eq!(second.last_seq, 3, "增量书签 = 最大消息 seq");
    assert_eq!(second.errors(), 1, "错误口径 = 轮级 + 工具级");
}

/// 替换语义：后一个快照盖掉前一个（不是累加），第 1 轮必须消失。
#[test]
fn persist_replaces_previous_content() {
    let store = Store::open_in_memory().expect("内存库");
    store
        .persist_snapshot(&snapshot_v1("s-repl"))
        .expect("先落 v1");
    store
        .persist_snapshot(&snapshot_v2("s-repl"))
        .expect("再落 v2");

    let sum = the_summary(&store, "s-repl");
    assert_eq!(sum.turns, 0, "v2 没有轮 → 旧的轮必须被删掉");
    assert_eq!(sum.tool_calls, 0, "v2 没有工具 → 旧的工具行必须被删掉");
    assert_eq!(sum.tokens, 0);
    assert_eq!(sum.last_seq, 9, "书签跟着 v2 走");
    assert_eq!(sum.status.as_deref(), Some("running"), "状态跟着 v2 走");
    assert_eq!(
        sum.title.as_deref(),
        Some("改名后的会话"),
        "标题是 UPSERT 更新的列"
    );
}

/// 空 `session_id` 必须被拒绝：它是 `events`/`turns` 的父键，落进库只会制造孤儿行。
#[test]
fn empty_session_id_is_rejected() {
    let store = Store::open_in_memory().expect("内存库");
    let mut snap = snapshot_v1("s-x");
    snap.session_id = String::new();

    let err = store.persist_snapshot(&snap).expect_err("空 id 必须报错");
    assert!(
        matches!(err, dshr_state::store::StoreError::InvalidSnapshot(_)),
        "错误类型应是 InvalidSnapshot：{err:?}"
    );
    assert!(
        store.session_summaries().expect("查询").is_empty(),
        "被拒绝的落库不该留下任何行"
    );
}

/// **复原往返**：`persist_snapshot` → `load_snapshot` 必须与原始快照**逐字段相等**。
///
/// 为什么这是核心契约（用户 2026-09-29 要求「会话跑到一定数量后关掉、打开还能复原记录」）：
/// 复原的正确判据不是「读回来有点像」，而是「读回来一样」——只要不等，就说明某个字段在
/// 落库或读回时丢了语义（本项目实测过的例子：把 adapter **未报**的 token 桶写成 0，
/// 读回就从「未知」变成「0」；所以六桶列可空，NULL 与 0 语义不同）。
#[test]
fn restore_roundtrip() {
    // 方法：只做「落一份 → 读回来」一个来回，然后**整体比较**（`assert_eq!` 比整个
    // `SessionSnapshot`），而不是挑几个字段断言。
    // 为什么要整体比：挑字段的写法只能证明「我想到的那些字段没丢」——而漏掉的恰好是没写进
    // 断言的那些（这个文件的 sample 快照刻意带上 error/meta/diffs/plan_mode 等边角字段）。
    // 目的：把「复原」的判据钉成「逐字段相等」，于是任何新增字段忘了落库/读回都会当场失败。
    let store = Store::open_in_memory().expect("内存库");
    let snap = snapshot_v1("s-restore");
    store.persist_snapshot(&snap).expect("落库");

    // 读回走的是 `Store::load_snapshot`（子表 + meta_json 拼装），而不是复用内存里的那份。
    let back = store
        .load_snapshot("s-restore")
        .expect("读回应成功")
        .expect("应有该会话");
    assert_eq!(
        back, snap,
        "复原必须逐字段相等（含 turn/step/source/error/meta/模式/请求）"
    );

    // 目录：复原列表按最后更新倒序（最近用过的在前）。
    store
        .persist_snapshot(&snapshot_v1("s-older"))
        .expect("再落一个");
    let ids = store.load_session_ids().expect("列会话");
    assert_eq!(ids.len(), 2, "{ids:?}");
}

/// 库里没有这个会话 → `None`（而不是造一个空快照出来，那会让 UI 以为会话存在）。
#[test]
fn restore_missing_session_is_none() {
    let store = Store::open_in_memory().expect("内存库");
    assert!(store.load_snapshot("s-不存在").expect("读回").is_none());
    assert!(store.load_session_ids().expect("列会话").is_empty());
}

/// 一个库可并存多个会话（多 runtime / 多会话是产品能力，库必须按 session_id 隔离）。
#[test]
fn sessions_are_isolated_in_one_db() {
    let store = Store::open_in_memory().expect("内存库");
    store.persist_snapshot(&snapshot_v1("s-a")).expect("落 s-a");
    store.persist_snapshot(&snapshot_v1("s-b")).expect("落 s-b");

    let all = store.session_summaries().expect("聚合查询不应失败");
    assert_eq!(all.len(), 2);
    for s in &all {
        assert_eq!(s.turns, 1, "两个会话各自的轮数独立：{s:?}");
        assert_eq!(s.tool_calls, 1);
    }

    // 再落一次 s-a（幂等），s-b 不受影响。
    store
        .persist_snapshot(&snapshot_v1("s-a"))
        .expect("重复落 s-a");
    let all = store.session_summaries().expect("聚合查询不应失败");
    assert_eq!(all.len(), 2, "重复落库不该产生新会话");
}
