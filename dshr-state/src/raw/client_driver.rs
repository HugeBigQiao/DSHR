//! `SessionDriver` 的真实实现：把 `HarnessClient` 适配成 raw 层的接口。
//!
//! 主要用途：`raw::Runtime` 启动 runtime 时（生产路径）创建本类型，并以
//! `Box<dyn SessionDriver>` 持有——从而与具体客户端解耦，M3 的 engine 可换成假 driver 测试。
//! 为什么需要（而不是直接 `impl SessionDriver for HarnessClient`）：`HarnessClient::shutdown`
//! **消费 `self`**（收尸后类型上不可再用），而 trait 的 `shutdown(&mut self)` 要能在
//! `Box<dyn SessionDriver>` 上重复调用。本类型用 `Option` 容纳「已被收尾」这一状态，
//! 把这个所有权差异挡在适配层内，不污染协议层也不泄漏给上层。
//!
//! 上接：`raw.rs`（`Runtime` 持有 `Box<dyn SessionDriver>`）、`raw/driver.rs`（被实现的接口）。
//! 下接：`dsh_sdk_client::client::HarnessClient`（真正干活的那个）。
//! 官方对应：无（官方没有这一层适配；它源自 Rust 的所有权约束，见模块头说明）。

use std::sync::Arc;

use dsh_sdk_client::client::{Error, HarnessClient};
use dsh_sdk_client::process::{HarnessSpawnConfig, RuntimeStatus};
use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SessionPromptParams, SessionPromptResult,
};
use dsh_sdk_protocol::rpc::Notification;
use tokio::sync::{broadcast, mpsc};

use super::driver::{BoxFuture, SessionDriver};

/// 把 [`HarnessClient`] 适配为 [`SessionDriver`]。
///
/// 为什么需要：见模块头——消化 `HarnessClient::shutdown(self)` 与
/// `SessionDriver::shutdown(&mut self)` 之间的所有权差异。
pub struct ClientDriver {
    /// `None` = 已收尾（`shutdown` 已消费掉客户端）。
    client: Option<HarnessClient>,
}

impl ClientDriver {
    /// 拉起 runtime 子进程并适配成 driver。
    ///
    /// # 参数
    /// - `config`：见 [`HarnessSpawnConfig`]（命令/env/三档超时/wire log 路径）。
    ///
    /// # 返回 / 错误
    /// 成功返回可用的 driver；`Error::Io` = 进程起不来或 wire log 打不开
    ///（此阶段还没有协议交互，故不会有 `RpcError`/`SdkProtocol`）。
    ///
    /// 为什么需要：上层（`raw::Runtime`）不该知道 `HarnessClient` 的存在，
    /// 所以「怎么造出它」也留在这里。
    pub async fn spawn(config: HarnessSpawnConfig) -> Result<Self, Error> {
        Ok(Self {
            client: Some(HarnessClient::spawn(config).await?),
        })
    }
}

impl SessionDriver for ClientDriver {
    /// 委托 `HarnessClient::initialize`；已收尾时报 `TransportClosed`。
    ///
    /// 为什么把「已收尾」映射为 `TransportClosed`：语义一致——调用的那个通道已经不在了，
    /// 上层按「runtime 没了」处理即可，不需要为「已主动收尾」单独造一个错误变体。
    /// 返回 `Box::pin(...)` 而非 `async fn`：见 `driver::BoxFuture` 的说明（dyn 兼容）。
    fn initialize<'a>(
        &'a mut self,
        params: &'a InitializeParams,
    ) -> BoxFuture<'a, Result<InitializeResult, Error>> {
        Box::pin(async move {
            match self.client.as_mut() {
                Some(c) => c.initialize(params).await,
                None => Err(closed()),
            }
        })
    }

    /// 委托 `HarnessClient::prompt`；已收尾时报 `TransportClosed`。
    fn prompt<'a>(
        &'a mut self,
        params: &'a SessionPromptParams,
    ) -> BoxFuture<'a, Result<SessionPromptResult, Error>> {
        Box::pin(async move {
            match self.client.as_mut() {
                Some(c) => c.prompt(params).await,
                None => Err(closed()),
            }
        })
    }

    /// 委托 `HarnessClient::take_events`（**交出**当前游标之后的接收端）。
    ///
    /// 注意与 `HarnessClient::events()`（借用）的区别：trait 用的是 `take` 语义，
    /// 因为 `raw::Runtime` 要把接收端移进自己的 `select` 循环里常驻持有。
    /// 已收尾时返回一个立即结束的通道（接收端下次 `recv` 即得 `Closed`）。
    fn events(&mut self) -> broadcast::Receiver<Notification> {
        match self.client.as_mut() {
            Some(c) => c.take_events(),
            None => {
                let (tx, rx) = broadcast::channel(1);
                drop(tx);
                rx
            }
        }
    }

    /// 委托 `HarnessClient::take_stderr`。
    ///
    /// 只能取到一次：`HarnessClient` 内部用「空通道替换」实现交出所有权，
    /// 第二次调用返回的是永不产出行的通道。这是刻意的——stderr 是**单消费者流**。
    fn stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        match self.client.as_mut() {
            Some(c) => c.take_stderr(),
            None => mpsc::unbounded_channel().1,
        }
    }

    /// 委托 `HarnessClient::runtime_status`（`Arc` 克隆，随时可查）。
    ///
    /// 为什么已收尾时也能返回：退出状态是**历史事实**（exit code 是收尾后才知道的），
    /// 收尾后查询反而最需要它——所以这里保留句柄而不报错。
    fn runtime_status(&mut self) -> Arc<RuntimeStatus> {
        match self.client.as_ref() {
            Some(c) => c.runtime_status(),
            // 已收尾：造一个空的（exit_code = None）而不是 panic——调用方拿到的信息量
            // 少了，但不会因为「收尾后还被问一次」而崩溃。
            None => Arc::new(RuntimeStatus::default()),
        }
    }

    /// 收尾：取出客户端并委托 `HarnessClient::shutdown`（协议 shutdown + dispose 阶梯）。
    ///
    /// 幂等：第二次调用时 `client` 已是 `None`，直接返回 `Ok(())`。
    /// 为什么需要幂等：`Runtime::stop` 与异常拆除（`teardown`）都可能触发收尾，
    /// 而「已收尾」不是错误状态。
    fn shutdown(&mut self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            match self.client.take() {
                Some(c) => c.shutdown().await,
                None => Ok(()),
            }
        })
    }
}

/// 构造「通道已不在」的错误（与 transport EOF 的语义一致）。
///
/// 为什么抽成函数：`TransportClosed` 有两个字段，三处调用重复构造会掩盖「这里是同一个原因」。
/// 返回：`exit_code = None`、`stderr_tail` 为空——因为此时不是从进程读到的退出，
/// 而是「driver 自己已经收尾了」，没有真实退出码可报。
fn closed() -> Error {
    Error::TransportClosed {
        exit_code: None,
        stderr_tail: Vec::new(),
    }
}
