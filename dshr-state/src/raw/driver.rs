//! raw 层的驱动接口：`SessionDriver` 与退出信号快照。
//!
//! 主要用途：定义「一个 runtime 的完整可观测面」，让 `raw::Runtime` 依赖**接口**而不是
//! 具体的 `HarnessClient`。M3 的 engine 因此能注入假 driver 做单测（不起进程、不烧 token）。
//! 为什么需要：`dsh-sdk-client` 的 `HarnessClient` 是唯一能碰进程与管道的类型，
//! 若 `Runtime` 直接持有它，任何针对会话路由/落库的测试都必须真的拉起一个 node 子进程。
//!
//! 暴露面的构成（**三进四出 + 两类进程信号**）：
//! - 请求侧 3：`initialize` / `prompt` / `shutdown`；
//! - 通知侧 4：由 [`SessionDriver::events`] 返回的原始帧流承载（`session.event` /
//!   `session.status` / `subagent.started` / `subagent.finished`，解析在
//!   `dsh_sdk_protocol::notifications::parse`）；
//! - **进程信号 2**：stderr 行流（[`SessionDriver::stderr`]）与退出状态
//!   （[`SessionDriver::runtime_status`]）。这两类**不走协议通知**，但少了它们
//!   engine 就无法做「runtime 死了要报错 + 落库审计」——这正是把它们放进 trait 的理由。
//!
//! 粒度 = **runtime 级**：一个实现 = 一个 runtime 子进程（`Transport` 本就是每 runtime 一条：
//! 一个 stdin、一个广播发送端、一个 `WireLog`）。所以 `session_id` 是**方法参数**而
//! 不是 trait 的身份——同一 driver 可服务同一 runtime 内的多个会话。
//!
//! 上接：`raw.rs`（`Runtime` 经 `Box<dyn SessionDriver>` 调用）、`raw/client_driver.rs`（实现）。
//! 下接：`dsh_sdk_client::client` / `process`（退出状态类型）、
//!       `dsh_sdk_protocol::{requests, rpc}`（请求与通知类型）、`crate::error`（错误来源）。
//! 官方对应：无（官方 TS client 的 `HarnessClient` 直接暴露方法，没有抽接口层；
//! dshr 抽这层是为了让宿主侧可测——见 `DESIGN.md` §3.5）。

use dsh_sdk_client::client::Error;
use dsh_sdk_client::process::RuntimeStatus;
use dsh_sdk_protocol::requests::{
    InitializeParams, InitializeResult, SessionPromptParams, SessionPromptResult,
};
use dsh_sdk_protocol::rpc::Notification;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// trait 里 async 方法的统一返回类型。
///
/// 为什么需要（而不是 `async fn`）：`async fn` 在 trait 中生成的 future 不满足 dyn 兼容性，
/// 无法放进 `Box<dyn SessionDriver>`——而「可替换驱动」正是本 trait 存在的理由。
/// 手写 `Pin<Box<dyn Future>>` 换来 dyn 兼容，代价是每个实现要 `Box::pin`;
/// 相比引入 `async-trait` 依赖，这里只有 3 个 async 方法，手写更划算且零依赖。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 一个 runtime 的驱动接口。
///
/// 为什么需要：见模块头——把「进程 + 管道 + 两条通道」收敛成可替换接口。
/// 约束：实现必须是 `Send`（调用方在 tokio 多线程运行时里持有它并跨 `await`）。
/// 生命周期：`shutdown` 是收尾动作；调用后不应再使用本实例（实现方可自行置为不可用）。
pub trait SessionDriver: Send {
    /// 进程级握手：校验 provider/model 路由并建立会话运行环境。
    ///
    /// # 参数
    /// - `params`：`cwd` / `provider` / `model` / `reasoningEffort?` / `maxTokens?`。
    ///
    /// # 返回 / 错误
    /// 成功给出 `serverInfo`（注意 `version` 官方硬编码为 `'0.0.1'`，不可用于版本校验）；
    /// 错误可能是 `RpcError`（握手被拒）或 `TransportClosed`（进程已死）。
    fn initialize<'a>(
        &'a mut self,
        params: &'a InitializeParams,
    ) -> BoxFuture<'a, Result<InitializeResult, Error>>;

    /// 发一条用户消息（**session 级**：`params.session_id` 指定目标会话）。
    ///
    /// # 参数
    /// - `params`：目标 `session_id` + 内容块（文本/图片）。官方语义：未知 id 会惰性
    ///   创建 agent+session 对，所以同一 runtime 内可并存多个会话。
    ///
    /// # 返回 / 错误
    /// 成功返回 `messageId`（**入队回执**，不代表模型已答完；产出走 `events` 流）。
    /// 错误同 `initialize`。
    fn prompt<'a>(
        &'a mut self,
        params: &'a SessionPromptParams,
    ) -> BoxFuture<'a, Result<SessionPromptResult, Error>>;

    /// 取通知流（4 种通知的**原始帧**；解析交给 `notifications::parse`）。
    ///
    /// # 返回
    /// 广播接收端，从当前游标开始。游标之前的缓冲事件不可见——所以要在发 `prompt`
    /// **之前**取流，否则可能漏掉入队回执。
    ///
    /// 为什么需要：trait 不在这里解析成 `Kind`，是为了让「谁负责解析」保持在消费方
    ///（engine 按会话路由时要先看 `sessionId` 原始字段），也避免 trait 依赖解析策略。
    fn events(&mut self) -> broadcast::Receiver<Notification>;

    /// 取进程 stderr 行流（**进程信号 1**）。
    ///
    /// # 返回
    /// 未绑定通道的行接收端；**只能取到一次**（实现方交出所有权，之后返回空通道）。
    ///
    /// 为什么需要：`store` 有 `runtime_logs` 表等着写、崩溃排查也需要尾部之外的全量日志。
    /// 目前上层未消费它——一个建了表却没有写入方的字段，正是「接口没暴露导致信息丢失」的例子。
    fn stderr(&mut self) -> mpsc::UnboundedReceiver<String>;

    /// 进程退出状态的共享句柄（**进程信号 2**，非阻塞查询）。
    ///
    /// # 返回
    /// `Arc<RuntimeStatus>`：`exit_code()`（`None` = 还没退出）与 `stderr_tail()`
    ///（有界尾部，最多 40 行，用于把「为什么死的」和「死了」一起报出来）。
    ///
    /// 为什么是查询而不是事件流：调用方的常驻 `select` 循环本来就会周期性醒来，
    /// 轮询一个 `Arc` 比多养一个任务 + 通道简单；且退出信息与 stderr 尾部同源，
    /// 分成事件流反而要在两处对齐。
    fn runtime_status(&mut self) -> Arc<RuntimeStatus>;

    /// 收尾：协议 `shutdown` → dispose 阶梯（EOF → [SIGTERM] → SIGKILL）。
    ///
    /// # 返回 / 错误
    /// 正常收尸返回 `Ok(())`；只有在**强杀后**仍未退出才 `Err(Io(TimedOut))`。
    /// 进程已死时协议请求必然失败——实现方应忽略它继续收尸。
    ///
    /// 为什么用 `&mut self` 而不是消费 `self`：trait 要能放进 `Box<dyn SessionDriver>` 并被
    /// `Runtime` 持有；消费型方法在 trait 对象上难以调用（`Box<Self>` 的 sized 约束）。
    /// 实现方内部用 `Option` 记录「已收尾」，重复调用是安全的空操作。
    fn shutdown(&mut self) -> BoxFuture<'_, Result<(), Error>>;
}
