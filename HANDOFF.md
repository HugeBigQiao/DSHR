# DSH Rust Desktop — Handoff（记忆交接）

> 本目录内容由 `D:\DeepseekHarness\deepseek-harness` 工作区的会话导出生成。
> 会话 JSON：`session-e22d3b2c-....json`（dshr 工作区主会话，本轮讨论完整记录，10273 条）、`session-d11ded60-....json`（deepseek-harness 工作区主会话，4058 条）、`session-c016cf5d-....json`（早期小会话）。
> 读完本文件后如需要更细的上下文，直接搜索会话 JSON 里的关键词。

## 目标

用 **Rust 写一个纯桌面端客户端**（宿主应用形态）驱动 DeepSeek Harness runtime：
聊天 UI 全自绘，不套 WebView。定位 = **完整聊天客户端 + 内建监管能力**：对话/工作区/多会话/流式/工具调用展示与 web 对等，同时天然带 web 没有的**监管/调试数据主权**（每次对话 token 消耗、工具调用记录、时序、可扩展方法面）——二者共享 `session.event` 同一数据源，是同一份数据的两个视图，不是二选一。

## 已确定的决策（按讨论顺序）

1. **形态 = 宿主应用，不是插件**：Rust UI 作为前端，harness runtime 作为 sidecar 子进程，stdio JSON-RPC 通信。不 fork、不改写官方仓库。
2. **通信通道 = SDK 协议**（`packages/sdk/protocol`）：`session.event` 通知流把完整 session 日志信封流给客户端 → 监管数据第一手全量（tool/call、tool/result、request/header、usage token 明细全在）。官方另有 Python SDK（`deepseek-harness`，bundled runtime 不依赖 Node）。
3. **UI 框架倾向 Iced 0.14**（待原型验证）：理由 = API 稳定、subscription 与事件流同构、Windows/Linux 一等公民、0.14 wgpu GPU 渲染。GPUI 强在 Zed 级文本栈但 API 不稳、crates.io 滞后（0.2.x）、文档少。**待办：写两个原型对比**（5000 条消息滚动 + 每秒 20 token 流式文本），验证后再定。
4. **路线 = 渐进**：宿主应用 → 用 Rust 替换边界清晰的组件（Excel 工具、Windows 沙箱 runner）→ 未来才评估是否自研 minimal loop。彻底重写 = 疯狂，不选。
5. **更新机制**：自己的仓库 git pull/Release 自更新；runtime 走 npm/pip 依赖锁版本，**不 git pull 官方**；升级前备份 `$DSH_HOME`（pre-release 无兼容承诺）。
6. **runtime 获取（dshr 侧）**：不拷官方仓库，runtime 作为 sidecar 可执行文件在**部署/启动时**被 dshr 拉起来。分两步：**先走 node carrier**（开发期，目标机装 Node ≥22.19，spawn `node .../packaged-bin.js`）跑通 UI/协议/进程管理；**再自建 Windows 单文件 exe 构建**（fork 官方 `scripts/build-exe-for-python-sdk.ts` 的 Windows 分支，由 dshr CI 产出 exe 打进安装包）。开源仓库只放源码 + 构建脚本 + runtime 版本声明，**不 commit 任何 .exe**；exe 属发布产物（Release asset / 安装包），并需随附官方第三方 NOTICE（MIT 合规）。**A/B 双轨定位：A（node carrier）= 长期开发态，B（单文件 exe）= 发布态，非二选一**；沿用官方 `resolve_bundled_launch_args()` 双模式（自动只找 exe，node 靠 `DSH_RUNTIME_MODE=node` 显式 opt-in）——开发者跑 A、最终用户装 B。官方 node carrier 是 dev-only 非正式分发面，故发布态必须坚持 B。
7. **定位与插件边界**：dshr = **完整聊天客户端**（对话、工作区、多会话、流式、工具调用展示），监管面板是**内建差异化**而非唯一功能，二者同源 `session.event`。放弃的是 web 的 UI 可插拔性（`dsh-client-ui-slots` / `dsh-session-projection` 等渲染层插件用不了）与精致 UI 细节的完整复刻（完美 markdown、无障碍全量、第三方 UI 注入），**不是聊天功能本身**。工具/能力插件（`dsh-tool-*`、core、persistence、compaction 等）全保留——它们在 runtime 进程内跑，往 `cordis.yml` 加即可，Rust 侧零改动。审批交互通道（`ask_user_question`）是独立缺失项，既非工具也非 UI。

## 关键事实与坑（已查证）

- **审批/询问交互流在 SDK 通道是死的**：server→client 请求是 dead capability；`ask_user_question` 无 provider。要么配 `approval: never`，要么扩展协议补 server→client（这是"暴露更多方法"的第一个清单项）。
- **web_fetch 默认禁用**：当前 web profile 里 `tool-web` 配了 `fetch: false`（SSRF 未防护），`web_search` 可用（DeepSeek 官方，60s 超时）。
- **会话日志格式**：`.jsonl.zstd` = 多个独立 Zstandard frames 拼接（header 帧 + 每 append 批次一帧）。**Node 内置 zstd 只解第一个 frame**，需要按 RFC 8878 切 frame 边界逐个解（转换脚本在 `C:\Users\qiaoy\AppData\Local\Temp\dsh-session-export.cjs`，可复用）。
- **当前环境**：`DSH_HOME=C:\Users\qiaoy\.dsh`；web profile bundles = `@deepseek-ai/dsh-base` + `@deepseek-ai/dsh-web-app`；用户 patch 层为空。
- **SDK wire 协议已初步确认**：client→server 请求 3 个（`initialize` / `session/prompt` / `shutdown`）+ server→client 通知 4 个（`session.event` / `session.status` / `subagent.started` / `subagent.finished`），transport = 换行分隔 JSON-RPC 2.0（`transport.ts` ~280 行）。`session.event` 信封依赖三个富类型 `ContentBlock`(dsh-llm) / `SessionEvent`(dsh-session) / `SubagentStopReason`(dsh-subagent) → 监管数据主干，是 port 评估的核心工作量。
- **官方 bundled runtime exe 无 Windows carrier**：`deepseek-harness-runtime-bin` wheel 打进单文件 `dsh-jsonrpc-agent-pkg-<platform>-<arch>`（免 Node），但 `platforms.json` 仅 linux-x64 / linux-arm64 / macos-arm64。开发期用 node carrier（需 Node ≥22.19）；Windows exe 需自建（见决策 6）。
- **官方参考组合**：`examples/jsonrpc-agent`（minimal 组合：bash/read/write/edit/subagent/todo_write + 持久化 + compaction，无 UI 插件）。
- **Windows 沙箱现状**：TS + koffi FFI 调 Win32 API（`dsh-sandbox-windows-acl`），有 ABI 漂移痛点 → 未来 Rust 化候选。
- **Excel 工具**：Rust 生态 = calamine（读）+ rust_xlsxwriter（写），都是纯 Rust 无 C 依赖；交叉编译可用 cargo-zigbuild（Windows↔Linux），但进官方仓库需按 landlock-run 惯例每平台原生构建。

## 下一步计划（阶段 1，按序）

1. **SDK 协议 port 评估**：拉 `packages/sdk/protocol` 的方法面 + 事件类型清单，估 Rust struct 翻译量
2. **Iced vs GPUI 原型**：各写 ~30 行原型，对比长列表滚动 + 流式文本的实际性能
3. **协议客户端骨架**：JSON-RPC over stdio + 子进程生命周期（EOF→SIGTERM→SIGKILL 阶梯、崩溃恢复）
4. **审批流扩展设计**（如需要交互）：协议加 server→client 方法 + runtime 侧 TS 插件转发 `ctx.approval`
5. **监管面板（内建于聊天客户端，非独立工具）**：消费 `session.event` 流，本地算费用（token × 定价表，离线）

## 协作方式（重要）

用户正在学 Rust，**不要一次大批量生成代码**。采用小步结对：web 端讨论出思路/小片段，用户在自己编辑器里写，agent 负责 **review 检查 + 指出问题 + 给最小修复建议**。每次只推进一个小模块，避免信息过载。

## 工作区切换

新会话请在 `D:\DeepseekHarness\dshr` 目录启动（`pnpm dsh web` 或对应方式），让该目录成为 workspace root，即可直接读写本目录。
