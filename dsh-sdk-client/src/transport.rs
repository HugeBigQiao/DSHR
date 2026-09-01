//! 管道对话层：读循环 + id 配对 + 事件通道 + 请求方法。
//!
//! 只管"stdin/stdout 管道上的对话"——帧的形状（构造/判断/解析）在
//! `dsh_sdk_protocol::rpc`（纯函数），这里做 I/O 与配对。
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use dsh_sdk_protocol::rpc::{self, Frame, Notification};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::error::Error;
use crate::process::RuntimeStatus;

/// 事件通道容量（广播）。最慢订阅者超限丢最旧事件（官方订阅模型同款 lagging 语义）。
const EVENT_CHANNEL_CAPACITY: usize = 4096;

/// 线级日志：全程落盘（JSONL），按类别分两路——
///   `cat="dsh"`：与 dsh 的线级交互（`{t, cat, dir, kind, id?/method?/eventType?, raw}`，
///                raw 是完整原始 JSON 帧；session.event 细到 eventType）；
///   `cat="app"`：应用自己的运行轨迹（`{t, cat, kind, data}`，由 state 层写入）。
/// 正式桌面端的排查/监管数据源；一个文件承载全部记录。
#[derive(Debug)]
pub struct WireLog {
    file: Mutex<std::fs::File>,
}

impl WireLog {
    /// 打开（追加模式；父目录需已存在）。
    pub fn open(path: &str) -> std::io::Result<Self> {
        Ok(Self {
            file: Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(path)?),
        })
    }

    /// 写一行 JSON（t = epoch ms）。
    fn write_line(&self, rec: serde_json::Map<String, serde_json::Value>) {
        let mut rec = rec;
        rec.insert(
            "t".to_string(),
            serde_json::Value::Number(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
                    .into(),
            ),
        );
        let line = serde_json::to_string(&serde_json::Value::Object(rec)).unwrap_or_default();
        let mut file = self.file.lock().unwrap();
        let _ = writeln!(file, "{line}");
    }

    /// 追加一条 dsh 线级记录。
    fn record_dsh(
        &self,
        dir: &str,
        kind: &str,
        extra: &serde_json::Map<String, serde_json::Value>,
        raw: &serde_json::Value,
    ) {
        let mut rec = serde_json::Map::new();
        rec.insert("cat".to_string(), "dsh".into());
        rec.insert("dir".to_string(), dir.into());
        rec.insert("kind".to_string(), kind.into());
        for (k, v) in extra {
            rec.insert(k.clone(), v.clone());
        }
        rec.insert("raw".to_string(), raw.clone());
        self.write_line(rec);
    }

    /// 记录一条发出去的请求。
    pub fn record_send(&self, id: u64, method: &str, raw: &serde_json::Value) {
        let mut extra = serde_json::Map::new();
        extra.insert("id".to_string(), serde_json::json!(id));
        extra.insert("method".to_string(), serde_json::json!(method));
        self.record_dsh("send", "request", &extra, raw);
    }

    /// 记录一条收到的行（response/notification/unparseable）。
    pub fn record_recv(
        &self,
        kind: &str,
        extra: &serde_json::Map<String, serde_json::Value>,
        raw: &serde_json::Value,
    ) {
        self.record_dsh("recv", kind, extra, raw);
    }

    /// 追加一条 app 记录（非 dsh 交互：配置/安装/spawn/initialize/run/shutdown 等）。
    /// state 层持同一个 WireLog 句柄调用。
    pub fn record_app(&self, kind: &str, data: &serde_json::Value) {
        let mut rec = serde_json::Map::new();
        rec.insert("cat".to_string(), "app".into());
        rec.insert("kind".to_string(), kind.into());
        rec.insert("data".to_string(), data.clone());
        self.write_line(rec);
    }
}

/// 管道对话：发送请求并配对响应，通知进事件广播。
#[derive(Debug)]
pub struct Transport {
    stdin: ChildStdin,
    // 统一读循环：响应 → 配对挂起请求；通知 → 事件广播。
    _read_task: JoinHandle<()>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<String, Error>>>>>,
    /// 事件流（广播，多订阅者）。消费方 subscribe/resubscribe 出多个接收端。
    events: broadcast::Receiver<Notification>,
    /// 线级日志（可选）：请求与收到的每一行都落盘。
    wire_log: Option<Arc<WireLog>>,
    next_id: u64,
}

impl Transport {
    /// 从管道启动对话（spawn 后台读循环）。
    /// 接收：stdin/stdout 管道（来自 process::spawn）+ 共享 runtime 状态（EOF 时取 exit code/stderr 尾部）
    ///      + 线级日志（可选）。
    /// 生成：Transport（持有 stdin、读循环句柄、事件接收端）。
    pub fn start(
        stdin: ChildStdin,
        stdout: ChildStdout,
        status: Arc<RuntimeStatus>,
        wire_log: Option<Arc<WireLog>>,
    ) -> Self {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<String, Error>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_loop = pending.clone();
        let (events_tx, events_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        // 读循环（后台常驻）：接收 stdout 管道，逐行交给 rpc::classify 帧判断——
        // 响应（有 id）→ 从 pending 取出对应的 oneshot 发送方，把响应行送回去（配对）；
        // 通知（无 id）→ 结构化（method + params）塞进 events 通道，供消费方解析；
        // 无法分类 → 打日志。
        // 循环结束（EOF = runtime 退出）→ 失败所有仍挂起的请求（带 exit code + stderr 尾部，
        // 对应官方 TransportClosedError 的语义，client.ts L39-46）。
        let task = {
            // 任务持有 Arc 克隆，Self 保留原值。
            let wire_log = wire_log.clone();
            tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // 原始行 → JSON（解析失败退化为字符串，仍进线级日志）。
                let parsed: serde_json::Value =
                    serde_json::from_str(&line).unwrap_or_else(|_| serde_json::Value::String(line.clone()));
                match rpc::classify(&line) {
                    Some(Frame::Response { id }) => {
                        if let Some(log) = &wire_log {
                            let mut extra = serde_json::Map::new();
                            extra.insert("id".to_string(), serde_json::json!(id));
                            log.record_recv("response", &extra, &parsed);
                        }
                        if let Some(tx) = pending_loop.lock().unwrap().remove(&id) {
                            let _ = tx.send(Ok(line));
                        }
                    }
                    Some(Frame::Notification(notification)) => {
                        if let Some(log) = &wire_log {
                            let mut extra = serde_json::Map::new();
                            extra.insert("method".to_string(), serde_json::json!(notification.method));
                            // session.event 细到事件类型（"具体到 event"）。
                            if notification.method == "session.event" {
                                if let Some(ty) = notification
                                    .params
                                    .pointer("/event/type")
                                    .and_then(serde_json::Value::as_str)
                                {
                                    extra.insert("eventType".to_string(), serde_json::json!(ty));
                                }
                            }
                            log.record_recv("notification", &extra, &parsed);
                        }
                        // 广播通知帧；消费方按 method 解析 params 成具体通知类型。
                        let _ = events_tx.send(notification);
                    }
                    None => {
                        if let Some(log) = &wire_log {
                            log.record_recv("unparseable", &serde_json::Map::new(), &parsed);
                        }
                        eprintln!("[unparseable] {line}");
                    }
                }
            }
            let exit_code = status.exit_code();
            let stderr_tail = status.stderr_tail();
            let leftover = std::mem::take(&mut *pending_loop.lock().unwrap());
            for (_, tx) in leftover {
                let _ = tx.send(Err(Error::TransportClosed {
                    exit_code,
                    stderr_tail: stderr_tail.clone(),
                }));
            }
            })
        };

        Self {
            stdin,
            _read_task: task,
            pending,
            events: events_rx,
            wire_log,
            next_id: 1,
        }
    }

    /// 发一个请求并等待配对响应；超时 → RequestTimeout（对应官方 RequestTimeoutError）。
    /// 接收：method（协议方法名）+ params（已序列化的 JSON，无 params 传 "{}"）+ 超时窗口。
    /// 处理：分配自增 id → 把 oneshot 发送方登记进 pending（先登记后写，防响应先到丢包）
    ///       → rpc::build_request 构造信封行 → 写入 stdin → 限时等待配对。
    /// 生成：读循环配对后返回对应的响应行（或错误）。
    pub async fn request(
        &mut self,
        method: &str,
        params: &str,
        timeout_ms: u64,
    ) -> Result<String, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = oneshot::channel();
        // 先登记再写：避免响应先到丢包。
        self.pending.lock().unwrap().insert(id, tx);

        let line = rpc::build_request(method, id, params);
        if let Some(log) = &self.wire_log {
            let parsed: serde_json::Value =
                serde_json::from_str(&line).unwrap_or_else(|_| serde_json::Value::String(line.clone()));
            log.record_send(id, method, &parsed);
        }
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;

        // timeout 的结果嵌套三层：timeout → oneshot → 值（Result<String, Error>）。
        // 内层/中层失败都意味着"没有配对响应"（读循环已关 / 读任务已死）→ TransportClosed。
        match timeout(Duration::from_millis(timeout_ms), rx).await {
            Ok(Ok(Ok(resp))) => Ok(resp),
            Ok(_) => Err(Error::TransportClosed {
                exit_code: None,
                stderr_tail: Vec::new(),
            }),
            // 超时：摘除挂起项（迟到的响应会被读循环忽略），报 RequestTimeout。
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(Error::RequestTimeout {
                    method: method.to_string(),
                    timeout_ms,
                })
            }
        }
    }

    /// 关闭 stdin（EOF）——dispose 阶梯第 1 级；sdk-app 把 stdin EOF 绑定到退出。
    pub async fn close_stdin(&mut self) -> Result<(), Error> {
        self.stdin.shutdown().await?;
        Ok(())
    }

    /// 事件流接收端（广播）。消费方从这里解析。
    pub fn events(&mut self) -> &mut broadcast::Receiver<Notification> {
        &mut self.events
    }

    /// 事件流接收端（move 出所有权，供后台任务常驻 select；当前广播游标起）。
    pub fn take_events(&mut self) -> broadcast::Receiver<Notification> {
        let fresh = self.events.resubscribe();
        std::mem::replace(&mut self.events, fresh)
    }

    /// 新建一个事件订阅（从当前广播游标开始；错过此前的缓冲事件）。
    /// 官方：client.ts 的 subscribe（一个连接多订阅者，客户端侧过滤）。
    pub fn subscribe(&mut self) -> broadcast::Receiver<Notification> {
        self.events.resubscribe()
    }
}
