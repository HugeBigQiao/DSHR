# 测试约定（`dsh-sdk-client`）

本目录**当前为空**（原先有 `run_smoke.rs`，已随全仓测试清空一并删除）。
测试将在本目录重新建立（通则见 `dshr-state/tests/_conventions.md` 的前半部分）。

## 本 crate 该测什么

它是**唯一碰进程与管道的 crate**，所以测试分两类，边界要清楚：

| 类别 | 做法 | 要不要真进程 |
|---|---|---|
| **协议配对/超时语义** | 起 `tests/fixtures/fake_runtime.mjs`（node 脚本，官方 `fake-runtime.ts` 的等价物）当服务端，验证 id 配对、请求超时、EOF→`TransportClosed`、广播 lagging | 要（但只是 node 脚本，不烧 token） |
| **纯逻辑**（分类/解析/状态转移） | 直接调函数，不需要进程 | 不要 |

**不要**在本目录做「真 dsh runtime」的测试：那属于 `dshr-state` 的端到端验证，
且需要 API key、消耗额度（见 `DESIGN.md` §12 的验证策略）。

## 可用的 fixture

`tests/fixtures/fake_runtime.mjs` —— 若在清空时被一并删除，可从官方
`packages/sdk/client/tests/fake-runtime.ts` 重新移植（协议已对齐到
`0.1.7-rc.2`）；它按 stdio JSON-RPC 应答 `initialize` / `session/prompt` /
`shutdown`，并发出 4 种通知。

## 注意

- **退出状态只能查询**：`HarnessClient::runtime_status()` 返回 `Arc<RuntimeStatus>`
  （`exit_code()` / `stderr_tail()`）。协议 4 种通知**不含**退出信息，
  断言退出必须走这个句柄。
- **stderr 是单消费者流**：`take_stderr()` 只能取到一次，第二次拿到的是空通道。
