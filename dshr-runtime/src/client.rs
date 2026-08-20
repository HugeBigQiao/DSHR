//! 总装师：组装 process（进程）+ transport（管道对话），暴露类型化方法 API。
//!
//! 本文件保持薄：方法体是"序列化 → transport.request → rpc.parse"三行委托。
//! 协议形状在 `dshr-protocol`，I/O 与配对在 `transport`，进程生死在 `process`。
use dshr_protocol::requests::{
    InitializeParams, InitializeResult, SessionPromptParams, SessionPromptResult, ShutdownResult,
};
use dshr_protocol::rpc::Notification;
use tokio::sync::mpsc;

use crate::process::RuntimeProcess;
use crate::transport::Transport;

pub use crate::error::Error;
pub use crate::process::HarnessSpawnConfig;

#[derive(Debug)]
pub struct HarnessClient {
    process: RuntimeProcess,
    transport: Transport,
}

impl HarnessClient {
    /// 组装：拉起进程 → 启动管道对话。
    pub async fn spawn(config: HarnessSpawnConfig) -> Result<Self, Error> {
        let (process, stdin, stdout) = RuntimeProcess::spawn(config).await?;
        let transport = Transport::start(stdin, stdout);
        Ok(Self { process, transport })
    }

    /// initialize：进程级握手，返回 serverInfo。
    pub async fn initialize(
        &mut self,
        params: &InitializeParams,
    ) -> Result<InitializeResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self.transport.request("initialize", &body).await?;
        Ok(dshr_protocol::rpc::parse(&resp)?)
    }

    /// session/prompt：发一条消息，返回 messageId 入队回执。
    pub async fn prompt(
        &mut self,
        params: &SessionPromptParams,
    ) -> Result<SessionPromptResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self.transport.request("session/prompt", &body).await?;
        Ok(dshr_protocol::rpc::parse(&resp)?)
    }

    /// shutdown：发 shutdown → 等响应 → 杀进程 → 收尸。
    pub async fn shutdown(self) -> Result<(), Error> {
        let Self {
            process,
            mut transport,
        } = self;
        let resp = transport.request("shutdown", "{}").await?;
        dshr_protocol::rpc::parse::<ShutdownResult>(&resp)?;
        process.kill_and_wait().await?;
        Ok(())
    }

    /// 事件流接收端（结构化通知帧）。state 从这里消费。
    pub fn events(&mut self) -> &mut mpsc::UnboundedReceiver<Notification> {
        self.transport.events()
    }
}
