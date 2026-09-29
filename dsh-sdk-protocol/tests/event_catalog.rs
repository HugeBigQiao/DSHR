//! 事件全集对账与降级策略（`dsh-sdk-protocol` 契约测试 1/2）。
//!
//! 覆盖 `tests/_conventions.md` 里的两条关注点：
//! 1. **事件全集对账**——`session_event` 判别枚举声明的类型集合必须与官方快照一一对应
//!   （协议漂移是本项目的头号风险）；
//! 2. **降级策略**——`Unknown` / `#[serde(other)]` 两层兜底各自守住什么、代价是什么。
//!
//! 与仓库脚本的分工（别互相替代）：
//! - `scripts/compare-session-events.mjs`：**对官方真源**做集合对账（需要 deepseek-harness 克隆）；
//! - 本文件：**对锁定的官方快照**（`fixtures/known-event-types.txt`）做集合对账，
//!   另外做形状级的降级行为断言——`cargo test` 就能跑，不需要官方克隆。
//!
//! 上接：无（测试入口）。
//! 下接：`dsh_sdk_protocol::session_event`（`SessionEvent` 与 `event_type()`）、
//!       `dsh_sdk_protocol::session_event::message`（`MessageRole`）、
//!       `dsh_sdk_protocol::session_event::turn`（`TurnEndReason` / `TurnEndCancelCause`）。

use serde_json::json;

use dsh_sdk_protocol::session_event::SessionEvent;

/// 官方快照里的 59 个事件类型（`fixtures/known-event-types.txt` 的机器可读版本）。
///
/// 为什么用 `include_str!` 而不是运行期读文件：编译期嵌入 → 不依赖测试的工作目录；
/// 且该文件被 cargo 记为依赖，改它必然触发重编译（不会出现「源码/快照改了但测试用旧的」）。
const PINNED_SNAPSHOT: &str = include_str!("fixtures/known-event-types.txt");

/// 判别枚举所在的源文件（解析 `#[serde(rename = "...")]` 用）。
const ENUM_SOURCE: &str = include_str!("../src/session_event.rs");

/// 官方 0.1.7-rc.2 的事件种数（数量变了就说明该同步文档与快照，而不是悄悄过）。
const PINNED_COUNT: usize = 59;

/// 从快照里取事件名（跳过 `#` 注释与空行）。
fn pinned_event_types() -> Vec<&'static str> {
    PINNED_SNAPSHOT
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// 从枚举源码里取所有 `#[serde(rename = "...")]` 的**事件**类型串。
///
/// 判据与仓库脚本一致：事件类型串一定含 `/`（数据枚举的 rename 如 `tool-call`
/// 不带斜杠，故被排除）。为什么扫源码而不是加一份 Rust 常量表：常量表是**第四个**
/// 维护点且不受编译期检查约束（枚举 + `fallback` 分发 + `meta` 四方法是既有三处，
/// 第三处靠穷尽匹配在编译期抓）；扫源码让「声明的集合」本身可断言，且不引入新维护点。
fn declared_event_types() -> Vec<&'static str> {
    const MARK: &str = "#[serde(rename = \"";
    let mut out = Vec::new();
    let mut rest = ENUM_SOURCE;
    while let Some(i) = rest.find(MARK) {
        rest = &rest[i + MARK.len()..];
        let Some(end) = rest.find('"') else { break };
        let name = &rest[..end];
        if name.contains('/') {
            out.push(name);
        }
        rest = &rest[end..];
    }
    out
}

/// 判别枚举声明的类型集合 == 锁定的官方快照，且无重复声明。
#[test]
fn enum_declares_exactly_the_pinned_official_set() {
    let mut declared = declared_event_types();
    declared.sort_unstable();
    let pinned = pinned_event_types();

    // 重复声明 = 分发时后一个永远不可达（`fallback::known` 的 match 从上往下命中）。
    let mut deduped = declared.clone();
    deduped.dedup();
    assert_eq!(
        declared.len(),
        deduped.len(),
        "有重复声明的类型：{declared:?}"
    );

    let missing: Vec<&str> = pinned
        .iter()
        .copied()
        .filter(|name| !deduped.contains(name))
        .collect();
    let extra: Vec<&str> = deduped
        .iter()
        .copied()
        .filter(|name| !pinned.contains(name))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "与官方快照不一致。\n  官方有、dshr 缺（需新增结构化变体）：{missing:?}\n  \
         dshr 有、官方已无（需移除或降级）：{extra:?}\n\
         同步步骤：① 改 `src/session_event.rs` 枚举变体 + `fallback.rs` 分发 + `meta.rs` 四个访问器；\
         ② 重跑 `node scripts/compare-session-events.mjs`（对官方真源）；\
         ③ 刷新 `tests/fixtures/known-event-types.txt` 与文档里的「59 种」。"
    );
    assert_eq!(
        pinned.len(),
        PINNED_COUNT,
        "官方快照的种数变了：同步完成后请一并更新 PINNED_COUNT 与 dshr/README.md、DESIGN.md §4.2"
    );
}

/// 真正未知的类型（插件自注册 / 新版新增）必须 lossless 降级，而不是解析失败。
#[test]
fn unknown_event_type_is_kept_verbatim() {
    let frame = json!({
        "type": "future/thing",
        "seq": 7,
        "time": 42,
        "data": { "a": 1, "nested": { "b": [1, 2] } },
    });
    let ev: SessionEvent = serde_json::from_value(frame).expect("未知类型也必须能解析");
    match &ev {
        SessionEvent::Unknown {
            event_type,
            seq,
            time,
            data,
            ..
        } => {
            assert_eq!(event_type, "future/thing", "原始 type 串必须保留");
            assert_eq!((*seq, *time), (7, 42), "信封字段必须保留");
            assert_eq!(
                data["nested"]["b"],
                json!([1, 2]),
                "data 必须无损保留（回放/落库都靠它）"
            );
        }
        other => panic!("期望 Unknown，实际 {}", other.event_type()),
    }
    // 占位串固定，落库/统计可用它识别「未结构化」。
    assert_eq!(ev.event_type(), "unknown");
    // 「类型未知」不是漂移（merge-extensible 的预期行为）→ 不该被当成降级报警。
    assert_eq!(ev.degraded_event(), None);
}

/// 降级要**可识别**：已知类型但 data 对不上 → `degraded_event()` 报出来。
///
/// 为什么单独一条：这正是 2026-09-29 漏掉 10 条消息的那条路径（`system/message` 因
/// `source.kind` 没建模而整条降级）。此前两条降级路径产生完全一样的 `Unknown`，
/// 宿主无从区分「官方改了字段」（要修）与「插件发了新事件」（正常）。
/// 现在 engine 靠这个回执写 `event.degraded` 轨迹（见 `dshr-state/tests/engine_flow.rs`）。
#[test]
fn known_type_with_mismatched_data_is_flagged_degraded() {
    // assistant/message 缺 step 与 message（必填）→ 解析失败 → 降级。
    let ev: SessionEvent = serde_json::from_value(json!({
        "type": "assistant/message",
        "seq": 42,
        "time": 7,
        "data": { "turn": 1 },
    }))
    .expect("解析本身不失败（降级是设计内的容错）");

    assert_eq!(ev.event_type(), "unknown", "降级后类型视图为占位串");
    assert_eq!(
        ev.degraded_event(),
        Some(("assistant/message", 42)),
        "必须报出「是哪个已知类型降级了」——否则只能靠人工扫日志"
    );

    // 反面对照：类型本身不在 59 种里 → 不算降级。
    let unknown: SessionEvent =
        serde_json::from_value(json!({ "type": "future/x", "seq": 1, "time": 2, "data": {} }))
            .expect("未知类型必须能解析");
    assert_eq!(unknown.degraded_event(), None);
}

/// merge-extensible 的联合类型带 `#[serde(other)]`：官方加值不该让整条事件降级。
#[test]
fn merge_extensible_enums_tolerate_new_values() {
    // TurnEndReason：官方 TurnEndReasonMap 注释明写 "Merge-extensible sum type"。
    let ev: SessionEvent = serde_json::from_value(json!({
        "type": "turn/end",
        "seq": 1,
        "time": 2,
        "data": { "turn": 3, "reason": { "kind": "future-kind" } },
    }))
    .expect("未知 kind 不该让事件解析失败");
    assert_eq!(
        ev.event_type(),
        "turn/end",
        "兜底后仍是类型化事件（轮结束这个骨架事实不能丢）"
    );

    // MessageRole：官方已从 3 成员扩到 5，仍可能继续加。
    let ev: SessionEvent = serde_json::from_value(json!({
        "type": "user/message",
        "seq": 1,
        "time": 2,
        "data": {
            "id": "m1",
            "role": "future-role",
            "content": [],
            "source": { "kind": "user" },
        },
    }))
    .expect("未知 role 不该让整条消息降级");
    assert_eq!(ev.event_type(), "user/message");
}

/// `TurnEndCancelCause` **故意没有** `#[serde(other)]`——这条测试把代价钉住。
///
/// 依据：官方 `packages/core/session/src/types.ts` 里 `TurnEndCancelCause = AgentCancelCause
/// | { kind: 'legacy' }` 是**普通联合**（只有 `TurnEndReasonMap` 被标注为 merge-extensible），
/// 所以 dshr 未给它加兜底。代价：官方真加一个新 cause 时，整条 `turn/end` 会降级 `Unknown`
/// ——丢的是「轮已结束」这个骨架事实（fold 里的那一轮会一直开着）。
///
/// 为什么要把这个代价写成测试：它是**已知取舍**而不是疏忽。若将来决定加 `#[serde(other)]`
/// 兜底，这条测试会失败——那时把它改成期望 `turn/end` 即可（说明兜底已生效）。
#[test]
fn unknown_turn_cancel_cause_degrades_the_whole_event() {
    let ev: SessionEvent = serde_json::from_value(json!({
        "type": "turn/end",
        "seq": 1,
        "time": 2,
        "data": {
            "turn": 3,
            "reason": { "kind": "aborted", "reason": { "kind": "future-cause" } },
        },
    }))
    .expect("解析本身不失败（降级是设计内的容错）");
    assert_eq!(
        ev.event_type(),
        "unknown",
        "已知 cause 之外的值会让整条 turn/end 降级（当前取舍，见本测试文档注释）"
    );
    // 好消息：这个取舍现在**可见**（engine 会记 `event.degraded`），不是静默的。
    assert_eq!(ev.degraded_event(), Some(("turn/end", 1)));
}
