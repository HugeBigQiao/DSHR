# 测试约定（`dsh-sdk-protocol`）

测试按各 crate 的 `tests/` 目录重建（约定见 `dshr-state/tests/_conventions.md` 的前半部分）。

## 已落地（2026-09-29）

| 文件 | 条数 | 覆盖 |
|---|---|---|
| `event_catalog.rs` | 5 | 枚举声明的类型集合 == 官方快照（59 种，无重复）、未知类型 lossless、**降级识别**（`degraded_event`）、merge-extensible 兜底、`TurnEndCancelCause` 无兜底的**已知取舍** |
| `frame_shape.rs` | 6 | **真实录制帧**的结构化对账、20 种消息来源 kind 建模（含漂移容忍）、内容块 roundtrip（8 种）、请求面/信封/错误分流 |

**消息来源的形状对账不在本目录**：`scripts/scan-message-sources.mjs`（仓库级脚本）才是
**发现机制**——它扫已安装 runtime 的代码字面量 + typert 合并声明 + wire-log 观测，
能报出「新增 kind」与「必填/可选不一致」。这里的测试只能守 fixture 样本里**已知**的 kind。
换 runtime 版本后跑一次那个脚本（详见 AI-LOG §4.5 与 `_conventions` 上游约定）。

`fixtures/` 里两个文件都是**提交进仓库的测试输入**：

- `known-event-types.txt`：官方 `known-event-types.ts` 的 59 个事件名快照（含更新时机说明）；
- `wire-log-sample.jsonl`：从 `dshr/data/wire-logs/*.jsonl` 按
  **(通知方法, 事件类型, 消息来源 kind)** 粒度各抽一行的真实帧（18 行 / ~31 KB）。
  **`data/` 是 gitignore 的**，所以样本必须落成 fixture 才能在干净克隆里跑。

## 本 crate 该测什么

`dsh-sdk-protocol` 是**纯逻辑的 wire 协议移植**（不碰进程、不碰 I/O），
因此它是最适合「一类断言一个文件」的 crate。重建时建议至少覆盖：

| 关注点 | 为什么值得单独一个文件 |
|---|---|
| **事件全集对账** | `session_event` 的分发臂必须与官方 `known-event-types.ts` 一一对应（当前 59 种）。这是**协议漂移的头号风险**，官方每次发版都要重跑（仓库已有脚本 `scripts/compare-session-events.mjs` 做集合对账，测试侧应补形状对账）。 |
| **未知事件降级** | `fallback::known()` 在 data 形状不匹配时降级为 `Unknown`——这是**刻意的容错**，但也是「测试静默通过」的温床：必须有一条测试证明「形状对了会解析成结构化变体」，否则拼错字段也看不出来。 |
| **内容块 roundtrip** | 7 种内容块（含 0.1.7 新增的 `tool-addition`/`tool-removal`）的序列化/反序列化对称性。 |
| **合并扩展性原则** | `TurnEndReason` / `MessageRole` 带 `#[serde(other)]`——官方加新值时必须仍能解析（不能抛错）。 |
| **请求面只有 3 个方法** | `initialize` / `session/prompt` / `shutdown`。多一个都要改协议面文档（`DESIGN.md` §4.1）。 |

## 注意

- 本 crate 的**所有**内容都是 `pub`，所以测试可以直接用 `dsh_sdk_protocol::...` 路径，
  不需要任何测试接缝。
- 内置的**静态 fixture**（录制的事件 JSON）建议放 `tests/fixtures/`，用
  `include_str!` 或运行期读取；不要内联巨大的 JSON 字符串到代码里。
