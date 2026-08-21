# dshr-runtime

dshr 的**运行时层**：管理单个 DeepSeek Harness runtime 进程（sidecar 子进程），通过 stdio 新行分隔 JSON-RPC 驱动它。

**一个 `HarnessClient` = 一个 node 子进程**。协议形状在 `dshr-protocol`，本 crate 负责"把进程拉起来、把管道上的对话进行下去"。

## 职责边界

| 做 | 不做 |
|---|---|
| spawn 子进程、接管三根管道、kill/wait | 定义协议类型（那是 protocol 的活） |
| 发送请求 + id 配对 + 事件通道 | 业务逻辑、落库（那是 state 的活） |
| stderr 转发（防管道堵 + 崩溃可见） | UI |

## 分层（按职责不按方向）

```
client.rs     总装师：组装 process + transport，暴露类型化方法 API（≈ 官方 client.ts）
  ├─ process.rs   进程生死：spawn / stderr 任务 / kill / wait（≈ dispose.ts 简化）
  └─ transport.rs 管道对话：读循环 + id 配对 + 事件通道（≈ transport.ts 的 I/O 半）
```

- **process**：只关心"进程"本身——`Command::spawn` 拉起 node，三根 stdio 全部 piped，stderr 每行转进 mpsc 通道（`runtime_logs` 落库用）
- **transport**：只关心"管道上的对话"——后台读循环逐行 `rpc::classify`，响应按 id 配对挂起请求，通知进事件通道；EOF（runtime 退出）→ 失败所有挂起请求
- **client**：`spawn → initialize → prompt → events()/stderr() → shutdown` 的类型化入口

## 用法示例

```rust
use dshr_runtime::client::{HarnessClient, HarnessSpawnConfig};

// ① 拉起进程（阶段 A：node carrier）
let config = HarnessSpawnConfig {
    command: "node".into(),
    args: vec!["--import".into(), "tsx".into(),
               "packages/examples/jsonrpc-demo/src/bin.ts".into(),
               "examples/jsonrpc-agent/cordis.yml".into()],
    current_dir: harness_root.into(),
    env: vec![("DEEPSEEK_API_KEY".into(), key), ("DSH_CWD".into(), cwd)],
};
let mut client = HarnessClient::spawn(config).await?;

// ② 握手
let info = client.initialize(&InitializeParams { cwd, provider, model, max_tokens }).await?;

// ③ 发消息（响应是入队回执；事件随后经 events() 流出）
let result = client.prompt(&SessionPromptParams { session_id, content_blocks }).await?;

// ④ 消费事件流（state 在这里接：notifications::parse → Kind 分发）
while let Some(notification) = client.events().recv().await {
    // notification: Notification { method, params }
}

// ⑤ 收尾（正式版 dispose 阶梯 EOF→SIGTERM→SIGKILL）
client.shutdown().await?;
```

## 事件/日志通道

| 通道 | 类型 | 说明 |
|---|---|---|
| `client.events()` / `take_events()` | `mpsc<Notification{method, params}>` | 结构化通知帧（响应不经过这里，走请求返回值） |
| `client.stderr()` / `take_stderr()` | `mpsc<String>` | 进程日志行（state 写 `runtime_logs`；不读会管道堵死子进程） |

> `take_*` 把接收端 move 出来，供后台任务常驻 `tokio::select!`（state 的 RuntimeTask 就是这么用的）。

## 与官方的对应

| dshr | 官方 |
|---|---|
| `HarnessClient` | `packages/sdk/client`（进程外 SDK 客户端） |
| `transport.rs` 读循环 + 配对 | `packages/sdk/protocol/src/transport.ts` |
| `process.rs` 生死 | `packages/sdk/server` 的 dispose 语义（简化为 kill+wait） |
| 协议类型 | `dshr-protocol`（本 crate 不重复定义） |

## 测试

真实链路测试在 `dshr-state/tests/full_round.rs`（需要 `.env` + 官方仓库 `pnpm install`）：

```bash
cargo test -p dshr-state --test full_round -- --nocapture
```

## 维护提示

- **请求-响应配对是跨方向的**：不要按 send/receive 拆模块（已验证会变空壳）——配对在 transport 的 pending 表 + 读循环
- **先登记后写**：`request()` 先插 pending 再写 stdin，防响应先到丢包
- **stderr 必须一直读**：只 pipe 不读，缓冲区满子进程卡死
- **`session/prompt` 响应不保证先到**：第一次 prompt 先 `getOrCreateSession`（慢，期间发事件），按 id/method 区分而非顺序
