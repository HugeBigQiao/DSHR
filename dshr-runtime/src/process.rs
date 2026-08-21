//! 进程生命周期：把 runtime 拉起来，管收尸。
//!
//! 只管"进程"本身（spawn / stderr 任务 / kill / wait），
//! 不管协议——管道交出去后由 transport 负责对话。
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::Error;

/// 一个已启动的 runtime 进程。
#[derive(Debug)]
pub struct RuntimeProcess {
    child: Child,
    // 后台任务持续读 stderr 并转发到通道（state 落库）。只 pipe 不读的话，缓冲区满了子进程会卡死。
    _stderr_task: JoinHandle<()>,
}

/// 启动 runtime 进程的配置。
#[derive(Debug)]
pub struct HarnessSpawnConfig {
    pub command: String,
    pub args: Vec<String>,
    pub current_dir: String,
    pub env: Vec<(String, String)>,
}

impl RuntimeProcess {
    /// spawn 进程 + 接管三根管道 + 起 stderr 后台任务。
    /// 接收：HarnessSpawnConfig（command/args/current_dir/env）。
    /// 处理：配置一个独立的 runtime 子进程，三根 stdio 全部 piped（不走终端走管道）；
    ///       stderr 每行转进 mpsc 通道（state 消费后落库，GUI 无终端时崩溃也可排查）。
    /// 生成：进程句柄 + stdin/stdout 管道 + stderr 行通道接收端。
    pub async fn spawn(
        config: HarnessSpawnConfig,
    ) -> Result<
        (
            Self,
            ChildStdin,
            ChildStdout,
            mpsc::UnboundedReceiver<String>,
        ),
        Error,
    > {
        // 用 config 逐项配置子进程：可执行文件 + 参数 + 工作目录 + 环境变量，
        // stdin/stdout/stderr 设为管道（而非继承终端），spawn() 真正拉起进程。
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(&config.current_dir)
            .envs(config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // 从 Child 里 take() 出管道所有权，各归其位：
        // stdin → transport 写请求；stdout → transport 读响应/通知；
        // stderr → 后台任务转发到通道；child 本身保留进程句柄供 kill/wait。
        // 已 piped 就必然是 Some，take 后 child 不再持有管道。
        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stderr = child.stderr.take().expect("stderr 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");

        // stderr 所有权移进后台任务：循环读到 EOF（子进程退出）才结束。
        // 持续读的意义：① 管道不读会缓冲区满、子进程卡死；② 每行转进通道（
        //   同时 eprintln 保留终端可见），state 消费后写 runtime_logs 落库。
        let (stderr_tx, stderr_rx) = mpsc::unbounded_channel();
        let _stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(err_line)) = err_lines.next_line().await {
                eprintln!("[runtime stderr] {err_line}");
                let _ = stderr_tx.send(err_line);
            }
        });

        Ok((
            Self {
                child,
                _stderr_task,
            },
            stdin,
            stdout,
            stderr_rx,
        ))
    }

    /// 收尸：kill + wait（dispose 阶梯的简化版，正式版见 DESIGN M1 收尾）。
    pub async fn kill_and_wait(mut self) -> Result<(), Error> {
        self.child.kill().await?;
        self.child.wait().await?;
        Ok(())
    }
}
