//! 总装师：组装 process（进程）+ transport（管道对话），暴露类型化方法 API。
//!
//! 本文件保持薄：方法体是"序列化 → transport.request → rpc.parse"三行委托。
//! 协议形状在 `dsh-sdk-protocol`，I/O 与配对在 `transport`，进程生死在 `process`。
//! 对应官方：packages/sdk/client/src/client.ts 的 HarnessClient。
use std::sync::Arc;

use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SessionPromptParams, SessionPromptResult, ShutdownResult,
};
use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::mpsc;

use crate::process::RuntimeProcess;
use crate::transport::{Transport, WireLog};

pub use crate::error::Error;
pub use crate::process::HarnessSpawnConfig;

#[derive(Debug)]
pub struct HarnessClient {
    process: RuntimeProcess,
    transport: Transport,
    /// stderr 行通道接收端（消费方落库）。
    stderr: mpsc::UnboundedReceiver<String>,
    request_timeout_ms: u64,
    dispose_eof_grace_ms: u64,
    dispose_kill_grace_ms: u64,
}

impl HarnessClient {
    /// 组装：拉起进程 → 启动管道对话。
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
        let transport = Transport::start(stdin, stdout, status, wire_log);
        Ok(Self {
            process,
            transport,
            stderr,
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
            request_timeout_ms,
            dispose_eof_grace_ms,
            dispose_kill_grace_ms,
        } = self;
        let resp = transport.request("shutdown", "{}", request_timeout_ms).await;
        if let Ok(resp) = resp {
            let _ = dsh_sdk_protocol::rpc::parse::<ShutdownResult>(&resp);
        }
        let _ = transport.close_stdin().await;
        process
            .dispose(dispose_eof_grace_ms, dispose_kill_grace_ms)
            .await?;
        Ok(())
    }

    /// 事件流接收端（广播）。消费方从这里解析。
    pub fn events(&mut self) -> &mut tokio::sync::broadcast::Receiver<Notification> {
        self.transport.events()
    }

    /// 事件流接收端（move 出所有权，供后台任务常驻 select）。
    pub fn take_events(&mut self) -> tokio::sync::broadcast::Receiver<Notification> {
        self.transport.take_events()
    }

    /// 新建一个事件订阅（从当前广播游标开始）。
    /// 官方：client.ts 的 HarnessClient.subscribe（一个连接多订阅者）。
    pub fn subscribe(&mut self) -> tokio::sync::broadcast::Receiver<Notification> {
        self.transport.subscribe()
    }

    /// stderr 行接收端（进程日志）。消费方消费后落库。
    pub fn stderr(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        &mut self.stderr
    }

    /// stderr 行接收端（move 出所有权，供后台任务常驻 select）。
    pub fn take_stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        std::mem::replace(&mut self.stderr, mpsc::unbounded_channel().1)
    }
}
