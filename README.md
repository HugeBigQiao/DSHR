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

无边框窗口：顶栏标签（任务/监控/配置）+ 窗口控制（— □ ✕ 自绘图标）+ 拖动区；
任务页 = 侧边栏 runtime/会话树（行尾 ⋯ 覆盖菜单）+ 对话区（composer 支持 **Enter 发送**、点击 ↑ 发送）+ 底部图标栏。

### state 层验证（真实 runtime 全链路）

```bash
cargo run -p dshr-state
```

spawn 官方 runtime → initialize → 两轮 prompt → shutdown，全程写入 WireLog（见下）。

### SDK 冒烟测试

```bash
cargo test -p dsh-sdk-client --test run_smoke
```

用 fake runtime（`tests/fixtures/fake_runtime.mjs`）验证帧层/配对/超时，不依赖真实 dsh。

## 目录结构

| 路径 | 说明 |
|---|---|
| `dsh-sdk-protocol/` | 协议层：wire 类型（请求/通知/内容块/会话事件）+ 帧层，纯 serde |
| `dsh-sdk-client/` | 客户端：runtime 进程管理 + stdio 管道对话 + 事件订阅 + run() |
| `dshr-state/` | state 层：config.json 加载、WireLog 记录、runtime 获取、全链路运行 |
| `dshr-ui/` | 桌面端 UI（Iced 0.14，无边框，参考官方 deepseek-harness 页面设计） |
| `dsh/` | dsh 运行时（自动安装，发布不带，删除可重下） |
| `data/` | 状态数据（见下） |
| `config.json` | 本地配置：api-key / provider / model / dsh-version（gitignore） |
| `DESIGN.md` | 设计文档（单一信息源，含决策记录与官方源码对照） |

## data/ 目录说明

`data/` 是 dshr 的本地状态目录（gitignore，可整体删除重建，除 wire-logs 外）：

| 路径 | 内容 |
|---|---|
| `data/dsh-home/` | 每个 runtime 子进程独立的 DSH_HOME（不碰用户 `~/.dsh`）：`profiles/sdk` 是 sdk profile 的插件安装、`sessions/<工作区>/<会话id>/session.jsonl.zstd` 是会话持久化日志（zstd 压缩，多独立 frame）、`storages/` 是官方存储缓存、`.anonymous-user-id` 匿名用户 id |
| `data/wire-logs/` | **WireLog 全程记录**（JSONL，每行一个事件）：`cat:"dsh"` = 与 runtime 的每条 wire 消息（请求/响应/通知，含 dir/kind/id/method/eventType/raw）；`cat:"app"` = 应用侧事件（config.loaded / runtime.ready / spawn.start 等）。状态冻结期间 UI 发的一切消息与 dsh 返回都在这里可查 |
| `data/.pnpm-store/` | pnpm 内容寻址 store（v11，index.db + files/）——runtime 安装时 pnpm install 的共享仓库，删除后下次安装会重新拉取 |

> 会话日志命名教训：会话 id 必须唯一（R7），复用固定 id（如 `s1`）会撞上官方磁盘持久化日志
> （`session already has a persisted log on disk`），详见 `DESIGN.md §7`。

## 参考

- 官方仓库：<https://github.com/deepseek-ai/deepseek-harness>（本地镜像 `D:\dsh\deepseek-harness`，
  源码是唯一权威；页面设计 token 在 `packages/client/ui-theme/src/styles/design-platform.css`）
- 官方 TS SDK client：`packages/sdk/client`（design twin，`HarnessClient` / `DeepSeekHarness.run`）
- 官方 Python SDK：`packages/sdk/python`

## 状态

- SDK 主线（协议 + 客户端）**完成**：M0–M2 全绿（typed errors / 超时 / dispose 阶梯 / 订阅 / run /
  图片块 / 回归测试）。
- state 层 **完成**：真实 runtime 全链路验证通过。
- UI 骨架 **完成**（占位桥回显）：三页布局、runtime/会话树 + 覆盖菜单、对话 + Enter 发送、
  窗口控制自绘图标。下一步：bridge 接 dshr-state（真实数据 + 真实运行时），然后视觉打磨与发布。
