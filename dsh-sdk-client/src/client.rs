//! 总装师：组装 process（进程）+ transport（管道对话），暴露类型化方法 API。
//!
//! 主要用途：`HarnessClient::spawn` 一次拉起进程与读循环，之后提供 `initialize` / `prompt` /
//! `shutdown` 三个类型化请求，以及 `events` / `stderr` 两条消费通道，供 api.rs 的 run 与
//! 上层状态层使用。
//! 为什么需要：把「进程 + 管道 + 超时/收尸配置」合成一个对象，调用方才不必自己维护
//! 生命周期顺序（必须先 spawn 才能读、shutdown 必须消费 self）；本文件刻意保持薄——
//! 每个方法只是「序列化 → transport.request → rpc.parse」三行委托，
//! 一旦这里长出业务逻辑，就说明该逻辑放错了层（协议归 protocol、状态归 dshr-state）。
//! 上接：`dshr-state` 的 raw 层直接持有它；`dsh-sdk-client` 的 api.rs / subscription.rs
//!       在本类型上扩展（`impl HarnessClient`）；集成测试（重建后放 `tests/`）。
//! 下接：`crate::process::{RuntimeProcess, HarnessSpawnConfig}`、
//!       `crate::transport::{Transport, WireLog}`、`dsh_sdk_protocol::{requests, rpc}`。
//!
//! 本文件保持薄：方法体是"序列化 → transport.request → rpc.parse"三行委托。
//! 协议形状在 `dsh-sdk-protocol`，I/O 与配对在 `transport`，进程生死在 `process`。
//!
//! 官方对应：packages/sdk/client/src/client.ts 的 HarnessClient。
use std::sync::Arc;

use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SessionPromptParams, SessionPromptResult, ShutdownResult,
};
use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::mpsc;

use crate::process::{RuntimeProcess, RuntimeStatus};
use crate::transport::{Transport, WireLog};

pub use crate::error::Error;
pub use crate::process::HarnessSpawnConfig;

/// 一个 runtime 的客户端总装：进程 + 管道 + 两条消费通道 + 三个超时窗口。
///
/// 为什么需要：调用方只应看到「一个可用的 runtime」，而不是四个需要按顺序组装的对象
///（先 spawn、再起读循环、再拿 stderr）；把这些字段私有化后，生命周期顺序由构造器保证。
/// 约束：所有请求方法都 `&mut self`（要写 stdin 并改 id/挂起表），
/// `shutdown` 消费 `self`（收尸后不可再用）。
#[derive(Debug)]
pub struct HarnessClient {
    process: RuntimeProcess,
    transport: Transport,
    /// stderr 行通道接收端（消费方落库）。
    stderr: mpsc::UnboundedReceiver<String>,
    /// 进程退出状态的共享句柄（`Arc`，由 `process` 的 exit 监控任务写）。
    ///
    /// 为什么留在客户端：这是「进程是否还活着、退出码是多少」的**唯一可查询来源**。
    /// 协议通知面（4 种）不含退出信息，只在请求失败时经 `Error::TransportClosed` 间接暴露；
    /// 上层（engine）需要主动轮询来判断「runtime 是不是悄悄死了」。
    status: Arc<RuntimeStatus>,
    request_timeout_ms: u64,
    dispose_eof_grace_ms: u64,
    dispose_kill_grace_ms: u64,
}

impl HarnessClient {
    /// 组装：拉起进程 → 启动管道对话。
    ///
    /// # 参数
    /// - `config`：见 `HarnessSpawnConfig`（命令/环境/三档超时/wire log 路径）。
    ///
    /// # 返回 / 错误
    /// 成功返回可用客户端；`Error::Io` = 进程起不来或 wire log 打不开，
    ///（此阶段还没有协议交互，故不会有 `RpcError`/`SdkProtocol`）。
    ///
    /// 为什么需要：把「打开 wire log → spawn 进程 → 起读循环」三步固定成一个构造器，
    /// 保证读循环一定在第一个请求之前就绪、且三个超时窗口与进程同源。
    pub async fn spawn(config: HarnessSpawnConfig) -> Result<Self, Error> {
        let request_timeout_ms = config.request_timeout_ms;
        let dispose_eof_grace_ms = config.dispose_eof_grace_ms;
        let dispose_kill_grace_ms = config.dispose_kill_grace_ms;
        let wire_log = config
            .wire_log_path
            .as_deref()
            .map(WireLog::open)
            .transpose()?
            .map(Arc::new);
        let (process, stdin, stdout, stderr, status) = RuntimeProcess::spawn(config).await?;
        // transport 与 client 共享同一个 status（Arc 克隆，零拷贝）：transport 在 EOF 时读它
        // 构造 `TransportClosed`，client 的 `runtime_status()` 让上层随时查询退出码。
        let transport = Transport::start(stdin, stdout, status.clone(), wire_log);
        Ok(Self {
            process,
            transport,
            stderr,
            status,
            request_timeout_ms,
            dispose_eof_grace_ms,
            dispose_kill_grace_ms,
        })
    }

    /// initialize：进程级握手。
    /// 官方：client.ts 的 HarnessClient.initialize（provider/model/reasoningEffort/maxTokens 路由校验）。
    pub async fn initialize(
        &mut self,
        params: &InitializeParams,
    ) -> Result<InitializeResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self
            .transport
            .request("initialize", &body, self.request_timeout_ms)
            .await?;
        Ok(dsh_sdk_protocol::rpc::parse(&resp)?)
    }

    /// session/prompt：发一条消息。
    /// 官方：client.ts 的 HarnessClient.prompt（返回 messageId 入队回执，不等 agent 活动）。
    pub async fn prompt(
        &mut self,
        params: &SessionPromptParams,
    ) -> Result<SessionPromptResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self
            .transport
            .request("session/prompt", &body, self.request_timeout_ms)
            .await?;
        Ok(dsh_sdk_protocol::rpc::parse(&resp)?)
    }

    /// shutdown：协议 shutdown → dispose 阶梯（EOF → [SIGTERM] → SIGKILL）。
    /// 官方：client.ts 的 HarnessClient.close + dispose.ts 的 disposeRuntimeProcess。
    /// runtime 已死时协议请求会失败——忽略，继续收尸即可。
    pub async fn shutdown(self) -> Result<(), Error> {
        let Self {
            process,
            mut transport,
            stderr: _,
            status: _,
            request_timeout_ms,
            dispose_eof_grace_ms,
            dispose_kill_grace_ms,
        } = self;
        let resp = transport
            .request("shutdown", "{}", request_timeout_ms)
            .await;
        if let Ok(resp) = resp {
            let _ = dsh_sdk_protocol::rpc::parse::<ShutdownResult>(&resp);
        }
        let _ = transport.close_stdin().await;
        process
            .dispose(dispose_eof_grace_ms, dispose_kill_grace_ms)
            .await?;
        Ok(())
    }

    /// 进程退出状态的共享句柄（非阻塞查询）。
    ///
    /// # 返回
    /// `Arc<RuntimeStatus>` 的克隆——`exit_code()`（`None` = 尚未退出）与 `stderr_tail()`
    ///（有界尾部，最多 40 行）随时可读。
    ///
    /// 为什么需要：协议通知面**不含**进程退出信息；上层要判断「runtime 是不是已经死了」
    /// 只能靠这个句柄或等某个请求失败。设计上刻意做成**查询**而非事件流——调用方的常驻
    /// `select` 循环本来就会周期性醒来，轮询一个 `Arc` 比多养一个任务/通道简单得多。
    pub fn runtime_status(&self) -> Arc<RuntimeStatus> {
        self.status.clone()
    }

    /// 事件流接收端（广播）。消费方从这里解析。
    ///
    /// # 返回
    /// 借用内部接收端（游标在 client 内，可反复借用）。
    /// 为什么需要：raw 层的常驻消费循环要持续借用同一个接收端而不交出所有权。
    pub fn events(&mut self) -> &mut tokio::sync::broadcast::Receiver<Notification> {
        self.transport.events()
    }

    /// 事件流接收端（move 出所有权，供后台任务常驻 select）。
    ///
    /// # 返回
    /// 从当前广播游标开始的新接收端（错过此前缓冲的事件）——必须在发 prompt **之前**调用。
    pub fn take_events(&mut self) -> tokio::sync::broadcast::Receiver<Notification> {
        self.transport.take_events()
    }

    /// 新建一个事件订阅（从当前广播游标开始）。
    ///
    /// # 返回
    /// 独立订阅端；多个订阅者互不影响（慢订阅者超限时丢最旧事件）。
    /// 官方：client.ts 的 HarnessClient.subscribe（一个连接多订阅者）。
    pub fn subscribe(&mut self) -> tokio::sync::broadcast::Receiver<Notification> {
        self.transport.subscribe()
    }

    /// stderr 行接收端（进程日志）。消费方消费后落库。
    ///
    /// # 返回
    /// 借用内部的无界通道接收端（runtime 的 stderr 原文，逐行）。
    /// 为什么需要：`runtime_logs` 表（审计）等这条流；借用版供原地 `try_recv` 轮询。
    pub fn stderr(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        &mut self.stderr
    }

    /// stderr 行接收端（move 出所有权，供后台任务常驻 select）。
    ///
    /// # 返回
    /// 原接收端；内部换上一个新的空通道（原通道的后续行会被丢弃，
    /// 故调用方取走后必须自己常驻消费，否则 stderr 只留在 `RuntimeStatus` 的有界尾部里）。
    pub fn take_stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        std::mem::replace(&mut self.stderr, mpsc::unbounded_channel().1)
    }
}
