# dsh-sdk-client

DeepSeek Harness SDK 的 Rust 客户端层：管理一个 `dsh --profile sdk` runtime 子进程，stdio JSON-RPC 驱动。
对应官方 TS 包 `@deepseek-ai/dsh-sdk-client` 的 `HarnessClient`。

**一个 `HarnessClient` = 一个 `dsh --profile sdk` 子进程**。协议形状在 `dsh-sdk-protocol`，本 crate 负责"把进程拉起来、把管道上的对话进行下去"。

## 职责边界

| 做 | 不做 |
|---|---|
| spawn 子进程、接管三根管道、kill/wait | 定义协议类型（那是 protocol 的活） |
| 发送请求 + id 配对 + 事件通道 | 业务逻辑（消费方自己处理 `Notification`） |
| stderr 转发（防管道堵 + 崩溃可见） | UI |

## 分层（按职责不按方向）

```
client.rs       总装师：组装 process + transport，暴露类型化方法（≈ 官方 client.ts 的 HarnessClient）
   ├─ process.rs     进程生死：spawn / stderr 任务 / kill / wait + dispose 阶梯（≈ dispose.ts）
   ├─ transport.rs   管道对话：读循环 + id 配对 + 事件通道 + WireLog 落盘（≈ transport.ts 的 I/O 半）
   ├─ subscription.rs 事件订阅 + 会话树 scoping（≈ client.ts 的 subscribeSessionTree）
   ├─ api.rs         run() receipt-to-idle（≈ api.ts 的 DeepSeekHarness.run）
   └─ error.rs       统一错误（四类对应官方错误类，From 链吸收 ParseError）
```

## 用法示例（runtime = dsh --profile sdk）

```rust
use dsh_sdk_client::client::{HarnessClient, HarnessSpawnConfig};
use dsh_sdk_client::subscription::Subscription;
use dsh_sdk_protocol::requests::{InitializeParams, SdkPromptContentBlock, SessionPromptParams};

// ① 拉起 runtime：node <已装 dsh 的 bin> --profile sdk
//    dsh 安装 = npm install @deepseek-ai/dsh@0.1.2-alpha.3 --prefix <管理目录>（锁版本，勿用 latest）
let config = HarnessSpawnConfig {
    command: "node".into(),
    args: vec![
        "<管理目录>/node_modules/@deepseek-ai/dsh/lib/bin.js".into(),
        "--profile".into(), "sdk".into(),
    ],
    current_dir: "<管理目录>".into(),
    env: vec![
        ("DEEPSEEK_API_KEY".into(), key),
        ("DSH_HOME".into(), "<管理目录>/home"),   // 独立 home，别碰用户 ~/.dsh
        ("DSH_CWD".into(), cwd),
    ],
    request_timeout_ms: 30_000,       // 官方 requestTimeoutMs
    dispose_eof_grace_ms: 2_000,      // 官方 disposeEofGraceMs（stdin EOF 后等协作退出）
    dispose_kill_grace_ms: 1_000,     // 官方 disposeGraceMs（SIGTERM/强杀后确认窗口）
    wire_log_path: None,              // Some(path)：双向全量消息落盘 JSONL（排查/监管）
};
let mut client = HarnessClient::spawn(config).await?;

// ② 握手（provider/model/reasoningEffort/maxTokens）
let info = client.initialize(&InitializeParams {
    cwd: cwd.into(),
    provider: "deepseek-official".into(),
    model: "deepseek-v4-flash".into(),
    reasoning_effort: None,
    max_tokens: None,
}).await?;

// ③ 发消息（响应是入队回执；事件随后经订阅流出）
let result = client.prompt(&SessionPromptParams {
    session_id: "s1".into(),
    content_blocks: vec![SdkPromptContentBlock::text("hi")],  // 图片：SdkPromptContentBlock::image(...)
}).await?;

// ③' 或直接用 run()：prompt → 等 inbox 回执 → 收集事件到根会话 idle → finalResponse
// let result = client.run("s1", vec![SdkPromptContentBlock::text("hi")], 120_000).await?;

// ④ 订阅事件流（会话树 scoping：root + subagent 血缘后代）
let mut sub = Subscription::scoped(client.subscribe(), "s1");
while let Ok(notification) = sub.next().await {
    // Notification { method, params }
}

// ⑤ 收尾（dispose 阶梯：EOF → [SIGTERM] → SIGKILL，见 DESIGN.md §8）
client.shutdown().await?;
```

## 事件/日志通道

| 通道 | 类型 | 说明 |
|---|---|---|
| `events()` | `mpsc::UnboundedReceiver<Notification>` | 结构化通知帧（session.event / session.status / subagent.*） |
| `stderr()` | `mpsc::UnboundedReceiver<String>` | runtime stderr 逐行（崩溃排查） |

## 状态（M0–M2 全完成，见 DESIGN.md §8/§10）

已做：spawn / initialize / prompt / shutdown、id 配对 + 请求超时、事件通道、stderr 转发、
typed errors 四类、dispose 阶梯（EOF→SIGTERM→SIGKILL，Windows 跳过 SIGTERM）、订阅 / 会话树
scoping、run() receipt-to-idle、SdkEncodedImageBlock / reasoningEffort 透传、fake-runtime 集成测试
（`tests/run_smoke.rs`，跑 `cargo test -p dsh-sdk-client --test run_smoke`）。

未做：crate 发布（等 SDK 全做完 + 测试完，见 DESIGN.md §6.5）。
