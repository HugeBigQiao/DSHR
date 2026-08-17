// 集成测试：spawn 真实 runtime → initialize → 打印响应
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

// #[tokio::test]：测试版的 #[tokio::main]，自动启动 tokio runtime 并运行 async 函数体。
// 注意 tests/ 目录下的文件是集成测试 crate，cargo test 只会运行 #[test] 标记的函数，
// 普通 fn main 不会被调用。
#[tokio::test]
async fn init_smoke() {
    // 1. spawn：启动 node 进程，像 shell 里敲命令一样。
    //    注意 .current_dir() 必须是 deepseek-harness 根目录（tsx 要解析相对依赖）。
    //    .env() 注入 runtime 需要的环境变量。
    //    .stdin/.stdout 设成 piped = 把子进程的 stdin/stdout 变成管道，我们才能读写。
    let mut child = Command::new("node")
        .args([
            "--import",
            "tsx",
            "packages/examples/jsonrpc-demo/src/bin.ts",
            "examples/jsonrpc-agent/cordis.yml",
        ])
        .current_dir(r"D:\DeepseekHarness\deepseek-harness")
        .env("DEEPSEEK_API_KEY", "dummy-key")
        .env("DSH_CWD", r"D:\DeepseekHarness\dshr\.tmp-ws")
        .env(
            "DSH_SESSION_ROOT",
            r"D:\DeepseekHarness\dshr\.tmp-ws\.sessions",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn runtime failed");

    // 2. 从 child 里"拿走"stdin/stdout 管道（take() 把所有权拿出来，child 自己不再持有）。
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    // BufReader + lines()：按行读 stdout，这正是"换行分隔 JSON-RPC"需要的。
    let mut lines = BufReader::new(stdout).lines();

    // 3. 发 initialize 请求（注意末尾的 \n——协议是"一行一个 JSON"）。
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"cwd":"D:\\DeepseekHarness\\dshr\\.tmp-ws","provider":"deepseek-official","model":"deepseek-v4-pro","maxTokens":100}}"#;
    stdin.write_all(req.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();

    // 4. 读第一行响应并打印。
    if let Some(line) = lines.next_line().await.unwrap() {
        println!("response: {line}");
    }

    // 5. 收尾：发 shutdown，等进程退出（EOF 后 runtime 会自己退）。
    //    这行你自己写：shutdown 请求 + 杀掉进程（child.kill().await）。
    let shutdown = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":{}}"#;
    stdin.write_all(shutdown.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    let _ = lines.next_line().await; // 吃掉 shutdown 响应
    child.kill().await.unwrap();
    child.wait().await.unwrap();
}
