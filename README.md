# dshr — DeepSeek Harness Rust SDK + 桌面端

用 Rust 驱动 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 的官方 runtime
（`dsh --profile sdk`），并提供一个原生桌面端薄壳。

三层结构：

```
dshr-ui（Iced 0.14 桌面端薄壳，参考官方 UI + Zed 布局）
   ↓ 事件/命令
dshr-state（配置 / WireLog 记录 / runtime 获取 / 会话全链路）
   ↓ spawn + stdio JSON-RPC
dsh-sdk-client + dsh-sdk-protocol（类型化客户端 + 协议帧层，纯 Rust）
   ↓
dsh --profile sdk（官方 runtime，node 子进程）
```

## 快速开始

环境要求：Rust 1.85+、Node.js ≥ 22.19（跑官方 dsh CLI 用）。

### 桌面端 UI（开发中，占位桥回显）

```bash
cargo run -p dshr-ui
```

无边框窗口：顶栏标签（任务/文件/监控/配置）+ 窗口控制（— □ ✕ 自绘图标）+ 拖动区；
任务页 = 侧边栏 runtime/会话树（行尾 ⋯ 覆盖菜单）+ 对话区（composer 支持 **Enter 发送**、点击 ↑ 发送）+ 底部图标栏。

渲染后端与编辑器按 feature 选择（默认即推荐配置）：

| 命令 | 渲染 | 编辑器 | 启动常驻（release，实测 private） |
|---|---|---|---|
| `cargo run -p dshr-ui` | **wgpu（GPU，失败自动退回 tiny-skia）** | iced-code-editor（高亮/折叠/行号） | ≈ 97 MB |
| `$env:WGPU_BACKEND='gl'; cargo run -p dshr-ui` | wgpu 走 OpenGL 后端 | 同上 | **≈ 52 MB** |
| `cargo run -p dshr-ui --no-default-features --features code-editor` | tiny-skia（纯 CPU） | 同上 | **≈ 19 MB**（但弱机/高缩放下滚动会卡） |
| `cargo run -p dshr-ui --no-default-features` | tiny-skia（纯 CPU） | iced 自带 `text_editor`（纯文本兜底） | ≈ 19 MB |

> **为什么默认回到 wgpu**：tiny-skia 是 CPU 光栅化，滚动时整窗重绘在弱机/高缩放下会直接卡死
> （实测同一台机器：tiny-skia 滚轮卡住，wgpu 正常）。常驻差距可以用 `WGPU_BACKEND=gl` 补回来一半。
> 代价是 exe 从 23.8 MB 涨到 29.9 MB，且依赖树重新拉进 wgpu（编译时间 +4 分钟）。
>
> ⚠️ `iced-code-editor` 是路径依赖（`../包/iced-code-editor`），其 `Cargo.toml` 已打补丁
> 关闭 `iced` 的默认 features；**重新下载该仓库会让补丁丢失**（见 DESIGN.md §9.16）。

> ⚠️ Windows 上如果出现「UI 被切掉一半 / 顶栏右侧看不到窗口按钮 / 打开文件后极卡」，
> 多半是**系统 DPI 与显示器 DPI 不一致**（如系统 96、显示器 192）。两害相权：
> `DSHR_DPI=auto`（默认，进程设 DPI-unaware：UI 完整但被系统拉伸发糊）、
> `DSHR_DPI=aware`（清晰但 UI 右/下被切一半，**且 wgpu 常驻翻倍到 ≈185 MB**）。
> 详见 `dshr-ui/src/dpi.rs` 与 DESIGN.md §9.17。
> 诊断开关：`DSHR_AUTO_OPEN=dshr/Cargo.toml` 启动即打开指定工作区文件。

### state 层验证（真实 runtime 全链路）

```bash
cargo run -p dshr-ui
```

⚠️ **`dshr-state` 不再提供可执行文件**（`src/main.rs` 与 `src/session.rs` 已删除）：
它的驱动逻辑与 `raw` 层重复，而「启动一个 runtime 并跑一轮」现在由 UI 入口
（`dshr-ui/src/main.rs` → bridge 总线 → engine）覆盖同一路径。
本项目的入口**只有 `dshr-ui` 一个**。

真实验证需要 API key 且消耗额度，只在低谷期做（见 `DESIGN.md` §12）。

### 验证状态（测试重建中：3/5）

```bash
cargo test --workspace     # 当前 45 条契约测试（全部注入式/离线，不烧 token）
cargo build --workspace --all-targets
cargo fmt --all -- --check
node ../scripts/scan-message-sources.mjs   # 消息来源形状对账（换 runtime 版本后跑一次）

# 导出落盘数据 + 会话历史（CSV）到 data/exports/：PowerShell 用 $env:，bash 用 DSHR_EXPORT=1 前缀
$env:DSHR_EXPORT='1'; cargo test -p dshr-state --test export_csv -- --nocapture
```

测试于 2026-09-29 一次性清空后，正按「**各 crate 一个 `tests/` 目录**」重建（不再散落在 `src/` 里）。
已落地的是**优先级最高的三项**（详见 [`DESIGN.md` §12.4](DESIGN.md)）：

| crate | 测试文件 | 覆盖 |
|---|---|---|
| `dshr-state` | `tests/engine_flow.rs`（8） | engine 主链路、多 runtime 路由隔离、stderr/退村落盘、空注册表不空转、降级写 app 轨迹、请求事实与失败可见 |
| `dshr-state` | `tests/fold_projection.rs`（10） | fold 投影语义 + 在线/离线同源同巡 + 模型请求可见 + 行级 turn/step/source/error（含注入与尝试成行） |
| `dshr-state` | `tests/store_persistence.rs`（6） | 落库幂等、替换语义、多会话隔离、**复原往返（逐字段相等）** |
| `dshr-state` | `tests/export_csv.rs`（5） | CSV 转义、八张表导出、跨会话历史回放、全量导出 + 复原抽查 |
| `dsh-sdk-protocol` | `tests/event_catalog.rs`（5） | 59 事件全集对账（对锁定快照）、降级识别、merge-extensible 兜底 |
| `dsh-sdk-protocol` | `tests/frame_shape.rs`（6） | **真实录制帧**形状对账、20 种消息来源建模、内容块 roundtrip |

未重建：`dsh-sdk-client`（帧层/配对/超时，需 node fixture）、`dshr-ui`（纯映射函数）。
各 crate 目录里的 `_conventions.md` 写明该测什么、以及**已预留的测试接缝**：

| crate | 约定文件 |
|---|---|
| `dshr-state` | [`tests/_conventions.md`](dshr-state/tests/_conventions.md)（含接缝清单与踩坑） |
| `dsh-sdk-protocol` | [`tests/_conventions.md`](dsh-sdk-protocol/tests/_conventions.md) |
| `dsh-sdk-client` | [`tests/_conventions.md`](dsh-sdk-client/tests/_conventions.md) |
| `dshr-ui` | [`tests/_conventions.md`](dshr-ui/tests/_conventions.md) |

## 目录结构

| 路径 | 说明 |
|---|---|
| `dsh-sdk-protocol/` | 协议层：wire 类型（请求/通知/内容块/会话事件）+ 帧层，纯 serde |
| `dsh-sdk-client/` | 客户端：runtime 进程管理 + stdio 管道对话 + 事件订阅 + run() |
| `dshr-state/` | 状态层（**内部分三层：raw / engine / fold**，详见 `dshr-state/README.md`）：与 SDK 沟通、数据处理与落库、配置/密钥/WireLog/runtime 获取/工作区文件读写 |
| `dshr-ui/` | 桌面端 UI（Iced 0.14，默认 wgpu 渲染，无边框，参考官方 deepseek-harness 页面设计）；文件页 = 工作区文件树 + 代码编辑器 |
| `dsh/` | dsh 运行时（自动安装，发布不带，删除可重下） |
| `data/` | 状态数据（见下） |
| `config.json` | 本地配置：provider / model / dsh-version（gitignore） |
| `DESIGN.md` | **整体设计单一真源**：架构、边界、决策、操作约束 |
| `AI-LOG.md` | **交流记录 / 踩坑 / 设想**：为什么变成这样、下一步想做什么（过程） |
| `dshr-state/README.md` | state crate 的组成结构与依赖关系 |

## data/ 目录说明

`data/` 是 dshr 的本地状态目录（gitignore，可整体删除重建，除 wire-logs 外）：

| 路径 | 内容 |
|---|---|
| `data/dsh-home/` | 每个 runtime 子进程独立的 DSH_HOME（不碰用户 `~/.dsh`）：`profiles/sdk` 是 sdk profile 的插件安装、`sessions/<工作区>/<会话id>/session.jsonl.zstd` 是会话持久化日志（zstd 压缩，多独立 frame）、`storages/` 是官方存储缓存、`.anonymous-user-id` 匿名用户 id |
| `data/wire-logs/` | **WireLog 全程记录**（JSONL，每行一个事件）：`cat:"dsh"` = 与 runtime 的每条 wire 消息（请求/响应/通知，含 dir/kind/id/method/eventType/raw）；`cat:"app"` = 应用侧事件（config.loaded / runtime.ready / spawn.start 等）。状态冻结期间 UI 发的一切消息与 dsh 返回都在这里可查 |
| `data/.pnpm-store/` | pnpm 内容寻址 store（v11，index.db + files/）——runtime 安装时 pnpm install 的共享仓库，删除后下次安装会重新拉取 |
| `data/exports/` | CSV 导出产物（`DSHR_EXPORT=1` 真跑；库表 + 跨会话语历史，可整目录删除） |
| `data/secrets.json` | API key 本地密钥（Unix 0600；不再写入 config.json） |

> 会话日志命名教训：会话 id 必须唯一（R7），复用固定 id（如 `s1`）会撞上官方磁盘持久化日志
> （`session already has a persisted log on disk`），详见 `DESIGN.md §10`。

## 参考

- 官方仓库：<https://github.com/deepseek-ai/deepseek-harness>（本地镜像 `D:\dsh\deepseek-harness`，
  源码是唯一权威；页面设计 token 在 `packages/client/ui-theme/src/styles/design-platform.css`）
- 官方 TS SDK client：`packages/sdk/client`（design twin，`HarnessClient` / `DeepSeekHarness.run`）
- 官方 Python SDK：`python/sdk`

## 状态

- SDK 主线（协议 + 客户端）**完成**：已同步 deepseek-harness `0.1.7-rc.2` 的 59 种已知事件；
  typed errors / 超时 / dispose 阶梯 / 订阅 / run / 图片块 / 契约测试全绿。
  （0.1.7 增量：`developer/message`、`image/offload`、`workspace/changes` 三个事件；
  内容块新增 `tool-addition`/`tool-removal` 与 `ImageBlock.offloaded`；`turn/end` 新增 `forked` 原因。
  可用 `node scripts/compare-session-events.mjs` 与官方真源做机器化比对。）
- **2026-09-29 修掉两个「不写测试就看不见」的问题**：① `Engine::next()` 在没有任何 runtime 时
  立即返回（总线空转烧一个核，界面毫无异常）；② `MessageSource` 漏移植官方 kind →
  相关消息**整条**降级 `Unknown`（先补 `system-prompt`/`runtime-context`，再按取证补齐
  `plan-mode`/`model-selection`/`user-approval`/`ptc-mode`/`compact-checkpoint`，共 20 种）。
  均由新建的契约测试抓出（见 `DESIGN.md` §4.4 / §10 / §12.4）。
- **降级不再静默**：`Unknown.degraded` 区分「已知类型解析失败」（协议漂移，engine 写
  `event.degraded` 轨迹）与「类型本身就未知」（预期内）；`scripts/scan-message-sources.mjs`
  负责**发现**新 kind 与字段漂移（测试只能守已知样本）。
- **按需拉取 + 历史复原的数据面就绪**（M6 的一半）：`EngineCmd::ReadSnapshot` 双路查找
  （运行中 → 库里复原）、`ListSessions` 给出会话目录；UI 侧切换（事件只发轻通知 + 按需拉）
  与多 runtime 一起做（M5），理由见 `DESIGN.md` §7.2。
- **落库粒度 = 除逐 chunk 之外全落**（2026-09-29）：`messages` 表逐条记对话（含程序化注入与
  未提交的尝试）、turn/step/source 归属、工具全文与失败原因；`requests` 表记每次 prompt 的
  耗时/成败/**失败原因**。**关掉再打开可复原**：`Store::load_snapshot` 从库读回完整快照
  （契约测试断言逐字段相等）。原文不截断（截断只发生在渲染层）。
- state 层 **完成**：engine / fold / SQLite / WireLog 全链路验证通过；runtime 会校验安装版本，
  版本不匹配时自动 `pnpm install --force` 升级。
- UI **主路径完成**：任务页真实 bridge、会话/工具卡片、设置页、composer + Enter 发送、窗口控制；
  监控页、右侧详情栏、多 runtime/多 session 仍为后续里程碑。
- 安全：API key 已迁移到 `data/secrets.json`；为空时设置页和状态栏给出警告，Real 模式自动回退 Fake。
- 流式：协议已解析 v3 `assistant/message.stream` 并展示流统计；真正逐 token 直播依赖上游 live 通知，
  当前 SDK 事件模型下只能在 settlement 后回放/统计。
