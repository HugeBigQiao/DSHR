# 测试约定（`dshr-ui`）

本目录**当前为空**。全仓测试在 2026-09-29 被一次性清空，测试将在本目录重建
（通则见 `dshr-state/tests/_conventions.md` 的前半部分）。

## 本 crate 该测什么

UI 是**薄层**（只搬运命令与事件，不解释业务），所以**不要**在这里测数据语义——
那属于 `dshr-state`。这里值得测的是「UI 自己的映射逻辑」，它们都是纯函数：

| 关注点 | 为什么 |
|---|---|
| **消息 → 视图模型** | `model::MsgView::from_item` / `TokenCounts::from_usage` 等纯转换；快照换了会话/重置时视图模型该怎样变。 |
| **格式化** | token 计数与时间的显示规则（对应官方 `token-format.ts`）；数字边界（0 / 极大值 / 缺报）。 |
| **会话标题回落** | `session_title(&None, &id)` 在无标题时用短 id——侧边栏与聊天区必须一致。 |
| **交互契约的判定函数** | 例如「何时允许发送」（仅 idle）、草稿为空不发。把这些判定抽成纯函数后即可测，无需起 UI。 |

## 不要在这里做的事

- **不要起窗口**：`cargo test` 里跑 iced 的 `run()` 会需要图形环境，CI/无头机必挂。
  iced 的 `App::update` 是可单独调用的纯状态转移，测试应当直接构造 `App` 并喂
  `Message`，不碰 `view()`/`run()`。
- **不要断言像素或布局**：那要靠人眼与截图（见 `dshr-ui/src/dpi.rs` 的实测记录）。
- **不要测渲染后端差异**（wgpu / tiny-skia）：两者在 CI 的意义不同，靠人验。

## 注意

- 本 crate 是 **bin crate**（`src/main.rs`），`tests/` 下的集成测试**无法**
  `use dshr_ui::...`——bin crate 不产出可供依赖的 lib 目标。
  若确实需要集成测试，要么先把它拆出 `src/lib.rs`，要么把逻辑下移到
  `dshr-state`（后者更符合本项目的分层意图：UI 保持薄）。
