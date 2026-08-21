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
    /// stderr 行通道接收端（state 消费后落库 runtime_logs）。
    stderr: mpsc::UnboundedReceiver<String>,
}

impl HarnessClient {
    /// 组装：拉起进程 → 启动管道对话。
    pub async fn spawn(config: HarnessSpawnConfig) -> Result<Self, Error> {
        let (process, stdin, stdout, stderr) = RuntimeProcess::spawn(config).await?;
        let transport = Transport::start(stdin, stdout);
        Ok(Self {
            process,
            transport,
            stderr,
        })
    }

    /// initialize：进程级握手。
    /// 接收：InitializeParams（cwd/provider/model/maxTokens）。
    /// 处理：序列化 → transport.request 发送并等待配对 → rpc::parse 剥 result。
    /// 生成：InitializeResult（serverInfo），可校验 runtime 身份与版本。
    pub async fn initialize(
        &mut self,
        params: &InitializeParams,
    ) -> Result<InitializeResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self.transport.request("initialize", &body).await?;
        Ok(dshr_protocol::rpc::parse(&resp)?)
    }

    /// session/prompt：发一条消息。
    /// 接收：SessionPromptParams（sessionId + contentBlocks）。
    /// 处理：序列化 → transport.request → rpc::parse（未知 sessionId 由 runtime 懒创建）。
    /// 生成：SessionPromptResult（messageId 入队回执）；事件随后经 events() 流出。
    pub async fn prompt(
        &mut self,
        params: &SessionPromptParams,
    ) -> Result<SessionPromptResult, Error> {
        let body = serde_json::to_string(params)?;
        let resp = self.transport.request("session/prompt", &body).await?;
        Ok(dshr_protocol::rpc::parse(&resp)?)
    }

    /// shutdown：优雅关闭。
    /// 接收：无（self 消费）。
    /// 处理：发 shutdown（params 空对象 {}）→ 等响应 → 杀进程 → 收尸。
    /// 生成：进程已退出（dispose 阶梯的简化版，正式版 EOF→SIGTERM→SIGKILL）。
    pub async fn shutdown(self) -> Result<(), Error> {
        let Self {
            process,
            mut transport,
            stderr: _,
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

    /// 事件流接收端（move 出所有权，供后台任务常驻 select）。
    pub fn take_events(&mut self) -> mpsc::UnboundedReceiver<Notification> {
        self.transport.take_events()
    }

    /// stderr 行接收端（进程日志）。state 消费后写 runtime_logs。
    pub fn stderr(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        &mut self.stderr
    }

    /// stderr 行接收端（move 出所有权，供后台任务常驻 select）。
    pub fn take_stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        std::mem::replace(&mut self.stderr, mpsc::unbounded_channel().1)
    }
}
