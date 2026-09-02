//! 进程生命周期：把 runtime 拉起来，管收尸（dispose 阶梯）。
//!
//! 只管"进程"本身（spawn / stderr 任务 / exit 监控 / dispose），
//! 不管协议——管道交出去后由 transport 负责对话。
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::error::Error;

/// stderr 尾部行数上限（bounded，防长时间运行撑爆内存）。
const STDERR_TAIL_MAX: usize = 40;

/// 共享运行状态：exit code + bounded stderr 尾部。
/// transport 的 EOF 用它构造 TransportClosedError（官方 client.ts L39-46 的语义）。
#[derive(Debug, Default)]
pub struct RuntimeStatus {
    exit_code: Mutex<Option<i32>>,
    stderr_tail: Mutex<VecDeque<String>>,
}

impl RuntimeStatus {
    /// 记录一行 stderr（尾部环形缓冲，超上限丢最旧）。
    fn record_stderr_line(&self, line: String) {
        let mut tail = self.stderr_tail.lock().unwrap();
        if tail.len() >= STDERR_TAIL_MAX {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    /// 当前 exit code（None = 尚未退出）。
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().unwrap()
    }

    /// 当前 stderr 尾部（bounded，崩溃排查用）。
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock().unwrap().iter().cloned().collect()
    }
}

/// 启动 runtime 进程的配置。
#[derive(Debug)]
pub struct HarnessSpawnConfig {
    pub command: String,
    pub args: Vec<String>,
    pub current_dir: String,
    pub env: Vec<(String, String)>,
    /// 单次请求超时（ms）——官方 `requestTimeoutMs`。
    pub request_timeout_ms: u64,
    /// stdin EOF 后等待协作退出的窗口（ms）——官方 `disposeEofGraceMs`。
    pub dispose_eof_grace_ms: u64,
    /// SIGTERM/强杀后的退出确认窗口（ms）——官方 `disposeGraceMs`。
    pub dispose_kill_grace_ms: u64,
    /// 线级日志落盘路径（JSONL，双向全量：请求/响应/通知/无法分类）。
    /// Some = 全程记录；None = 不记录。
    pub wire_log_path: Option<String>,
}

/// 一个已启动的 runtime 进程。
#[derive(Debug)]
pub struct RuntimeProcess {
    /// 共享句柄：dispose 与 exit 监控任务都要轮询/kill。
    child: Arc<Mutex<Child>>,
    // 后台任务持续读 stderr 并转发到通道（消费方落库）。只 pipe 不读的话，缓冲区满了子进程会卡死。
    _stderr_task: JoinHandle<()>,
    // exit 监控：轮询 try_wait（不消费 child），退出后记 exit code。
    _exit_task: JoinHandle<()>,
}

impl RuntimeProcess {
    /// spawn 进程 + 接管三根管道 + 起 stderr/exit 两个后台任务。
    /// 接收：HarnessSpawnConfig（command/args/current_dir/env + 超时/收尸窗口）。
    /// 处理：配置一个独立的 runtime 子进程，三根 stdio 全部 piped（不走终端走管道）；
    ///       stderr 每行 → 共享状态尾部 + mpsc 通道；exit 轮询 → 记录退出码。
    /// 生成：进程句柄 + stdin/stdout 管道 + stderr 行通道 + 共享状态。
    pub async fn spawn(
        config: HarnessSpawnConfig,
    ) -> Result<
        (
            Self,
            ChildStdin,
            ChildStdout,
            mpsc::UnboundedReceiver<String>,
            Arc<RuntimeStatus>,
        ),
        Error,
    > {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(&config.current_dir)
            .envs(config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stderr = child.stderr.take().expect("stderr 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");

        let status = Arc::new(RuntimeStatus::default());

        // stderr 所有权移进后台任务：循环读到 EOF（子进程退出）才结束。
        let (stderr_tx, stderr_rx) = mpsc::unbounded_channel();
        let status_err = status.clone();
        let _stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(err_line)) = err_lines.next_line().await {
                eprintln!("[runtime stderr] {err_line}");
                status_err.record_stderr_line(err_line.clone());
                let _ = stderr_tx.send(err_line);
            }
        });

        // exit 监控：轮询 try_wait（不消费 child），退出后记 exit code。
        // try_wait 与 dispose 的轮询并发调用是安全的（tokio 内部共享状态）。
        let child = Arc::new(Mutex::new(child));
        let exit_task_child = child.clone();
        let status_exit = status.clone();
        let _exit_task = tokio::spawn(async move {
            loop {
                let done = {
                    let mut guard = exit_task_child.lock().unwrap();
                    match guard.try_wait() {
                        Ok(Some(st)) => {
                            *status_exit.exit_code.lock().unwrap() = st.code();
                            true
                        }
                        Ok(None) => false,
                        Err(_) => true,
                    }
                };
                if done {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        Ok((
            Self {
                child,
                _stderr_task,
                _exit_task,
            },
            stdin,
            stdout,
            stderr_rx,
            status,
        ))
    }

    /// 收尸阶梯（官方 dispose.ts 的 `disposeRuntimeProcess` 的 Rust 版）：
    /// 1. stdin 已由 transport 关闭（EOF）→ 等进程协作退出（sdk-app 绑定 EOF 到 shutdown）；
    /// 2. POSIX 发 SIGTERM，Windows 跳过（官方同款：Node 两信号都映射 TerminateProcess）；
    /// 3. 强杀（Windows TerminateProcess / POSIX SIGKILL），等到真实退出。
    /// 任一级在窗口内退出即成功返回。
    pub async fn dispose(self, eof_grace_ms: u64, kill_grace_ms: u64) -> Result<(), Error> {
        let Self {
            child,
            _stderr_task: _,
            _exit_task: _,
        } = self;

        // 1. 协作退出窗口（stdin EOF 已由 transport 关闭）。
        if exits_within(&child, eof_grace_ms).await {
            return Ok(());
        }
        // 2. POSIX：可捕获的 SIGTERM。id() 为 None = 进程已在窗口竞争间退出，
        // 跳过（若把 None 当 0 发信号会打到整个进程组）。
        #[cfg(unix)]
        {
            if let Some(pid) = child.lock().unwrap().id() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
                if exits_within(&child, kill_grace_ms).await {
                    return Ok(());
                }
            }
        }
        // 3. 强杀 + 有界退出确认。
        child.lock().unwrap().start_kill()?;
        if !exits_within(&child, kill_grace_ms).await {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("runtime 在强杀后 {kill_grace_ms}ms 内未退出"),
            )));
        }
        Ok(())
    }
}

/// 在窗口内轮询进程退出；true = 已退出。窗口到点未退出返回 false（不动进程）。
async fn exits_within(child: &Arc<Mutex<Child>>, ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        if let Ok(Some(_)) = child.lock().unwrap().try_wait() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
