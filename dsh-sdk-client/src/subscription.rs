//! 事件订阅：官方 client.ts 的 NotificationSubscription（subscribe / subscribeSessionTree）的 Rust 版。
//!
//! 主要用途：在广播事件流上提供「按会话树过滤」的消费端——`next()` / `try_next()` 两个
//! 取值入口，配合 `SessionTree` 把 `subagent.started` 血缘边在客户端扩成会话树。
//! 为什么需要：官方一个连接多订阅者、过滤在客户端做，所以「过滤」必须有独立类型承载；
//! 它同时是 run()（receipt-to-idle）的等待原语（先订阅再 prompt，避免漏回执），
//! 也是多会话 UI 只关心自己那棵树时的过滤点——放在 api.rs 里会让两个职责纠缠。
//! 上接：`dsh-sdk-client` 的 api.rs::run（`Subscription::scoped` + `next`）；
//!       `dshr-state` 的 raw 层（事件消费）。
//! 下接：`dsh_sdk_protocol::rpc::Notification`（原始帧，解析由调用方做）、
//!       tokio::sync::broadcast（底层通道）、`crate::error::Error`。
//!
//! 客户端侧过滤（官方同款）：会话树订阅按 subagent.started 血缘边在客户端扩展树，
//! 不依赖服务端做任何过滤。
//!
//! 官方对应：packages/sdk/client/src/client.ts 的 `NotificationSubscription` /
//! `NotificationSubscriptionImpl`（`next` / `tryNext`）与 `HarnessClient.subscribeSessionTree`。
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::broadcast;

use crate::error::Error;

/// 一个事件订阅：带可选会话树过滤的广播接收端。
///
/// 为什么需要：过滤状态（树）与通道游标必须一起移动——`next()` 每放行一条都可能
/// 改写树结构（血缘边），拆成两个参数传来传去极易漏掉一次吸收，故包成一个类型。
#[derive(Debug)]
pub struct Subscription {
    rx: broadcast::Receiver<Notification>,
    /// None = 全量；Some = 会话树内（root + subagent.started 血缘后代）。
    tree: Option<Arc<Mutex<SessionTree>>>,
}

/// 客户端侧会话树：root + 从 subagent.started 血缘边扩展出的后代。
///
/// 为什么需要：官方不做服务端过滤，客户端只能靠 `subagent.started` 的父子边自己长树；
/// 树只增不减（会话结束不影响归属），因此用 `HashSet` 无需额外遍历逻辑。
#[derive(Debug, Default)]
pub struct SessionTree {
    ids: HashSet<String>,
}

impl SessionTree {
    /// 以 root 会话建树。
    ///
    /// # 参数
    /// - `root`：根会话 id（调用方要观察的那棵树）。
    ///
    /// # 返回
    /// 只含 root 的树；后代靠 `absorb` 逐步长出来。
    pub fn new(root: &str) -> Self {
        let mut ids = HashSet::new();
        ids.insert(root.to_string());
        Self { ids }
    }

    /// 吸收一条 subagent.started 血缘边；父在树内时把子纳入并返回 true。
    ///
    /// # 参数
    /// - `parent` / `child`：血缘边的两端（来自通知的 parentSessionId / childSessionId）。
    ///
    /// # 返回
    /// `true` = 父在树内、已把子纳入；`false` = 父不在树内（与本次订阅无关的边，忽略）。
    ///
    /// 为什么需要：父子边**到达顺序不保证**（孙可能先于子在广播里出现），
    /// 故吸收必须是幂等且可重复的判断，而不是一次性建树。
    pub fn absorb(&mut self, parent: &str, child: &str) -> bool {
        if self.ids.contains(parent) {
            self.ids.insert(child.to_string());
            true
        } else {
            false
        }
    }

    /// 会话是否在树内。
    ///
    /// # 返回
    /// `true` = 该会话属于本订阅的会话树（根或已吸收的后代）。
    pub fn contains(&self, session_id: &str) -> bool {
        self.ids.contains(session_id)
    }
}

impl Subscription {
    /// 全量订阅。
    ///
    /// # 参数
    /// - `rx`：广播接收端（通常来自 `HarnessClient::subscribe`/`take_events`）。
    ///
    /// # 返回
    /// 不做任何过滤的订阅（所有通知放行）。
    pub fn new(rx: broadcast::Receiver<Notification>) -> Self {
        Self { rx, tree: None }
    }

    /// 会话树订阅（root + 血缘后代）。
    ///
    /// # 参数
    /// - `rx`：广播接收端；`root`：根会话 id。
    ///
    /// # 返回
    /// 只放行树内会话的 `session.event` / `session.status`（以及所有血缘边与其他通知，
    /// 见 `passes`）。
    ///
    /// 官方：client.ts 的 subscribeSessionTree。
    pub fn scoped(rx: broadcast::Receiver<Notification>, root: &str) -> Self {
        Self {
            rx,
            tree: Some(Arc::new(Mutex::new(SessionTree::new(root)))),
        }
    }

    /// 等一条通过过滤的通知（awaitable next）。
    ///
    /// # 返回
    /// 第一条通过过滤的 `Notification`（被过滤掉的会继续等，不返回）。
    ///
    /// # 错误
    /// 通道关闭（runtime 已退出 → 广播发送端全丢）时返回 `Error::TransportClosed`
    ///（此路径拿不到 exit code 与 stderr 尾部，故都是 None/空——完整信息在
    /// `RuntimeStatus` 或 `Error::TransportClosed` 的原始变体里）。
    ///
    /// 为什么需要：调用方（api::run）要的是「下一条我关心的事件」，
    /// 过滤循环不该在调用处重复实现。
    pub async fn next(&mut self) -> Result<Notification, Error> {
        loop {
            let n = self.rx.recv().await.map_err(|_| Error::TransportClosed {
                exit_code: None,
                stderr_tail: Vec::new(),
            })?;
            if self.passes(&n) {
                return Ok(n);
            }
        }
    }

    /// 非阻塞取一条（tryNext 同款）；Empty → Ok(None)。
    ///
    /// # 返回
    /// `Ok(Some(n))` = 立即可得且通过过滤；`Ok(None)` = 当前没有可取的（**不是**结束）。
    ///
    /// # 错误
    /// 通道关闭 → `Error::TransportClosed`。
    ///
    /// 注意：被广播丢弃（`Lagged`，消费太慢）时按官方 lagging 语义**跳过**继续取，
    /// 不报错——消费方应容忍事件缺口（WireLog 里有全量原文可回放）。
    pub fn try_next(&mut self) -> Result<Option<Notification>, Error> {
        loop {
            match self.rx.try_recv() {
                Ok(n) => {
                    if self.passes(&n) {
                        return Ok(Some(n));
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => return Ok(None),
                // 消费太慢被广播丢事件：跳过继续（官方 lagging 语义）。
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(broadcast::error::TryRecvError::Closed) => {
                    return Err(Error::TransportClosed {
                        exit_code: None,
                        stderr_tail: Vec::new(),
                    });
                }
            }
        }
    }

    /// 过滤：全量订阅直接放行；会话树订阅吸收血缘边，只放行树内会话的通知。
    fn passes(&mut self, n: &Notification) -> bool {
        let Some(tree) = &self.tree else {
            return true;
        };
        let mut tree = tree.lock().unwrap();
        match n.method.as_str() {
            // 血缘边：父在树内则把子纳入；边本身放行（run 需要它做树扩展）。
            "subagent.started" => {
                if let Some((parent, child)) = subagent_edge(&n.params) {
                    tree.absorb(&parent, &child);
                }
                true
            }
            // 会话树内的 session 事件/状态才放行。
            "session.event" | "session.status" => n
                .params
                .get("sessionId")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|sid| tree.contains(sid)),
            // 其他通知（subagent.finished 等）放行。
            _ => true,
        }
    }
}

/// 从 subagent.started 通知参数取血缘边 (parent, child)。
fn subagent_edge(params: &serde_json::Value) -> Option<(String, String)> {
    let parent = params.get("parentSessionId")?.as_str()?;
    let child = params.get("childSessionId")?.as_str()?;
    Some((parent.to_string(), child.to_string()))
}
