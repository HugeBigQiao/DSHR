//! 事件订阅：官方 client.ts 的 NotificationSubscription（subscribe / subscribeSessionTree）的 Rust 版。
//!
//! 客户端侧过滤（官方同款）：会话树订阅按 subagent.started 血缘边在客户端扩展树，
//! 不依赖服务端做任何过滤。
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::broadcast;

use crate::error::Error;

/// 一个事件订阅：带可选会话树过滤的广播接收端。
#[derive(Debug)]
pub struct Subscription {
    rx: broadcast::Receiver<Notification>,
    /// None = 全量；Some = 会话树内（root + subagent.started 血缘后代）。
    tree: Option<Arc<Mutex<SessionTree>>>,
}

/// 客户端侧会话树：root + 从 subagent.started 血缘边扩展出的后代。
#[derive(Debug, Default)]
pub struct SessionTree {
    ids: HashSet<String>,
}

impl SessionTree {
    /// 以 root 会话建树。
    pub fn new(root: &str) -> Self {
        let mut ids = HashSet::new();
        ids.insert(root.to_string());
        Self { ids }
    }

    /// 吸收一条 subagent.started 血缘边；父在树内时把子纳入并返回 true。
    pub fn absorb(&mut self, parent: &str, child: &str) -> bool {
        if self.ids.contains(parent) {
            self.ids.insert(child.to_string());
            true
        } else {
            false
        }
    }

    /// 会话是否在树内。
    pub fn contains(&self, session_id: &str) -> bool {
        self.ids.contains(session_id)
    }
}

impl Subscription {
    /// 全量订阅。
    pub fn new(rx: broadcast::Receiver<Notification>) -> Self {
        Self { rx, tree: None }
    }

    /// 会话树订阅（root + 血缘后代）。
    /// 官方：client.ts 的 subscribeSessionTree。
    pub fn scoped(rx: broadcast::Receiver<Notification>, root: &str) -> Self {
        Self {
            rx,
            tree: Some(Arc::new(Mutex::new(SessionTree::new(root)))),
        }
    }

    /// 等一条通过过滤的通知（awaitable next）。
    pub async fn next(&mut self) -> Result<Notification, Error> {
        loop {
            let n = self
                .rx
                .recv()
                .await
                .map_err(|_| Error::TransportClosed {
                    exit_code: None,
                    stderr_tail: Vec::new(),
                })?;
            if self.passes(&n) {
                return Ok(n);
            }
        }
    }

    /// 非阻塞取一条（tryNext 同款）；Empty → Ok(None)。
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
                    })
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
