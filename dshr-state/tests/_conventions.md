# 测试约定（`dshr-state`）

本目录按「各 crate 一个 `tests/` 目录」重建（全仓测试在 2026-09-29 被一次性清空，
理由见 `dshr/DESIGN.md` §12.4）。

## 已落地（2026-09-29）

| 文件 | 条数 | 覆盖 |
|---|---|---|
| `engine_flow.rs` | 11 | engine 主链路（prompt→通知→折叠→落库）、多 runtime 路由隔离、stderr/退村落盘、**空注册表不空转**、帧形状自检、**降级写入 app 轨迹**、**请求事实与发送失败可见**、**按需拉取（运行中/库里复原/会话目录）** |
| `fold_projection.rs` | 10 | fold 投影语义（消息序/工具配对/token/轮结算/错误口径）+ **在线与 WireLog 回放同源同巡** + **模型请求可见** + **行级 turn/step/source/error 与注入/尝试成行** |
| `store_persistence.rs` | 6 | 落库幂等、替换语义、空 `session_id` 拒绝、多会话隔离、**复原往返（逐字段相等）**、缺失会话为 `None` |
| `export_csv.rs` | 5 | CSV 转义（RFC 4180）、**八张事实表**导出、**跨会话回放分组**、`export_all` 端到端、`DSHR_EXPORT=1` 真跑（含复原抽查） |
| `engine_session.rs` | 2 | 真实会话逐步透明账本（`DSHR_LIVE=1`）+ 冷启动负例 |

**还没测的**（重建优先级第 4/5 项是 client 与 ui；本 crate 内还缺的）：
`runtime.rs`（版本校验/pnpm 回退链，需要能离线跑的替身）、`record.rs`（WireLog 行格式）、
`workspace.rs`（工作区内相对路径边界）、`config.rs` / `secrets.rs`（含**BOM 陷阱**：直接写
`data/secrets.json` 的测试要小心别用 PowerShell 生成输入）。

## 两类测试（分清楚，别混）

| 类型 | 文件 | 起进程？ | 烧 token？ | 何时跑 |
|---|---|---|---|---|
| **契约/集成**（注入假 driver） | `<关注点>.rs` | 否 | 否 | 每次 `cargo test` |
| **真实会话**（与 dsh 沟通） | `engine_session.rs` 的 `live_*` | 是（官方 dsh） | **是** | 显式 `DSHR_LIVE=1`，低谷期 |

真实会话测试**必须**用环境变量门控 + 无条件 `return` 跳过（不要用 `#[ignore]`：
用户 2026-09-29 的要求是「测试直接跑与 dsh 沟通的实际路径」，门控环境变量比
`#[ignore]` 更显式，且能在没配密钥时自动跳过）。

## 为什么清空

旧测试是**按文件散落**的：一部分内联在 `src/*.rs` 里（`#[cfg(test)] mod tests`），
一部分是 `src/<模块>/tests.rs`。这种布局有三个问题：

1. **测试与实现抢同一文件的行数预算**。本项目对单文件行数有约束（≤350 行为宜），
   内联测试会把已经接近上限的文件顶破——重构时被迫在「拆实现」与「拆测试」之间二选一。
2. **改实现时会顺手改到测试**，两者在同一文件的 diff 里纠缠，review 分不清
   「行为变了」还是「断言跟着改了」。
3. **契约测试放错了地方**。像「wire 帧形状能否解析」「落库是否幂等」这类
   **对外契约**，用集成测试（`tests/`）表达才正确——它们应该只依赖 crate 的
   公开接口，从而能在重构内部结构时保持不动。

## 新约定

| 测试类型 | 放哪 | 依赖什么 |
|---|---|---|
| **契约/集成**（对外行为、跨模块链路） | 本目录，一个关注点一个文件 | 只用 `pub` 接口；`dshr_state::...` 路径调用 |
| **纯内部单元**（私有函数边界） | 允许内联 `#[cfg(test)] mod tests`，但**仅当**该文件离行数上限还有余量 | 可用私有项 |

命名：`<关注点>.rs`，如 `engine_routing.rs` / `store_persistence.rs` / `fold_projection.rs`。
**不要**再建 `src/<模块>/tests.rs`。

## 测试接缝已改为**无门控的正式 API**

2026-09-29 用户要求取消 `#[cfg(test)]`（「以后测试都是直接跑最终和 dsh 沟通的实际测试了，
直接通过 test 块来模拟触发和接收就可以了」）。因此下列方法**在所有构建下都存在**：

| 接缝 | 用途 |
|---|---|
| `raw::Runtime::with_driver_for_test` | 注入假 driver + 通知流，**不 spawn 进程**即可驱动 raw 层 |
| `raw::Runtime::inject_stderr_for_test` | 注入 stderr 流（验证 stderr 落库链路） |
| `raw::Runtime::force_fake` | 跳过 `config.json` 判定，强制 Fake 模式 |
| `raw::Runtime::mark_exited_for_test` | 模拟进程退出（协议通知不含退出信息，只能这样造） |
| `engine::Engine::register_injected_runtime[_with_stderr]` | 往注册表注入「已就绪」runtime（engine 的唯一入口） |
| `engine::Engine::{with_db, store_ref, recorder_ref}` | 注入内存库、断言落库与 app 轨迹 |
| `engine::Engine::mark_runtime_exited_for_test` | 从 engine 侧模拟 runtime 退出 |

**设计含义**：它们不再是「测试专用代码」，而是**注入式测试基础设施**——正式 API 的一部分。
代价是生产二进制里也包含它们（几行、无副作用）；收益是测试与生产走**同一条代码路径**，
不存在「cfg 掉了才发现生产路径没编译」这类问题。
`injected` 字段已在取消门控时**删除**——它是只写不读的死字段，正是取消 `cfg(test)` 才暴露出来的。

假 driver 的写法：实现 `raw::SessionDriver`（5 个方法 + `BoxFuture`），
只回 `Ok(...)`，把通知流的发送端留在测试里用来制造事件。

## 三条踩过的坑（重建测试时先读）

1. **`assistant/message` 帧的必填字段**：`data` 需 `turn` / `step` / `message`
   （`message` 内含 `id`/`role`/`content`/`source`），且 `source.kind = "model"`
   **必须带 `provider` 与 `model`**。漏字段**不会报错**——`known()` 反序列化失败会
   静默降级成 `Unknown`，测试表现为「事件没反应、等超时」。建议保留一条
   「帧形状自检」测试专门钉住它。
2. **广播通道的生命周期**：假 driver 持有自己的发送端克隆，所以测试 drop 手里的
   发送端**关不掉**流。要模拟「进程退出」用 `mark_exited_for_test`，不要试图靠
   drop 发送端。
3. **一条测试只验一件事**。测试助手的推进循环有副作用（可能让 engine `teardown`
   并移除 runtime），把它和后续断言写在一条测试里会互相吃掉状态。

## 运行

```bash
cargo test -p dshr-state                      # 契约测试（默认，不烧 token）
cargo test --workspace                        # 全仓
$env:DSHR_LIVE=1; cargo test -p dshr-state --test engine_session -- --nocapture
                                              # 真实会话 + 逐步透明账本（烧 token）
```

