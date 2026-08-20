//! 管道对话层：读循环 + id 配对 + 事件通道 + 请求方法。
//!
//! 只管"stdin/stdout 管道上的对话"——帧的形状（构造/判断/解析）在
//! `dshr_protocol::rpc`（纯函数），这里做 I/O 与配对。
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dshr_protocol::rpc::{self, Frame, Notification};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::Error;

/// 管道对话：发送请求并配对响应，通知进事件通道。
#[derive(Debug)]
pub struct Transport {
    stdin: ChildStdin,
    // 统一读循环：响应 → 配对挂起请求；通知 → 事件通道。
    _read_task: JoinHandle<()>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<String, Error>>>>>,
    /// 事件流（结构化通知帧）。state 从这里消费解析。
    events: mpsc::UnboundedReceiver<Notification>,
    next_id: u64,
}

impl Transport {
    /// 从管道启动对话（spawn 后台读循环）。
    pub fn start(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<String, Error>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_loop = pending.clone();
        let (events_tx, events_rx) = mpsc::unbounded_channel();

        let task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match rpc::classify(&line) {
                    Some(Frame::Response { id }) => {
                        if let Some(tx) = pending_loop.lock().unwrap().remove(&id) {
                            let _ = tx.send(Ok(line));
                        }
                    }
                    Some(Frame::Notification(notification)) => {
                        // 结构化通知帧；state 按 method 解析 params 成具体通知类型。
                        let _ = events_tx.send(notification);
                    }
                    None => eprintln!("[unparseable] {line}"),
                }
            }
            // EOF：runtime 已退出，失败所有仍在等待的请求。
            let leftover = std::mem::take(&mut *pending_loop.lock().unwrap());
            for (_, tx) in leftover {
                let _ = tx.send(Err(Error::RuntimeExited));
            }
        });

        Self {
            stdin,
            _read_task: task,
            pending,
            events: events_rx,
            next_id: 1,
        }
    }

    /// 发一个请求并等待配对响应：分配 id → 登记 pending → 构造信封 → 写 stdin → 等。
    /// `params` 为已序列化的 JSON（无 params 的请求传 `"{}"`）。
    pub async fn request(&mut self, method: &str, params: &str) -> Result<String, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = oneshot::channel();
        // 先登记再写：避免响应先到丢包。
        self.pending.lock().unwrap().insert(id, tx);

        let line = rpc::build_request(method, id, params);
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;

        rx.await.map_err(|_| Error::TransportClosed)?
    }

    /// 事件流接收端（结构化通知帧）。state 从这里消费。
    pub fn events(&mut self) -> &mut mpsc::UnboundedReceiver<Notification> {
        &mut self.events
    }
}
