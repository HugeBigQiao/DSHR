use std::io::{Error, ErrorKind};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct HarnessClient {
    child: Child,
    stdin: ChildStdin,
    _stderr_task: JoinHandle<()>,
    lines: Lines<BufReader<ChildStdout>>,
}

#[derive(Debug)]
pub struct HarnessSpawnConfig {
    pub command: String,
    pub args: Vec<String>,
    pub current_dir: String,
    pub env: Vec<(String, String)>,
}

impl HarnessClient {
    pub async fn spawn(config: HarnessSpawnConfig) -> Result<Self, std::io::Error> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(&config.current_dir)
            .envs(config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // take() 拿走管道所有权，child 留着做 kill/wait;
        let stdin = child.stdin.take().expect("stdin 已 piped");
        let stderr = child.stderr.take().expect("stderr 已 piped");
        let stdout = child.stdout.take().expect("stdout 已 piped");

        // 后台 task 持续读 stderr，读到 EOF（子进程退出）才结束。
        // stderr 的所有权移进 task，字段里只留任务的句柄。
        let _stderr_task = tokio::spawn(async move {
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(err_line)) = err_lines.next_line().await {
                eprintln!("[runtime stderr] {err_line}");
            }
        });

        // stdout 的所有权被 BufReader 消费掉，所以结构体里没有单独的 stdout 字段
        let lines = BufReader::new(stdout).lines();

        Ok(Self {
            child,
            stdin,
            _stderr_task,
            lines,
        })
    }

    pub async fn initialize(&mut self, request: &str) -> Result<String, Error> {
        self.stdin.write_all(request.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        let line = self
            .lines
            .next_line()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "runtime 提前退出"))?;
        Ok(line)
    }

    pub async fn prompt(&mut self, request: &str, id: u64) -> Result<String, Error> {
        self.stdin.write_all(request.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        loop {
            let line = self
                .lines
                .next_line()
                .await?
                .ok_or_else(|| Error::new(ErrorKind::UnexpectedEof, "runtime 提前退出"))?;
            let value: serde_json::Value =
                serde_json::from_str(&line).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            if value.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return Ok(line);
            }
            println!("[notify] {line}");
        }
    }

    pub async fn shutdown(mut self) -> Result<(), Error> {
        let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":{}}"#;
        self.stdin.write_all(shutdown.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.child.kill().await?;
        self.child.wait().await?;
        Ok(())
    }
}
