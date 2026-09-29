//! 进程生命周期：把 runtime 拉起来，管收尸（dispose 阶梯）。
//!
//! 主要用途：`RuntimeProcess::spawn` 起 node 子进程并接管三根管道（stdin/stdout 交出去、
//! stderr 转成通道 + 有界尾部）、`dispose` 按 EOF → [SIGTERM] → SIGKILL 阶梯收尸；
//! `RuntimeStatus` 提供 exit code 与 stderr 尾部给上层构造 `TransportClosed`。
//! 为什么需要：进程生死与协议对话是两种完全不同的失败模式（OOM/挂死/不退 vs 帧畸形），
//! 混在一个文件后「读循环」会同时承担 I/O 与 wait/kill；分开才能让 transport 只管管道，
//! 也才能让 dispose 阶梯（Windows 跳过 SIGTERM 这类平台差异）有唯一落点。
//! 上接：`dsh-sdk-client` 的 client.rs::spawn（组装本模块与 transport）；
//!       `dshr-state` 的 raw 层（填 `HarnessSpawnConfig`：DSH_HOME/DSH_CWD/锁版本命令）。
//! 下接：tokio::process（`Command` / `Child`）、`crate::error::Error`。
//!
//! 只管"进程"本身（spawn / stderr 任务 / exit 监控 / dispose），
//! 不管协议——管道交出去后由 transport 负责对话。
//!
//! 官方对应：packages/sdk/client/src/launch.ts 的 `RuntimeProcessOptions`（= `HarnessSpawnConfig`；
//! 其 `command`/`args` 在官方由同文件的 `resolveDshLaunch` 解析出，dshr 侧由 dshr-state 的
//! runtime 层负责）、client.ts L211-221 的 `HarnessClient.start()`（官方在这里直接
//! `node:child_process` spawn，本文件把它拆出来）、packages/sdk/client/src/dispose.ts 的
//! `disposeRuntimeProcess`（= `RuntimeProcess::dispose`，由 client.ts 的 `performClose()` L404 调用）。
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
///
/// 为什么需要：stderr 尾部与 exit code 由两个后台任务写、由读循环与 dispose 读，
/// 必须共享（`Arc`）并在锁后提供快照——把 stderr 直接丢掉的话，
/// runtime 崩溃时错误信息就只剩一个退出码。
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
    ///
    /// # 返回
    /// `Some(code)` = 进程已退出（code 为 None 表示被信号终止）；`None` = 仍在运行或监控未及更新。
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().unwrap()
    }

    /// 当前 stderr 尾部（bounded，崩溃排查用）。
    ///
    /// # 返回
    /// 按时间顺序的最近若干行（上限 `STDERR_TAIL_MAX`，更早的被丢弃），克隆出的快照。
    ///
    /// 为什么需要：`TransportClosed` 只带这个尾部——它是 runtime 挂掉时唯一可读的诊断信息。
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock().unwrap().iter().cloned().collect()
    }
}

/// 启动 runtime 进程的配置。
///
/// 为什么需要：把「起哪个进程、在哪个目录、带什么环境」与「超时/收尸节奏」收成一个
/// 可序列化心智的值对象——上层（dshr-state 的 runtime/config 层）负责填，
/// 本层只负责用；三个超时字段都对应官方的可配置项，默认值由上层决定，
/// 这里**不设默认**（避免两层各有一套默认值）。
/// 官方对应：packages/sdk/client/src/launch.ts 的 `RuntimeProcessOptions`
///（`command` / `args` / `cwd` / `environment` / `requestTimeoutMs` / `disposeEofGraceMs`
/// / `disposeGraceMs`；Rust 侧把 `cwd`/`environment` 先求值成 `current_dir`/`env`，
/// 且不 port 官方的 `initializeTimeoutMs` / `shutdownTimeoutMs` 与 `description`）。
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
///
/// 为什么需要：spawn 之后必须**同时**管住三件事——子进程句柄（收尸用）、stderr 读取
///（不读会把子进程堵死）、退出监控（拿 exit code）；三者生命周期一致，绑在一个类型里
/// 才能靠 Drop/显式 dispose 保证不泄漏。注意 `dispose` 消费 `self`：收尸后本类型不可再用。
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
    /// 错误：`Error::Io`（command 不存在/无权限/current_dir 非法——注意 **node 缺失也走这里**，
    ///       当前不做 portable node 自动安装）。返回的元组字段顺序即本函数的契约：
    ///       调用方（`HarnessClient::spawn`）必须把 stdin/stdout 立刻交给 transport，
    ///       否则管道缓冲会写满并卡住子进程。
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
    ///
    /// # 参数
    /// - `eof_grace_ms`：第 1 级的等待窗口（官方 `disposeEofGraceMs`）。
    /// - `kill_grace_ms`：SIGTERM 后与强杀后的确认窗口（官方 `disposeGraceMs`）。
    ///
    /// # 返回 / 错误
    /// 任一级确认退出即 `Ok(())`；只有在**强杀后**仍未退出才 `Error::Io(TimedOut)`
    ///（此时进程可能已僵死，调用方只能认账）。
    ///
    /// 为什么需要：分层收尸是「不杀错、也不留孤儿」的唯一办法——直接 SIGKILL 会丢
    /// runtime 的落盘收尾，无脑等待又会让 UI 挂死；消费 `self` 是为了在类型上保证
    /// 收尸后不会再用已死的句柄发请求。
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
        // 注意：锁临时值先收进 let（语句末尾即释放）——if-let 模式里持有会让
        // MutexGuard 活到块结束并跨 await，使 dispose 的 future 不满足 Send。
        #[cfg(unix)]
        {
            let pid = child.lock().unwrap().id();
            if pid.is_some() {
                unsafe {
                    libc::kill(pid.unwrap() as i32, libc::SIGTERM);
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
