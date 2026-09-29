//! engine 的**每会话**状态：折叠器 + 脏检测基准 + 落库基准。
//!
//! 主要用途：一个会话一份 [`SessionState`]——它把「事件流 → 快照 → 落库」这条链的
//! 全部可变状态圈在一处，使 `Engine` 只需按 id 取用，不必管理若干平行的 HashMap。
//! 为什么单独成文件：这是 engine 里**唯一有状态**的部分，而 `registry.rs` 关心的是
//! 「有哪些 runtime、命令怎么路由」。两者变化理由不同（前者随折叠/落库需求，
//! 后者随多 runtime 需求），放一起会让 300 行的 `registry.rs` 再长一截。
//!
//! 上接：`registry.rs`（`Engine` 持有 `HashMap<SessionId, SessionState>`）。
//! 下接：`crate::fold::Folder`（折叠）、`crate::store::Store`（落库）、
//!       `crate::raw::Runtime::synth_user_message`（Fake 模式补用户行）、
//!       `dsh_sdk_protocol::notifications`（通知解析）。
//! 官方对应：无（见 `engine.rs` 文件头）。

use dsh_sdk_protocol::notifications::{self, Kind, SessionStatusNotification};
use dsh_sdk_protocol::rpc::Notification;

use super::{SessionId, SessionStatus};
use crate::fold::Folder;
use crate::raw::Runtime;
use crate::snapshot::SessionSnapshot;
use crate::store::Store;

/// 一条「已知类型但 data 解析失败 → 降级 `Unknown`」的事件（协议漂移信号）。
///
/// 为什么需要它（而不是让降级继续静默）：`fallback::known()` 的容错是刻意的，
/// 但**没有回执**意味着「官方改了字段形状」与「一切正常」在界面上完全一样——
/// 2026-09-29 就是靠事后人工扫日志才发现 `system/message` 全量降级。
/// engine 拿到这个回执后写一条 app 轨迹（`event.degraded`，含原始 type 与 seq），
/// 漂移从此在 wire log 里可查、可统计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degraded {
    /// 官方事件类型串（`Unknown` 变体里原样保留的那个）。
    pub event_type: String,
    /// 事件 seq（钉住是哪一条）。
    pub seq: u64,
}

/// 本层持有的一个会话态：折叠器 + 脏检测基准 + 落库基准。
///
/// 为什么需要这个类型（而不是把字段摊在 `Engine` 里）：多会话后这些状态是**每会话一份**，
/// 摊平会退化成若干平行的 HashMap（`folders` / `last_sent` / `last_persisted`），
/// 彼此靠 session_id 对齐——那正是容易写错的地方。合成结构后「一个会话一份状态」在类型上成立。
#[derive(Debug)]
pub struct SessionState {
    /// 会话 id。
    id: SessionId,
    /// 纯投影折叠器（`fold` 层）。
    folder: Folder,
    /// 本地合成事件的 seq（Fake 模式回显用户行用）。
    seq: u64,
    /// 上次**发给 UI** 的快照（相同则不发，省掉无变化事件的开销）。
    last_sent: Option<SessionSnapshot>,
    /// 上次**落库**的快照（比 `last_sent` 更保守：落库要考虑 WAL 与事务成本，
    /// 但当前两者同步触发；分开留字段是为了将来做落库节流而不影响 UI 实时性）。
    last_persisted: Option<SessionSnapshot>,
}

impl SessionState {
    /// 建一个空会话态（尚未收到任何事件）。
    pub fn new(id: SessionId) -> Self {
        Self {
            id,
            folder: Folder::new(),
            seq: 0,
            last_sent: None,
            last_persisted: None,
        }
    }

    /// 会话 id。
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// 只读快照（不落库、不发事件；给「拉取式」读取用）。
    pub fn snapshot(&self) -> SessionSnapshot {
        self.folder.snapshot()
    }

    /// 清空折叠态（新建会话语义）。
    pub fn reset(&mut self) {
        self.folder = Folder::new();
        self.seq = 0;
        self.last_sent = None;
        self.last_persisted = None;
    }

    /// 置会话状态（乐观 `running` / 新会话 `idle`）。
    ///
    /// 走与 wire 同一条折叠路径（合成一条 `session.status` 通知），
    /// 保证「本地合成」与「真实通知」产出完全一致的折叠结果。
    pub fn set_status(&mut self, status: SessionStatus) {
        self.folder
            .push_notification(&Kind::SessionStatus(SessionStatusNotification {
                session_id: self.id.0.clone(),
                status,
            }));
    }

    /// 追加一条**本地**记录（宿主侧事实，wire 上没有对应事件）：如「发送失败：<原因>」。
    ///
    /// 为什么由 engine 主动写：请求根本没送到 runtime 时不会有任何 wire 事件替我们记，
    /// 而用户明确要求「哪怕对话发送失败了，失败原因也要记」——同时它还会跟着快照落库、发 UI，
    /// 于是「失败」既有可见的一行，也有可查的一列。
    pub fn push_local_notice(&mut self, text: String, error: Option<String>) {
        if self.id.as_str().is_empty() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.folder.push_local_notice(now, text, error);
    }

    /// 补一条本地用户消息行（Fake 模式专用，见 `Runtime::synth_user_message`）。
    pub fn push_local_user_message(&mut self, text: &str) {
        self.seq += 1;
        let ev = Runtime::synth_user_message(text, self.seq);
        self.folder.push_event(&ev);
    }

    /// 喂一条 runtime 通知。
    ///
    /// # 返回
    /// `Some(Degraded)` = 这条通知里出现了「**已知类型但 data 解析失败**」的事件
    /// （协议漂移信号，engine 据此写一条 app 轨迹）；`None` = 正常（含「类型本身就未知」）。
    ///
    /// 归属判断仍由 [`SessionState::owns`] 在 engine 侧做（避免对每个会话重复解析畸形通知）；
    /// 这里再做一次是保险：不属于本会话就不折叠。
    pub fn feed(&mut self, n: &Notification) -> Option<Degraded> {
        if n.method == "session.event" || n.method == "session.status" {
            let sid = n
                .params
                .get("sessionId")
                .and_then(serde_json::Value::as_str);
            if sid != Some(self.id.as_str()) {
                return None;
            }
        }
        match notifications::parse(n) {
            Ok(Some(Kind::SessionEvent(ev))) => {
                // 先取回执再折叠（`push_notification` 会消费掉 kind）。
                let degraded = ev.event.degraded_event().map(|(event_type, seq)| Degraded {
                    event_type: event_type.to_string(),
                    seq,
                });
                self.folder.push_notification(&Kind::SessionEvent(ev));
                degraded
            }
            Ok(Some(kind)) => {
                self.folder.push_notification(&kind);
                None
            }
            // Ok(None)：未知通知方法（协议演进跳过）；Err：内容畸形（跳过，wire 保真留 WireLog）。
            _ => None,
        }
    }

    /// 若快照有变化：先落库，再返回「要不要发给 UI」的快照。
    ///
    /// # 返回
    /// - `Some(snapshot)` = 有变化且已落库，engine 应发 `EngineEvent::Snapshot`；
    /// - `None` = 与上次相同（无变化不发，忽略性事件零 UI 开销）。
    ///
    /// # 为什么「先落库再发」
    /// UI 收到快照就意味着它看到的状态是持久的；反过来（先发后落）会让崩溃时
    /// UI 显示过一段不存在的历史。
    pub fn take_changed(&mut self, store: Option<&Store>) -> Option<SessionSnapshot> {
        let snap = self.folder.snapshot();
        if self.last_sent.as_ref() == Some(&snap) {
            return None;
        }
        // 落库失败不打断会话：打一行 stderr 继续（错误面留给监控页）。
        if let Some(db) = store {
            if let Err(e) = db.persist_snapshot(&snap) {
                eprintln!("[engine] 落库失败（忽略继续）：{e}");
            }
        }
        self.last_persisted = Some(snap.clone());
        self.last_sent = Some(snap.clone());
        Some(snap)
    }

    /// 收尾落库：把当前快照补一次 persist（即使与上次相同——把会话最终态落盘）。
    ///
    /// 为什么需要：`Stop`/异常退出时最后一次变化可能刚好被脏检测跳过，
    /// 但「会话已经结束」这个事实需要落盘。store 是整体替换语义，重复 persist 幂等。
    pub fn flush(&mut self, store: Option<&Store>) {
        if self.id.as_str().is_empty() {
            return;
        }
        let snap = self.folder.snapshot();
        if let Some(db) = store {
            if let Err(e) = db.persist_snapshot(&snap) {
                eprintln!("[engine] 收尾落库失败（忽略）：{e}");
            }
        }
        self.last_persisted = Some(snap);
    }

    /// 该通知是否引用了本会话（不折叠，只判断归属）。
    ///
    /// 为什么单独提供：engine 收到一条通知后需要先决定**分派给谁**；
    /// 若沿用 `feed` 试错（对每个会话都喂一遍），畸形通知会被重复解析多次。
    pub fn owns(&self, n: &Notification) -> bool {
        if n.method != "session.event" && n.method != "session.status" {
            return true; // 非会话类通知（subagent.*）不属于任何会话，由调用方另处理。
        }
        n.params
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|sid| sid == self.id.as_str())
    }
}
