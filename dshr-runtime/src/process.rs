//! 进程生命周期：把 runtime 拉起来，管收尸。
//!
//! 只管"进程"本身（spawn / stderr 任务 / kill / wait），
//! 不管协议——管道交出去后由 transport 负责对话。
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::error::Error;

/// 一个已启动的 runtime 进程。
#[derive(Debug)]
pub struct RuntimeProcess {
    child: Child,
    // 后台任务持续读 stderr 并打日志。只 pipe 不读的话，缓冲区满了子进程会卡死。
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
    /// 返回进程句柄 + stdin/stdout 管道（stdout 交给 transport 的读循环）。
    pub async fn spawn(
        config: HarnessSpawnConfig,
    ) -> Result<(Self, ChildStdin, ChildStdout), Error> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(&config.current_dir)
            .envs(config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // take() 拿走管道所有权，child 留着做 kill/wait；已 piped 就必然是 Some
        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stderr = child.stderr.take().expect("stderr 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");

        // stderr 的所有权移进 task，读到 EOF（子进程退出）才结束。
        let _stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(err_line)) = err_lines.next_line().await {
                eprintln!("[runtime stderr] {err_line}");
            }
        });

        Ok((
            Self {
                child,
                _stderr_task,
            },
            stdin,
            stdout,
        ))
    }

    /// 收尸：kill + wait（dispose 阶梯的简化版，正式版见 DESIGN M1 收尾）。
    pub async fn kill_and_wait(mut self) -> Result<(), Error> {
        self.child.kill().await?;
        self.child.wait().await?;
        Ok(())
    }
}
