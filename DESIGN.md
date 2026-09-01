# DSH Rust SDK — 设计文档（v4，单一信息源）

> 官方参考仓库：`D:\dsh\deepseek-harness`（源码是唯一权威，本文是施工蓝图 + 决策记录）。
> v4（2026-09）：在三层骨架（SDK + state + 桌面端）上继续——**SDK 主线已完成，UI 层在开发**。
> 官方 TS 客户端 `@deepseek-ai/dsh-sdk-client` 与 Python SDK 是 design twin。

## 1. 定位（一句话）

dshr = **Rust 三层**：

1. **dsh-sdk-protocol / dsh-sdk-client**：在任意 Rust 程序里驱动一个 DeepSeek Harness runtime
   子进程（`dsh --profile sdk`），stdio JSON-RPC；纯客户端库，无 UI，宿主需要 node（跑 dsh CLI）。
2. **dshr-state**：桌面端 state 层——配置 / 全程记录（WireLog）/ runtime 获取 / 会话全链路。
3. **dshr-ui**：桌面端薄壳 UI（Iced 0.14，无边框 Zed 式布局），页面设计**全部参考官方
   deepseek-harness**（token 对齐 `ui-theme/src/styles/design-platform.css` 的 `--dsw-alias-*`）。

## 2. 架构总览

```
┌───────────────────────────── 桌面端薄壳（dshr-ui，Iced）─────────────────────────────┐
│ 顶栏（Zed：页面标签 + — □ ✕ + 拖动区）                                                │
│  ┌─侧边栏─┐  ┌─对话区─┐  ┌─详情─┐   底部图标栏（任务页）                                │
│  └────────┘  └────────┘  └──────┘                                                     │
└───────────────────────────┬───────────────────────────────────────────────────────────┘
                           ▼ 事件/命令（bridge 薄层）
                  ┌──────────────────────┐
                  │ dshr-state（state 层）│  配置(config.json) / 记录(WireLog) / runtime 获取
                  └──────────────────────┘
                           ▼ spawn + stdio JSON-RPC
                  ┌──────────────────────┐
                  │ dsh-sdk-client        │  client(总装) + transport(管道) + process(生死)
                  └──────────────────────┘
                           ▼
                  dsh --profile sdk（官方 runtime，node 子进程）
```

**UI 设计原则**：页面参考官方 `packages/client/*`（AppFrame 三栏、SettingsRoot 遮罩面板、
SidebarRoot、消息气泡/工具卡片、StatsLine、composer dock）；窗口/顶栏布局参考 Zed
（标签与窗口控制同排、无边框 + 拖动区、底部图标栏）。

### 2.1 wire 方法面（7 种消息，双向）

**请求侧（client → server，3 个）：**

| method | params 类型 | result 类型 |
|---|---|---|
| `initialize` | `InitializeParams` | `InitializeResult` |
| `session/prompt` | `SessionPromptParams` | `SessionPromptResult` |
| `shutdown` | 无（wire 上 `{}`） | 空对象 `{}` |

**通知侧（server → client，4 个）：**

| method | 类型 | payload 形状 |
|---|---|---|
| `session.event` | `SessionEventNotification` | `{ sessionId, event: SessionEvent }` |
| `session.status` | `SessionStatusNotification` | `{ sessionId, status: 'idle' \| 'running' }` |
| `subagent.started` | `SubagentStartedNotification` | `{ parentSessionId, childSessionId }` |
| `subagent.finished` | `SubagentFinishedNotification` | `{ provider, agentId, parent, child, status, stopReason, lastAssistantMessage? }` |

**方向判定规则**：请求带 `id`（配对响应）；通知无 `id` 有 `method`。`rpc::classify` 按此区分。

## 3. 仓库布局（逐文件标注官方对应）

```
dshr/
├── Cargo.toml                # workspace（protocol + client + state + ui）
├── dsh/                      # dsh 本体（运行时下载，发布不带，gitignore）
├── config.json               # 本地配置（api-key/provider/model/dsh-version，gitignore）
├── data/                     # 状态数据（gitignore）：dsh-home / wire-logs / .pnpm-store
├── dsh-sdk-protocol/         # ① 协议：类型 + 帧层（纯逻辑，仅 serde）
│   └── src/
│       ├── lib.rs            # pub mod 汇总
│       ├── rpc.rs            # 帧层 ← 官方 transport.ts
│       ├── requests.rs       # 请求侧 wire 类型根 ← types.ts 的 HarnessSdkRequestMap
│       ├── requests/         #   initialize.rs / session.rs / shutdown.rs ← types.ts
│       ├── content_block.rs  # 内容块根 ← llm/types.ts 的 ContentBlockMap
│       ├── content_block/    #   contentblock.rs / fallback.rs（未知块兜底）
│       ├── session_event.rs  # SessionEvent 信封 + 判别枚举 + turn_step() ← core/session/types.ts
│       ├── session_event/    # 事件 data 按事件族拆（48 种结构化 + Unknown；新增 3 种暂走 Unknown）
│       ├── llm.rs            # TokenUsage/FinishReason/StreamChunk/LlmFailure ← llm/types.ts
│       ├── notifications.rs  # 通知侧 wire 类型 + Kind 分发 ← types.ts 的 NotificationMap
│       └── subagent.rs       # SubagentStopReason ← subagent/types.ts
├── dsh-sdk-client/           # ② 客户端：管理单个 runtime 进程
│   ├── src/
│   │   ├── lib.rs            # crate 声明（≈ 官方 client.ts 的 HarnessClient）
│   │   ├── error.rs          # 统一客户端错误（四类对应官方错误类，From 链吸收 ParseError）
│   │   ├── client.rs         # 总装师：HarnessClient 类型化方法 API
│   │   ├── transport.rs      # 管道对话：读循环 + id 配对 + 事件广播（≈ transport.ts 的 I/O 半）
│   │   ├── process.rs        # 进程生死：spawn/stderr/exit 监控/dispose 阶梯（≈ dispose.ts）
│   │   ├── subscription.rs   # 事件订阅 + 会话树 scoping（≈ client.ts 的 subscribeSessionTree）
│   │   └── api.rs            # run() receipt-to-idle（≈ api.ts 的 DeepSeekHarness.run）
│   └── tests/                # 集成测试（fake runtime 进程，见 DESIGN §5.1）
├── dshr-state/               # ③ state 层（桌面端地基）
│   ├── src/
│   │   ├── lib.rs            # 模块声明
│   │   ├── config.rs         # 配置加载（config.json：api-key/provider/model/dsh-version）
│   │   ├── record.rs         # 全程记录（一个 JSONL：cat=dsh 细到 event / cat=app 分开）
│   │   ├── runtime.rs        # runtime 获取（锁版本 pnpm install --ignore-scripts）
│   │   ├── session.rs        # 全链路运行（full round：spawn → initialize → run → shutdown）
│   │   └── main.rs           # 可执行入口（运行 + 汇总）
│   └── Cargo.toml
├── dshr-ui/                  # ④ 桌面端薄壳 UI（Iced 0.14）
│   ├── src/
│   │   ├── main.rs           # iced::application，无边框窗口（decorations:false）
│   │   ├── app.rs            # 根状态机 + 消息分发 + 窗口控制（iced::window 动作）
│   │   ├── nav.rs            # 顶栏：页面标签 + canvas 自绘 — □ ✕ + 拖动区（Zed 布局）
│   │   ├── theme.rs          # 官方设计 token → Palette（深/浅两套）+ 控件样式
│   │   ├── model.rs          # UI 数据快照（bridge 提供；state 接入后由真实桥更新）
│   │   ├── bridge.rs         # 占位桥（state 冻结期间回显；接入 dshr-state 后换真实）
│   │   ├── widgets/
│   │   │   └── popover.rs    # 覆盖式菜单（自定义 advanced widget，官方下拉形态）
│   │   ├── task.rs           # 任务页装配（侧边栏 + 对话 + 详情三区）
│   │   ├── task/
│   │   │   ├── sidebar.rs    # runtime/会话树 + ⋯ 覆盖菜单（Popover）
│   │   │   ├── chat.rs       # 消息流 + StatsLine + composer（Enter 发送）
│   │   │   └── details.rs    # 右侧详情占位
│   │   ├── monitor.rs        # 监控页（占位）
│   │   ├── setting.rs        # 配置页（左类别导航 + 右内容，官方 SettingsRoot 形态）
│   │   └── statusbar.rs      # 底部图标栏（任务页专属）
│   └── Cargo.toml            # iced features: tokio + advanced + canvas
├── DESIGN.md                 # 本文档（单一信息源）
├── README.md                 # 项目门面（三层简介 + 快速开始 + data/ 说明）
├── Cargo.lock
└── .gitignore                # 含 /dsh/、/config.json、/data/、/secrets.json
```

## 4. 协议 port 关键决策

1. **判别联合用 `#[serde(tag = "type")]`**：事件 wire 类型带斜杠（`turn/start`），每个变体显式
   `#[serde(rename = "...")]`；data 结构体驼峰处 `camelCase`；嵌套联合（如 `FinishReason`）用 `tag = "kind"`。
2. **merge-extensible 必须宽容**：信封 → 按 type 分发，未知进 `Unknown`（lossless 保留）；
   字符串联合枚举加 `#[serde(other)] Unknown`（参照 `subagent.rs` 的 `SubagentStopReason`）。
   **已实现（v3）**：`fallback.rs` 的 `known()` 助手——已知类型 data 解析失败也降级 `Unknown`
   （lossless），不整体报错；这是 `reason: 'series'` 教训的通用解法，有回归测试。
3. **transport 划分**：帧逻辑（构造/判断/解析/信封）全在 `protocol/rpc.rs`（零依赖纯函数）；
   管道 I/O + 配对在 `client/transport.rs`。
4. **错误分层**：`protocol::rpc::ParseError`（帧层）+ `client::Error`（thiserror，`From` 链吸收）——
   不建单独 error crate。
5. **事件通道结构化**：通知以 `Notification { method, params: Value }` 出通道，消费方按 method 解析。
6. **注释规范（强制）**：官方引用必须钉到**具体文件 + 类/方法/函数**（行号可加分），例如
   `packages/core/agent-loop/src/agent.ts 的 Agent.buildRequest()`、`types.ts 的 InitializeParams.reasoningEffort`。
7. **行数约束（强制）**：单文件平均 ≤350 行；超了拆文件。
8. **测试惯例（v3 起）**：协议改动必须带回归测试（2026-09-01 项目才有第一个测试
   `request_header_reason_series_parses`，此前零测试）。

## 5. 完整流程（fn 级调用链）

```
consumer 程序
│
├─ HarnessClient::spawn(config)              [client/client.rs]
│   ├─ RuntimeProcess::spawn(config)         [client/process.rs]
│   │   └─ Command::new("node").args([dsh_bin, "--profile", "sdk"])...spawn()
│   └─ Transport::start(stdin, stdout)       [client/transport.rs]
│       └─ tokio::spawn(读循环)：lines.next_line() → rpc::classify
│            ├─ Response{id} → pending.remove(id) → tx.send(Ok(line))
│            ├─ Notification{method, params} → events_tx.send(...)
│            └─ EOF → 失败所有 pending（Error::RuntimeExited）
│
├─ client.initialize(&InitializeParams)      [client/client.rs]
│   └─ transport.request("initialize", &body) → rpc::parse::<InitializeResult>
│
├─ client.prompt(&SessionPromptParams)       ← 同 initialize 路径，返回 messageId 入队回执
│
├─ 事件消费：
│   client.events().recv() → Notification{method:"session.event", params}
│   └─ notifications::parse → Kind（4 种之一；未知 method 返回 Ok(None)）
│
└─ client.shutdown()                         [client/client.rs]
    ├─ transport.request("shutdown", "{}") → parse::<ShutdownResult>
    └─ process.kill_and_wait()               （TODO：升级为官方 EOF→SIGTERM→SIGKILL 阶梯）
```

**一句话**：`client` 三行委托（序列化 → transport.request → rpc.parse），`transport` 管"写+配对"
（读循环后台常驻），`process` 管生死，`rpc` 管帧形状。

### 5.1 UI 层调用链（骨架阶段）

```
App::view ── nav(顶栏) + task/sidebar(树) + task/chat(对话) + statusbar
App::update ── Message 分发：
  ├─ Window(cmd)  → iced::window::{minimize,maximize,close,drag}(window_id)
  │                 （window_id 由 subscription 订阅 window::open_events 捕获，主窗口 Id::unique()）
  ├─ Task(⋯ 菜单) → Popover（自定义 advanced widget，见 §6.11）
  ├─ Task(Send)   → composer.text() → 占位桥本地回显（TODO：接 state → SDK）
  └─ Task(Edit)   → composer.perform(action)（Edit::Enter 除外——转 Send，见 §6.12）
```

## 6. 决策记录

1. **定位 = Rust SDK 主线**（2026-09-01）：协议 + 客户端是核心资产；官方 UI 面是 web 组件生态，
   原生重写不划算（详见 §7 事实）；SDK 直接产品化。
2. **runtime = `dsh --profile sdk`，锁版本**：npm `@deepseek-ai/dsh` 的 `latest` 是 0.1.1-rc.2
   （无 sdk profile），必须显式 `@deepseek-ai/dsh@0.1.2-alpha.3`。旧侧车包（jsonrpc-demo /
   agent-spine-demo）已从官方仓库移除（commit 244de7c18a）。
3. **DSH_HOME 独立**：spawn 时给 runtime 单独 DSH_HOME（如 `<管理目录>/home`），不碰用户 `~/.dsh`；
   工作区经 `DSH_CWD` env + `InitializeParams.cwd` 锁死。
4. **结构化范围 = 够用即可**：现有 48 个变体保留为"尽力而为的类型化视图"（`known()` 兜底，
   字段漂移自动降级 Unknown）；**不再追官方新增事件**——新事件一律 Unknown lossless，只有
   API/消费方真需要时才加变体。官方自己要求读端宽容未知（known-event-types.ts 注释）。
5. **发布策略 = 独立 crate + 生态目录**（awesome-dsh-plugin / dshget / market catalog）；
   官方树内收编等协议 1.0 稳定后（参照 python/ 进树先例）。**用户决定：发布等 SDK 全做完 + 测试完再说。**
6. **序列化兼容**：官方新增字段一律 `Option + skip_serializing_if`（wire 可选，缺省 = 旧行为）。
7. **测试为硬约束**：协议改动无回归测试不合并（v3 起）。
8. **模型自扩展是核心，UI 薄壳是正确形态**（2026-09-01）：官方给模型注册了
   `cordis_inspect_list` / `cordis_inspect_self` / `cordis_define` / `cordis_run` / `cordis_stop` /
   `cordis_remove` 工具（`packages/extensions/tool-cordis`），模型能在会话内自写自装插件。推论：
   桌面端只需"固定基础方案"的薄壳 UI（凭据/模型/策略落文件，UI 极简）；薄壳场景下原生（Iced）
   劣势消失、优势凸显（快/小/无 WebView）。**v4 落地**：UI 已成为开发主线（见 §10）。
9. **runtime 获取落地（2026-09-01 实测定案）**：dsh 本体放 **`dshr/dsh/`**（与 data/ 平级，
   发布不带，运行时检测/下载，删除可重下）；`data/` 只放状态（dsh-home、wire-logs、.pnpm-store）。
   包管理器 **pnpm**：共享全局 store 去重 + `--ignore-scripts`
   （实测 node-pty/koffi 的 tarball 自带预编译产物，跳过构建完全可用——免 node-gyp 工具链）
   + `--config.minimumReleaseAge=0`（pnpm 供应链年龄策略默认拒绝刚发布的 alpha 包）。
   node 检测（≥22.19，缺失报清晰错误；**自动安装 portable node 是下一步**）。
10. **UI 页面设计全参考官方 deepseek-harness**（用户指令，2026-09）：旧版设计作废；布局参考
    Zed（顶栏标签 + 窗口控制同排、侧边栏 runtime + 会话树、底部图标栏）。token 对齐官方
    `packages/client/ui-theme/src/styles/design-platform.css` 的 `--dsw-alias-*`
    （bg_base 21,21,23 / layer1-3 / label_primary 249,250,251 / accent deepseek-400 103,158,254 /
    border rgba(255,255,255,0.06) / bubble 44,44,46）。**描述不准时以官方源码为准。**
11. **覆盖式菜单自研（Popover，iced 无内置）**：`dshr-ui/src/widgets/popover.rs`，`features=["advanced"]`
    自定义 `Widget::overlay`。三个硬教训（对照官方 `iced_widget/src/overlay/menu.rs`）：
    - **viewport 必须传绝对坐标矩形 `layout.bounds()`**：传 `Rectangle::with_size(size)`（原点 0,0）
      会让整个菜单被渲染器裁剪掉——"画了但看不见"（首版 bug，layout/draw 日志全对，视觉全无）；
    - 锚点 = `layout.position() + translation`（视口绝对坐标，pick_list 同款）；
    - 菜单定位在宿主右下、偏移 +8（曾 0 偏移导致菜单第一项「＋ 新建」压在 ⋯ 正下方，
      点击 ⋯ 误触新建），右缘超出视口时左移钳制。
12. **Enter 发送**：iced 0.14 `text_editor` 把 Enter 发布为 `Action::Edit(Edit::Enter)`，插入换行
    是 App 收到 action 后 `content.perform()` 才执行的——`on_action` 里拦截它转 `Send`，不执行
    perform 即不插入换行。**0.14 限制：`Edit::Enter` 不携带 shift 信息，Shift+Enter 也会发送**
    （`Binding::from_key_press` 对 Enter 无条件返回 `Self::Enter`）；多行文本用中间换行。
13. **窗口控制图标 canvas 自绘**：unicode 字符（— □ ✕）在不同字体 fallback 下大小不一
    （U+25A1 在 Segoe UI 渲染偏小），用 `iced::widget::canvas`（feature "canvas"）自绘 14×14
    三个图标（横线/方框/叉），视觉统一。canvas 依赖 `lyon_path`（离线构建需先在线拉一次）。
14. **state 冻结先搭 UI**（用户决定，2026-09）：dshr-state 与 SDK 链路已验证（M2.5），UI 阶段
    bridge 用占位实现（本地回显），UI 骨架完成后再和 state/SDK 对着写真实桥。commit 暂缓。

## 7. 关键事实与坑（已查证）

- **npm latest 陷阱**：`@deepseek-ai/dsh-sdk-jsonrpc-demo` latest=0.0.1-rc.5（废弃，仓库已删）；
  `@deepseek-ai/dsh` latest=0.1.1-rc.2（无 sdk profile）。**锁版本是唯一安全路径**。
- **`reason: 'series'` 是生产事件**（0.1.2-alpha.x）：`packages/core/agent-loop/src/agent.ts` 的
  `Agent.buildRequest()` 在消息序列边界发出（goal 轮等场景必现）；严格枚举会整体解析失败（v3 已修 + 测试）。
- **会话 id 必须唯一**（R7 实测）：复用固定 id（如 "s1"）会撞上磁盘持久化日志，
  turn/end 报 `session already has a persisted log on disk ... (id collision)` error 回合。
  正式桌面端会话 id 一律唯一化（时间戳前缀）。
- **审批/询问流在 SDK 通道是死的**：`ask_user_question` 无 provider 转发；通知面固定 4 种，审批要
  runtime 侧 TS 插件转发 `ctx.approval`。
- **web_fetch 默认禁用**（SSRF 未防护），`web_search` 可用（60s 超时）。
- **会话日志 `.jsonl.zstd` = 多独立 Zstandard frames**（Node 只解第一帧，按 RFC 8878 切）；
  首行是 SessionHeader 非事件。SDK 若做历史直读要处理。
- **官方 exe 打包 Windows 是 non-goal**：Windows 上 runtime 必须有 node（或自捆 portable node）。
- **`examples/jsonrpc-agent` 已删**：其角色由 `dsh --profile sdk`（dsh-base + dsh-sdk-app）取代；
  `@deepseek-ai/dsh` 依赖含 dsh-sdk-app，npm 安装即支持 sdk profile。
- **官方 SDK 生态**：TS client（`DeepSeekHarness`/`HarnessClient`）+ Python SDK 是 design twin，
  同 runtime 同协议；ACP（`dsh --profile acp`，Zed 用）与 Claude Code/Codex hooks 是另外两条接入线。
- **iced 0.14 坑**：无 `theme::Button/Container` 枚举（用 `button::primary/secondary` + 闭包样式）；
  `Padding` 无 `[f32;4]` From；Tree 非 Clone；`Element::draw/update` 需 viewport 参数；
  无内置 popover（自定义 advanced widget）；无边框窗口用 `Window::Settings{decorations:false}` +
  `iced::window::{close,maximize,minimize,drag}`，主窗口 id 用 `window::open_events()` 订阅捕获
  （无 MAIN 常量，首个 `Id::unique()` 即主窗口）。

## 8. 待办

| 项 | 官方参照 | 状态 |
|---|---|---|
| typed errors（4 类） | `sdk/client/src/client.ts` 的 `JsonRpcResponseError` / `RequestTimeoutError` / `SdkProtocolError` / `TransportClosedError` | **完成** |
| 请求超时 | `client.ts` 的 `requestTimeoutMs` | **完成** |
| teardown ladder | `sdk/client/src/dispose.ts` | **完成**（Windows 跳过 SIGTERM） |
| 订阅 / 会话树 scoping | `client.ts` 的 `subscribeSessionTree` | **完成** |
| run() receipt-to-idle | `sdk/client/src/api.ts` 的 `DeepSeekHarness.run` | **完成** |
| SdkEncodedImageBlock / reasoningEffort 透传 | `sdk/protocol/src/types.ts` | **完成** |
| 集成测试（fake runtime） | `sdk/client/tests/fake-runtime.ts` 先例 | **完成** |
| UI 骨架（顶栏/侧边栏/对话/配置/状态栏） | `packages/client/*` + Zed 布局 | **完成**（占位桥回显） |
| UI 覆盖式菜单（Popover） | 官方下拉形态 | **完成**（viewport/偏移教训已归档 §6.11） |
| 窗口按钮 canvas 自绘 + Enter 发送 | — | **完成**（§6.12/§6.13） |
| bridge 接 dshr-state（真实数据 + 真实运行时） | state 层 | 未开始（UI 骨架完成后再做） |
| README 双语 + 发布准备 | — | 未做（发布等 SDK 全做完 + 测试完） |

## 9. 风险

- **R1 协议漂移**：0.1.x alpha 无兼容承诺 → lossless 兜底 + 锁 runtime 版本；官方发版后只核对
  7 个方法 + 已结构化事件。
- **R2 官方 TS client 永远先行**：新能力（图片等）先到 TS/Python → Rust 侧按需追。
- **R3 测试基线薄弱**：v3 起协议改动必须带测试，逐步补齐。
- **R4 Iced 0.14 前沿 API**：`Edit::Enter` 无 shift、Tree 非 Clone、无 popover 等——改动前查
  `iced_widget-0.14.2` 源码（registry 路径），以官方 widget 实现为准。

## 10. 里程碑与状态

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 dsh-sdk-protocol | 全部类型 + fallback + 帧层 | 完成（v3 同步 0.1.2-alpha.3） |
| M1 dsh-sdk-client | HarnessClient + spawn + dispose + smoke | **完成** |
| M2 API 对齐 | run / 订阅 / 会话树 / 图片 | **完成**（§8 全绿；剩发布） |
| M2.5 dshr-state 重建 | 配置/记录/runtime/全链路（真实 runtime 跑通） | **完成**（真实验证：init + 2 轮 prompt，记录 232 条 dsh + 11 条 app） |
| M3.5 UI 骨架 | 三页 + 侧边栏树 + 对话 + 覆盖菜单 + 窗口控制（占位桥） | **完成**（参考官方 + Zed；见 §6.10-13） |
| M3.6 UI 接真实数据 | bridge 填 dshr-state（真 runtime + 真记录） | 未开始 |
| M4 发布 | crate 打包 + README + 生态目录 | 未开始（用户暂缓） |
