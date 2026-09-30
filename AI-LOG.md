# AI-LOG — 交流记录 / 踩坑 / 设想

> **定位**：本文件记录「过程」，`DESIGN.md` 记录「结论」。
> - 想知道**现在**是什么样、边界在哪 → 看 [`DESIGN.md`](DESIGN.md)
> - 想知道**为什么**变成这样、踩过什么坑、下一步想做什么 → 看本文件
>
> 本文件是为**跨会话共享**写的：换一个 AI 会话（或换人）接手时，先读 DESIGN.md 建立整体认识，
> 再读本文件了解来龙去脉与未落地的设想，避免重复踩坑或推翻已定结论。
>
> 组织方式：按**主题**（不是按时间）分节，每节内部时间倒序。带 ✅ 的是已落地，带 💡 的是设想/未做。

## 目录

1. [协作方式约定](#1-协作方式约定)
2. [踩坑记录（按主题）](#2-踩坑记录按主题)
3. [决策简史](#3-决策简史)
4. [未落地的设想](#4-未落地的设想)
5. [与官方仓库对照的方法论](#5-与官方仓库对照的方法论)
6. [环境与工具链事实](#6-环境与工具链事实)

---

## 1. 协作方式约定

### 1.1 沟通与产出

- 用户以中文交流，文档与注释统一用中文；**官方引用必须钉到具体文件 + 类/方法/函数**（行号可加分）。
- 用户明确要求：**不要为了"看起来完整"而写空文档**；每一节都要有真实内容，没有就写"暂无"。
- 关键决策由用户拍板，AI 负责把选项、代价、推荐理由摆清楚；**不替用户做产品决策**。
- 用户的工作节奏是「小步 + 每步可验证」：每完成一步都要求 `cargo test` 全绿再进下一步。

### 1.2 文档分工（2026-09-28 定）

| 文件 | 定位 | 读者 |
|---|---|---|
| `DESIGN.md` | 整体设计**单一真源**：当前架构、边界、决策、操作约束 | 任何人（含新会话 AI）先读它 |
| `AI-LOG.md`（本文件） | 过程：交流、踩坑、设想、决策简史 | 接手者了解来龙去脉 |
| `dshr-state/README.md` | 该 crate 的组成结构与依赖关系 | 改 state 的人 |
| `<crate>/tests/_conventions.md` | **该 crate 的测试约定**：该测什么、已预留的测试接缝、踩过的坑 | 重建测试的人 |
| `scripts/pipeline.json` | 步骤索引：每个步骤**为什么**用那个脚本 | 执行/复现的人 |
| `scripts/README.md` | 脚本目录约定与步骤总览表 | 同上 |

**原则**：DESIGN 只描述**当前**状态；被推翻的旧方案不留在 DESIGN 里（只留结论），
其「为什么被推翻」写进本文件的决策简史。

**测试重建中的阅读顺序**（2026-09-29）：`DESIGN.md` §12.4（现状 + 已落地清单 + 重建优先级）
→ 目标 crate 的 `tests/_conventions.md`（接缝清单）→ 本文件 §2.6（静默失败类陷阱）。
**进度 3/5**：`dshr-state`（engine/fold/store/export）与 `dsh-sdk-protocol` 已重建（45 条绿）；
剩下 `dsh-sdk-client`（帧层/配对/超时，需 node fixture）与 `dshr-ui`（纯映射函数）。

**S1/S4 的当前进度**（2026-09-29）：M1–M3 已完成；**M6 的数据面完成**（按需拉取 + 库里复原，
§4.9 有 M5 的逐条待办清单）；S4 数据侧完成（逐条落库 + 失败原因 + CSV 导出 + 关掉再打开可复原，
§4.8）。**下一步是 M5（UI：多 runtime/多会话 + 轻通知切换 + 历史会话 + 等待可见显示）**——
用户要求「UI 之前先把核心逻辑做完」，核心逻辑这条线现在没有前置债了。

**另有两组「只记不做」的待拍板项**（都在等你的决定，与上面主线不冲突）：
§4.10 注释梳理顺出的两处代码与注释不一致（`last_persisted` write-only、`infer_op` 分支不可达）；
§4.11 文件页/`state::workspace` 的 6 条缺口决策（缺什么、代价都列了，DESIGN §7.4 有现状表）。

### 1.3 代码注释规范（用户 2026-09-28 要求；2026-09-30 补充块级要求）

- **文件级**：这个文件干什么、主要用途、为什么需要它、**上接**谁（谁调用/依赖它）、**下接**谁（它调用/依赖谁）。
- **函数 / trait 级**：为什么需要它、输入输出是什么、主要功能是什么。
- **函数内部要下沉到块级**（2026-09-30 用户明确要求）：每个成块的逻辑（一个分支、一趟循环、
  一次查询、一段拼装）前面写清三件事——
  1. **用了什么方法**（这里在做什么：先查内存再查库 / 先删后插 / 三步配对……）；
  2. **为什么这么写**（为什么是这个顺序、为什么用这种结构、另一种写法的代价是什么）；
  3. **目的是什么**（这段逻辑服务于哪个上层目标；出问题时它对应的症状是什么）。
  粒度：**不是逐行翻译代码**，而是「让下一任读者不必反推意图」；一段 5~20 行的逻辑配 2~5 行注释是合适的密度。
- 已有注释保留「官方对应」的钉法（见 DESIGN 的注释规范）。
- **写完注释要回头查一遍「过期注释」**：注释最大的危害不是少，而是**说了假话**——
  改实现时同一文件里的旧注释必须一起改（2026-09-30 的注释梳理里当场抓到 6 处：库里已有
  `messages` 表却写着「按设计没有」、`tool_calls` 写着「参数截断到 300 字符」（截断早已取消）、
  「尚无写入方」的两张表其实都接上了写入方、「六张事实表」实际是八张……全是改代码时漏改的注释）。

---

## 2. 踩坑记录（按主题）

### 2.1 协议同步

**✅ 用上游标签做权威 diff，别靠版本号推断**（2026-09-28）

我最初从「dshr 是 0.1.5-rc.2、官方现在 0.1.7-rc.2」推断出两处变更，**两处都错了**：

- 错判 1：以为 SDK wire 面新增了 `maxTokens` / `serverInfo`。实际
  `packages/sdk/protocol/src/types.ts` 在 0.1.5→0.1.7 之间**零改动**，这些字段早就有了。
- 错判 2：把 `deliverables/presented`、`subagent/catalog` 当成「本次官方新增」。
  实际它们属于**工作树里未提交的在制品**（用户正在做的 0.1.5 同步）——
  我读的是工作树，把在制品算进了基线。

**做法**：官方每个发布版都有标签（`dsh-v0.1.5-rc.2`、`dsh-v0.1.7-rc.2`、`dsh-v0.2.0-rc.1`），
直接 `git diff <tag-A> <tag-B> -- <协议真源文件>`，一次拿到全部字段级变更。
配合 `scripts/compare-session-events.mjs` 做**事件名集合**的机器化对账。
**56×59 手算必错，不要手工比对。**

**✅ 同步一个事件类型要改三处**（漏一处要么解析不到、要么编译失败）

枚举变体 + `session_event/fallback.rs` 的手写 `Deserialize` 分发 + `session_event/meta.rs`
的 `as_str/time/seq/turn_step` 四方法。第三处靠**穷尽匹配**在编译期抓出来——这是编译器帮忙的典型场景。

**✅ merge-extensible 枚举要加 `#[serde(other)]` 兜底**

`TurnEndReason`、`MessageRole` 都是官方可扩展的联合类型。不兜底的话，官方加一个 kind
就整体解析失败（不是我加的那个事件，而是**所有**带该枚举的事件）。加 `Other` 变体后退化为
「这个字段显示为 other」，其余字段无损。

**✅ `known()` 兜底的价值**（v3 起）

已知类型的 data 解析失败也降级 `Unknown`（lossless），不整体报错。这是
`reason: 'series'` 事件的通用解法——官方在消息序列边界会发这个值，严格枚举会让**整个事件**
解析失败。有回归测试守着。

**✅ 0.2 对协议层零改动**（2026-09-28）

官方 `0.2.0-rc.1`（261 提交 / 1109 文件 / 净删 5.7 万行）看起来很吓人，但经标签 diff 核实：
`packages/sdk/protocol`、`core/session`（types + known-event-types）、`llm/{types,message}`、
`subagent`、`attachment`、`sdk/client` **全部无改动**。0.2 的重心是 client UI（`ui-chat` 38 文件、
`ui-primitives` 24、`ui-settings-account` 22…）与 Electron 桌面端。

**结论**：换 runtime 版本对本项目**没有功能收益**，反而引入 261 个提交的行为变量。
npm 上 `0.2.0-rc.1` 挂在 `next` 标签，`latest` 仍是 `0.1.7-rc.2`——**暂不换**。

**✅ `MessageSource` 漏了两个官方 0.1.7-rc.2 就有的 kind → 整条消息静默降级**（2026-09-29）

**怎么发现的**：重建测试时加了一条「**真实录制帧**形状对账」（`dsh-sdk-protocol/tests/frame_shape.rs`，
样本取自 `data/wire-logs/*.jsonl`），它立刻报：真实会话的 `system/message` 解析出来是 `unknown`。
顺着查：`source.kind = "system-prompt"` 不在 `MessageSource` 里；再全量扫 44 个日志文件，
`{kind:'runtime-context', form:'snapshot', sections:[…]}` 同样没建模——**两类消息全量降级**：

| kind | 出现次数 | 影响 |
|---|---|---|
| `system-prompt` | 5/5 条 `system/message` | 整条事件 lossless 但结构化视图丢失 |
| `runtime-context` | 5/5 条注入类 `user/message` | 同上（fold 本来不渲染它，所以界面上看不出来） |

**为什么会漏**：文件头当时写的是「官方基座 = user/plugin/model/tool」——那是 **0.1.2 时代**的形状。
0.1.7-rc.2 起官方把 kind 分散到各包的 `declare module '@deepseek-ai/dsh-llm'` 注册，
**没有**统一的基座清单可抄，于是「读一遍官方 message.ts」这种核对方式失效了。
**教训**：source kind 的权威判据是**真实帧**（或逐个包 grep `declare module`），不是某一处汇总文件。

**修法与护栏**：补 `SystemPrompt {}` 与 `RuntimeContext { form?, sections? }`（含
`MessageSourceForm::Snapshot` / `ContextSnapshotSection`）；护栏是新的
`real_message_sources_are_modelled`——它扫真实帧里出现过的 kind，**出现新 kind 即失败**
（逼你先去官方找形状，而不是靠猜）。

**✅ 顺着这条线把「缺口 + 静默」一起收干净了**（同日，方案经用户批准）：

1. **取证**：扫已安装 runtime（0.1.7-rc.2，291 个 `@deepseek-ai` 包）找出**真实可达但未建模**的
   kind——只有 5 个，且形状都能从代码里逐条挖出来（不是猜）：
   `plan-mode` / `model-selection`（都是 `{kind, form:'notice', summary}`）、
   `user-approval` / `ptc-mode`（`{kind}`）、`compact-checkpoint`（`{kind, compactionId, sourceCommandId?}`）。
   判据的优先级与失效史写在 §4.7——**别再读 `message.ts` 的基座 map，它 0.1.7-rc.2 起就不存在了**。
2. **补建模**：5 个变体全部补齐（共 20 种）。`ContextFormed` 系的字段按**可选**处理——
   官方注释写明「缺省或未知值即默认形态」，一处省略不该吃掉整条消息。
3. **让降级可见**（根治「靠运气发现」）：`SessionEvent::Unknown` 新增 `degraded: bool`，
   区分「**已知类型解析失败**」（= 漂移，要修）与「类型本身就未知」（= 插件自注册，预期内）。
   `known()` 那个分支只可能被已知类型的 arm 走到，所以改动只有两行、零维护点。
   engine 对前者写一条 app 轨迹 `event.degraded`（含 eventType + seq + sessionId）。
4. **新增 `scripts/scan-message-sources.mjs`**：扫「已安装 runtime 的代码字面量 + typert 里的
   合并声明 + wire-log 观测」三处，输出形状清单、逐字段必填/可选比对、可达性判断，
   并能一条命令刷新协议 crate 的真实帧 fixture。**它是发现机制**（测试只能守已知样本）。

**踩到的坑（写脚本时，三处都是「看起来对但其实错」）**：
① 括号平衡从 `{` **之后**开始数 → 深度永远配不平，块退化成 `source: {`，kind 全丢；
② 合并声明取到「第一个 `}`」就截断 → 那张表正好在第一个条目 `user: { kind: 'user' }` 结束，整表为空；
③ `ContextFormed` 的 `form?` 与 `source: {...}` 上别的域（`session/title` 的标题来源
`fallback`/`provider`、命令结果 `error`/`success`）混进来 → 必须按**map 名**区分 source 域，
并给这四个非消息来源留一张写死的豁免表（写死是为了让「新出现的未知 kind」仍然会报警）。

**💡 `serverInfo.version` 是硬编码的 `'0.0.1'`**

`packages/sdk/server/src/server.ts:170` 返回 `{ name: 'deepseek-harness-sdk-runtime', version: '0.0.1' }`，
与包版本无关。所以**不能**拿它做运行时版本校验（会永远"匹配"）。
dshr 的 `runtime::ensure` 必须读已安装包的 `package.json`（现状已如此，此处留档避免误用）。

### 2.2 Windows 沙箱（DSH 自身）

**✅ 工作区根缺「用户显式 ACE」会让 ACL 沙箱 fail-closed**（2026-09-28 实测定位）

**现象**：所有 `pwsh` 命令失败，报
`SetNamedSecurityInfoW failed (Win32 5): grantWrite(D:\dsh)`。

**根因链**（读 `dsh-desktop/resources/app.asar` 里的 `sandbox-windows-acl` 实现 + 实测对照）：

1. `grantWrite` 用**一次** `SetNamedSecurityInfoW(path, …, 20, …)` 同时写 DACL 和完整性标签（SACL），
   `20 = 0x14 = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION`。
2. 它的幂等跳过要求「精确 capability ACE + 精确 world 拒绝 + 精确 Low 标签」三者齐备；
   而 `hasExactLabel` 要求标签 ACE 的 `inheritance == 3`，`buildLowLabelAcl` 用
   `AddMandatoryAce(..., AceFlags=0, ...)` 建 ACE —— **两者永不相等**，所以每次都走 apply 路径。
3. `D:\dsh` 上写标签返回 **ACCESS_DENIED(5)**，沙箱 fail-closed。

**决定性对照实验**（区分「ACL 内容」与「权限」）：

| 对象 | `SetNamedSecurityInfoW(LABEL)` |
|---|---|
| `D:\`、`D:\dsh`、D: 上新建目录 | `rc = 5` 全失败 |
| D: 上同一目录**加一条用户 `(A;OICI;FA)`** 后 | `rc = 0` 成功 |
| C: 上任意目录（含把 `D:\dsh` 的 DACL 原样克隆过去） | `rc = 0` 成功 |

→ 不是 DACL 内容问题，是**D: 卷根 DACL 里没有当前用户 SID 的 ACE**（只有 `AU:(M)` 与 `IO` 仅继承项）；
C: 的用户目录树自带 `(A;OICIID;FA;;;<用户SID>)`，所以一直正常。属主虽是自己，但没有显式 ACE 时
**属主隐含 WRITE_DAC 未生效**。

**结局**：这个状态后来**自愈**了（`D:\dsh` 拿到了 Low 完整性标签，沙箱能跳过 apply）。
我写的修复脚本已删除。

**教训**：这类"沙箱拒绝"要**先分辨是内容问题还是权限问题**——用「把已知内容克隆到另一个可写位置」
做对照，能一刀切开。另外 `D:\dsh`（工作区根）与 `D:\dsh\dshr`（子目录）的 ACE 可能不同，
读 ACL 要逐个路径看 `icacls` / `Get-Acl` 而不是抽查。

**✅ PowerShell 5.1 按 ANSI 读无 BOM 的 UTF-8 脚本 → 中文乱码破坏引号**

给用户写的 `.ps1` 脚本含中文时，**必须存成 UTF-8 with BOM**，否则 Windows PowerShell 5.1
按 ANSI 解码，中文变成乱码字节，引号配对错乱 → 语法错误。
另外：`pwsh` 的 `-Command` 里嵌套引号很多时容易踩自己的脚，脚本落盘再执行更稳。

**✅ 长按 `>` 重定向会毁掉文件编码**

`git show HEAD:path > file`（PowerShell）会把输出**重编码成 UTF-16LE**，
文件大小翻倍、git 立刻识别为二进制（`Bin 34651 -> 62396 bytes`）。
用 Node 以字节流取：`execFileSync('git', ['show', ...])` 得到 Buffer 再 `writeFileSync`。

### 2.3 构建与依赖

**✅ `git pull` 不会删除未跟踪的构建残留 → tsdown 把空壳当活跃包**（2026-09-28）

**现象**：官方 `deepseek-harness` 构建失败：
`[MISSING_EXPORT] "SettingsProvider" is not exported by "../settings/src/index.ts"`。

**根因**：官方在两次 pull 之间的提交里删掉了 4 个包，但 git 不删未跟踪文件，
于是 `packages/<组>/<包>/` 下留下**只有 `lib/` 与 `node_modules/`、没有 `package.json` 与 `src/`** 的空壳：

```
packages/settings/settings-file
packages/client/ui-settings-unarchive-sessions
packages/experimental/agent-team-web-profile
packages/preset/agent-presets
```

`tsdown.config.ts` 用 `workspace: ['vendor/*', 'packages/*/*', 'apps/cli', ...]` **按 glob 自动发现**构建目标，
把这些空壳当成活跃包去 bundle 其中的**旧** `lib/types/index.js`，于是解析出官方已删除的符号。

`git status` 之所以干净，是因为 `lib/` 与 `node_modules/` 都在 `.gitignore` 里——**git 完全看不见它们**。

**修法**：官方自带 `pnpm run clean`（`scripts/clean.ts`），它的逻辑正是为此写的：
**没有 `package.json` 的包目录，若只剩 `node_modules`/`lib`/`.typecheck`/`*.tsbuildinfo` 就整体删除，
遇到未知文件则拒绝删除并列出**。我这次只精确删了那 4 个目录（确认过零未知文件），
没跑全量 `clean` 是为了不误删 `packages/typert/generator/lib`（`tsdown.config.ts` 引导要 import 它）。

**排查手法**：扫 `packages/*/*/` 找「有 `lib/` 无 `src/`」或「有 `lib/` 无 `package.json`」的目录。

**✅ `cargo` 的 stderr 会被 PowerShell 误判为命令失败**

`cargo build/test` 把进度写到 stderr，PowerShell 把它当 `NativeCommandError`，
**裸看退出码会误判**（测试全绿但 `$LASTEXITCODE = 1`）。
判断成功与否要看输出里的 `test result:` / `Finished` 行，或用 `--message-format` 等结构化输出。

### 2.4 Rust 拆分与重构

**✅ 拆 God file 的正确姿势：机械切片 + 编译器驱动修正**

`fold.rs` 1193 行 → 4 个文件、`store.rs` 844 行 → 6 个文件，两次都是**零逻辑改动**，
11 个 / 全部测试用例数量与结果完全不变。做法：写一次性切分脚本（留档在 `scripts/split-*.mjs`），
按行号区间切片并**在脚本里断言切点**（行数、起始行不符就中止，不产出半成品），
然后靠编译期错误（E0603 可见性 / E0433 导入 / E0252 重复导入 / E0585 悬空 doc）逐个修。

**踩到的四个坑**（都是编译器抓出来的）：

1. **子模块里 `impl` 的私有方法父模块看不见** → 拆出去的方法要 `pub(super)`。
   （第一反应会以为 Rust 的"私有"像 Java 那样对包内可见，其实只对**自身及后代模块**可见。）
2. **`mod.rs` 位置的 `pub(super)` 只对 crate 根可见** → 跨子模块共享的小工具要用 `pub(crate)`；
   或者让中间模块 `pub(super) use` 转发（如 `render.rs` 转发 `text_of` 给 `event.rs`）。
3. **多行 `use` 必须整条删除**：只删首行会留下孤儿标识符与 `};`，产生语法错误。
4. **段切点必须把"下一段的 doc 注释"排除在上一段之外**，否则 doc 悬空（E0585
   "expected item after doc comment"）。`store.rs` 里 `SessionSummary` 的 doc 在 306 行的 struct
   之前两行，切到 304 就正好把 doc 留在了上一段尾部。

**另外两个**：

5. **`#[cfg(test)] mod tests { ... }` 内嵌测试改独立文件时**：要去掉 `mod tests {` 包裹与它对应的
   收尾 `}`，**并把整体缩进回退一级**；还要把原 `use super::*` 提供的导入**显式补回**
   （漏 `SessionStatus` / `MsgKind` 之类会在测试编译时才暴露）。
6. **批量正则改名会误伤局部变量**：我把 `Engine` → `Runtime` 时，`&mut engine` /
   `engine: &mut Engine` 也被一起改成了 `&mut Runtime`，触发 E0423 / E0573。
   正确做法：按上下文逐一改，或先改类型再单独处理变量名。

**✅ `mod.rs` 全面弃用（用户 2026-09-28 要求）**

统一用 Rust 2018+ 风格：`x.rs` + `x/` 同名并列。涉及 `fold`、`raw`、`ui/files`、`ui/widgets`。
转换时要**保留原模块文档**（`mod.rs` 的 doc 注释搬到 `x.rs`），
`widgets/mod.rs` 那种纯声明文件尤其容易在搬运时丢中文文档。

### 2.5 UI / iced

（详细实测数据见 DESIGN 的渲染后端与 DPI 决策；此处只留方法论）

**✅ 定位内存问题要「逐项实测 + 对照实验」，不要凭直觉归因**

用户报「打开一个文件就 167 MB」，直觉会怪编辑器。逐项实测后定位：
**167 MB 是 iced 0.14 默认 features 里 `wgpu` 的基线**（空 UI 即 163 MB private），
与编辑器无关；同版本换 `tiny-skia` 后空 UI 为 29 MB。而后来观测到的 163 MB 又有一部分
是 **DPI bug 导致的 2 倍渲染**——修掉 DPI 后 wgpu 为 97 MB。

**✅ DPI 不一致时 winit 自相矛盾**：创建窗口按 1:1（请求 1000x700 得 1000x700 物理像素），
**却向 iced 报告缩放因子 2.0**。iced 侧可见区域恒为画布的 `1/winit缩放`，
所以 **winit 缩放 > 1 时任何 `scale_factor` 都无法自洽**（实测 1.0 / 0.5 都验证过）。
唯一可行解是进程级 DPI-unaware（`SetProcessDpiAwarenessContext`），代价是位图拉伸发糊。

**💡 未解**：在 `aware` 模式下按缩放因子把根视图缩排到「画布 ÷ 缩放」的子区域内，
可同时拿到清晰 + 完整 + 不被拉伸。需要一轮实现与验证。

### 2.6 「静默失败」类陷阱（2026-09-29，M3 期间集中踩到）

这个项目里最贵的 bug 有一类共同特征：**不报错、不崩溃，只是「什么都没发生」**。
调试它们全靠「先证明某一层确实被调到了」，不能靠读代码推断。

**① 协议解析失败会静默降级成 `Unknown`。**
`fallback::known()` 在 data 反序列化失败时不抛错，而是**保留原始 type 字符串**降级为
`Unknown` 变体——这是刻意的容错（官方加字段不该让 dshr 挂）。
代价是：我在测试里手写 `assistant/message` 帧时漏了必填字段，表现是
「事件没反应、测试等超时」，而**没有任何错误信息**。
第一次定位时我还误判了两次：先怀疑路由没生效，再怀疑广播没送达，
实际是 `source.kind = "model"` 需要 `provider` + `model` 两个必填字段。
**对策**：加一条「帧形状自检」测试，直接断言 `parse()` 出来的 `event_type()`
等于预期字符串——把形状问题钉在它自己的失败信息里，而不是让它伪装成上层超时。

**② `tokio::select!` 在多分支同时就绪时是随机选择，会饿死某一路。**
engine 的命令通道只要有排队项就一直就绪（UI 点几下就会有），
于是 runtime 事件分支有一半概率被跳过；命令持续到达时事件被**完全饿死**。
现象同样是「什么都没发生」。**对策**：进 select 之前先用 `try_recv()` 把已排队的命令
排空，让「命令优先」成为确定顺序，而不是指望随机调度。

**③ 借用冲突的正解往往是「让数据离开结构体」，不是加 `clone`。**
engine 要同时等「N 个 runtime 的事件」与「一条命令」，而 future 各自要借 `&mut` 某个
runtime——即借走 `self`。`select!` 的 handler 又需要 `&mut self`。试过几种写法都撞
E0499，最后用 `std::mem::take(&mut self.runtimes)` 把表临时搬出结构体：
future 借的是本地变量，与本结构无关，循环结束再放回。
**这条比「每 runtime 起一个转发任务」更省事**——后者要额外解决
「进程退出后任务何时收敛」。

**④ 测试助手的副作用会吃掉后续断言的状态。**
把「stderr 落库」与「退出上报」写在一条测试里时，前者的推进循环已经让 engine 判为
「未启动」并 `teardown` 移除了该 runtime，导致后面标记退出时注册表已空。
**对策**：一条测试只验一件事。需要多条时，各用独立的 engine 实例，
而不是靠「固定推进 N 步」去猜前一步的副作用。

**⑤ 表建了不等于有人写。**
`runtimes` 与 `runtime_logs` 两张表在 DDL 里存在了很久，但**从来没有写入方**——
后果是 `sessions.runtime_id` 恒为 NULL（多 runtime 时数据无法归位）、
stderr 无处可查。这类"空表"从表面完全看不出来，只有查询才会暴露。
补充发现：**外键约束会进一步惩罚它**——`runtime_logs.runtime_id` 引用 `runtimes(id)`，
所以测试注入路径忘了写 `runtimes` 行时，落库直接 `FOREIGN KEY constraint failed`。

**⑥ 写脚本时，脚本文本本身也会咬人。**
自动化删除测试的脚本里，块注释中写了 `*/tests.rs` 与 `/* */` ——
提前闭合了注释，Node 直接语法错误。**对策**：在注释里避免出现 `*/` 与 `/*` 序列。

**⑧ 子表外键 + 「落库失败只打一行 stderr」= 又一次「表是空的」**（2026-09-29）

写 `requests` 表写入方时，测试报「一次 prompt 应落一行请求事实：[]」——表是空的。
表面看和「表根本没有写入方」（§2.6⑤ 那个老问题）一模一样，实际是**外键**：

`requests.session_id` 引用 `sessions(id)`，而**第一次 prompt 时该会话还没落过任何快照**
（快照要等事件回来才有内容）→ `INSERT` 撞 `FOREIGN KEY constraint failed`；
engine 的落库错误策略是「打一行 stderr 就继续」，所以失败被吞掉，只有查询才暴露。
对策：`Store::ensure_session`（幂等补壳行，`INSERT OR IGNORE`）。

**为什么这次很快就定位**：因为**测试写得足够具体**（断言「表里应有 1 行」而不是「没报错」）——
这与 §2.6 开篇那句「这类 bug 只能靠『对应该发生的事本身断言』抓出来」是同一件事。

**⑦ 空集合上的「立刻就绪」会让总线空转烧一个核**（2026-09-29，重建测试时抓到的真 bug）

`FuturesUnordered::next()` 在**空集合**上立刻返回 `None`。engine 的事件循环是
`select!{ 命令 ∨ 任一 runtime 的事实 }`，注册表为空时第二个分支**立即就绪**，
于是 `Engine::next()` 一个 `await` 都不产生就返回空批；而总线（`dshr-ui/src/bridge.rs`）
是 `loop { engine.next().await }` → **纯空转**，在 iced 的执行器线程上烧掉一个核，
**界面上没有任何异常**（不报错、不崩溃、只是「什么都没发生」）。
触发条件很常见：程序刚启动（还没 StartRuntime）、用户停掉最后一个 runtime。

**修法**：注册表为空时特判，只 `await` 命令通道（`registry.rs` 的 `next`）。
**怎么发现的**：写测试时顺手加了一条「无 runtime 时应阻塞」的断言——它当场失败，
输出是 `Ok(Some([]))`（说明根本没等）。**这类 bug 只能靠「对『应该等待』这件事本身断言」
抓出来**，读代码很容易读成「没东西就下一轮，没问题」。

### 2.7 runtime 下载稳定性（2026-09-29 实测）

**背景**：dshr 不打包 dsh，启动时自己下载（`runtime::ensure` → `pnpm install`）。
用户问「有没有什么方式能够稳定下载」。

**量化诊断**（查 npm 发布信息得到）：
`@deepseek-ai/dsh` 本体只有 **69 KB / 20 个文件**，但它声明的 **81 个直接依赖**
会解析成 **588 个包**，其中含 `sharp` / `sherpa-onnx` / `koffi` / `libreoffice-kit`
的**全平台**二进制。**`optionalDependencies` 为 0**——这些"用不到的平台产物"是硬依赖，
无法裁剪。

**失败实测**：官方 registry 两次都在 **356/587** 处因 socket 超时（`error (23)`）中断。
`error (23)` 对应 npm 的 `ERR_SOCKET_TIMEOUT`；npm 默认 `fetch-retries=2`
（日志里的"2 retries left"就是它），对间歇性超时太弱。

**解法与验证**：换 `registry.npmmirror.com` 后 **`Done in 56.6s`**，
`reused 561` + 新下 26 —— 镜像已镜像 `@deepseek-ai` scope（实测 dist-tags 与官方一致）。

**落地的三项加固**：
1. **重试 + 指数退避**（同 registry 3 次，5/10/20s）。关键认识：pnpm 的 store 是
   **content-addressable** 的，**重试即续传**——`reused 561` 就是证据，所以重试不是
   "把失败往后推"，而是真的在推进度。
2. **写 `dsh/.npmrc`**：`fetch-retries=10` / `fetch-timeout=600000` /
   `network-concurrency=4` / `prefer-offline=true`。为什么用文件而不是命令行旗标：
   `pnpm install --help` 里**没有** `--fetch-retries`；`--config.fetch-retries=10`
   能被接受，但 `--config.network-concurrency=4` 会报「期望数字得到字符串」——
   写文件最稳。
3. **registry 回退链**：官方 → npmmirror；可用 `DSHR_NPM_REGISTRY` 环境变量或
   `config.json` 的 `npm-registry` **覆盖为只走指定源**（内网代理场景下若还去试官方
   只会白等一轮超时，所以是"覆盖"而不是"插到最前"）。

**用户明确否掉的方案**：不在仓库里打包一个预热归档——「还是通过 pnpm 下载构建」。

**另一个值得反馈给官方的点**：那 588 个包里相当一部分（`libreoffice-kit`、
`sherpa-onnx` 这类明显属于 UI/语音/文档能力的）在 `--profile sdk` 下大概率不会被
import。若官方把它们改成 `optionalDependencies` 或 peer，下载量能降一个量级。

**踩到的两个自己的坑**：
- 用 PowerShell `Set-Content -Encoding utf8` 写 `data/secrets.json` 会加 **BOM**，
  而 `serde_json::from_str` 遇 BOM 直接失败 → 表现为"配了 key 却回落 Fake"。
  这与 §2.2 记的是同一个坑的不同面孔（**PowerShell 5.1 的 utf8 带 BOM**）。
- 账本里**硬编码了 runtime id**（`rt-sample`）去查 `runtime_logs`，于是永远显示 0 行。
  「用错 id 查不到」与「数据没写进去」在表面上完全一样——这是 §2.6⑤「表建了没人写」
  的同一个形状，值得警惕。

---

## 3. 决策简史

> 记录**被推翻/演进**的决策以及原因。当前有效结论见 DESIGN.md。

| 时间 | 决策 | 后续 |
|---|---|---|
| 2026-09-01 | 定位 = Rust SDK 主线；原生重写官方 UI 不划算 | 保留 |
| 2026-09-01 | **结构化范围 = 够用即可**：只结构化官方当时的事件，新事件一律 `Unknown` lossless | **2026-09-02 用户决定做协议大同步后作废**，改为全部结构化 + `known()` 兜底 |
| 2026-09-02 | 协议大同步到 0.1.2-alpha.5（51 种全集结构化） | 后续同步到 0.1.5-rc.2（56 种）、0.1.7-rc.2（59 种） |
| 2026-09-17 | 渲染后端默认改 **tiny-skia**（省内存：29 MB vs 163 MB） | **同日被用户实测推翻**：滚轮卡死。回到 wgpu 默认（97 MB），tiny-skia 降为创建失败时的兜底 |
| 2026-09-17 | 顶栏窗口图标用 `canvas` 自绘（视觉统一） | **同日推翻**：tiny-skia 后端下画布拿到 0 尺寸、完全不出图。改回文字字形（U+2212 / U+25A1 / U+2715） |
| 2026-09 | s3 曾把常驻 worker（Machine/RealBridge）放在 **dshr-ui** 侧，UI 直接 import `dsh-sdk-client` | **推翻**：旁路 state 层。整体下沉为 `dshr-state::engine`，UI 只经 bridge 搬运命令/事件 |
| 2026-09-28 | `dshr-state` 的分层：**raw（与 SDK 沟通）/ engine（核心数据处理）/ fold（UI 投影）** | 本轮定稿。旧的单一 `engine` 承担全部职责，被认定为第二个 God file |
| 2026-09-28 | `Engine` 结构体改名 `raw::Runtime`；`EngineCmd`/`EngineEvent` **名字保留**（M3 迁到 engine 层，加 runtime/session 标） | 为避免 UI 二次改动而做的过渡安排 |
| 2026-09-28 | `session.rs` **删除** | **推迟到 M4**：它用的 `HarnessClient::run()`（receipt-to-idle）当前 engine 接口没有，直接删会丢功能 |
| 2026-09-28 | runtime 版本钉 `0.1.7-rc.2` | 官方已发 `0.2.0-rc.1`，但协议零改动 → **暂不换** |
| 2026-09-29 | **`dshr-state` 不再提供可执行文件**（`main.rs` + `session.rs` 删除）。项目入口只有 `dshr-ui/src/main.rs` | 用户明确要求「state 里不要用 main，main 应该放在 ui」。用户提出的替代验证路径是「在每个 crate 下单独一个 test 文件夹」 |
| 2026-09-29 | **全部测试清空**（67 个 → 0），改为按「各 crate 一个 `tests/` 目录」重建 | 用户明确要求。理由与重建计划见 DESIGN.md §12.4。**执行顺序上我提了反对意见**：M3 是最大的结构性改动，先删测试等于在无安全网的路径上重构；用户采纳了「先做 M3（带测试）、完成后一次性删」的顺序 |
| 2026-09-29 | `EngineCmd::ResetSession` **不要求调用方给新 session id**（改由 engine 生成，经 `EngineEvent::SessionReset` 回报） | 我最初的设计让 UI 预测新 id——这是错的：id 唯一性规则（真实 dsh 按 id 落盘日志，重复会撞 `session already has a persisted log on disk`）属于 state 层，两侧各有一套生成逻辑必然漂移 |
| 2026-09-29 | wire-log **路径生成权收归 engine**（原在 raw 自己拼） | 因为 engine 必须把**同一个路径**同时交给 `Recorder`（写 app 轨迹）与 spawn 配置（SDK 写 dsh 线级记录）；两边各自拼路径会散成两个文件，时间线无法对齐 |
| 2026-09-29 | `Store::persist_snapshot` 签名 `&mut self` → **`&self`** | `rusqlite` 的 `unchecked_transaction` 只需 `&Connection`，所以落库本就该是只读借用。改掉后 engine 不必为每次落库做「先借 store 再借会话」的借用体操 |
| 2026-09-29 | **补 `MessageSource::{SystemPrompt, RuntimeContext}`**（+ `MessageSourceForm::Snapshot`/`Other`） | 实测代价驱动，见 §2.1。**只补真实帧里出现过的**，其余缺口留给 §4.7 拍板 |
| 2026-09-29 | `MessageSourceForm` 加 `#[serde(other)] Other` | 官方 ContextForm 的注释写明「缺省或未知值即默认形态」——未知 form 不该让整条消息降级 |
| 2026-09-29 | 协议侧「事件全集」对账**下沉一条到 `cargo test`**（对锁定快照） | 脚本对官方真源、测试对快照；只 clone dshr 的机器也能挡住枚举被改坏 |
| 2026-09-29 | `Engine::next()` 在空注册表时特判（只等命令） | 见 §2.6⑦：不特判就是总线空转烧一个核的静默 bug |
| 2026-09-29 | 补 `plan-mode` / `model-selection` / `user-approval` / `ptc-mode` / `compact-checkpoint` 五个 kind | 由 `scripts/scan-message-sources.mjs` 按「依赖闭包内 + 代码里真会写出去」筛出，形状逐条取证（见 §4.7） |
| 2026-09-29 | 给 `Unknown` 加 `degraded` 标记，engine 记 `event.degraded` | 把「已知类型解析失败」从「类型未知」里分出来——前者要修，后者是预期；这是「静默失败」那类 bug 的对策 |
| 2026-09-29 | 消息来源 kind 的判据改为「真实帧 + 合并声明 + 迁移表」三条，**弃用** `message.ts` 基座 map | 那份 map 在 0.1.7-rc.2 已不存在（§2.1 的错读就是这么来的） |
| 2026-09-29 | `request/header` + `request/context` 折进快照 `last_request`（此前都折成 `{}`） | 用户问「为什么等待时什么都看不到」的根因不是缺定时器：折完无变化 → 脏检测不发事件（§4.8）。**数据侧先行，UI 显示留 M5**（用户要求 UI 之前先做核心逻辑） |
| 2026-09-29 | 落盘数据用 **CSV 导出**打通「看得见」（`export.rs` + `Store::export_rows`） | 监控页未做，但数据已经有了（§4.8）；导出函数将来直接接监控页，所以不是临时脚本 |
| 2026-09-29 | 会话历史走「**按 sessionId 分组回放 wire-log**」而不是查库 | 库里按设计没有 messages 表（§8.2）；实测 wire-log 有 42 个会话而库里只有 3 行（§4.5） |
| 2026-09-29 | `MessageSourceForm` 等 `ContextFormed` 字段按可选比对 | 官方注释写明「缺省或未知值即默认形态」，一处省略不该吃掉整条消息 |
| 2026-09-29 | **新增 `messages` 表**：逐条对话落库（含注入与尝试），**原文不截断** | 用户要求「细致到每个会话内的每轮对话，除逐 chunk 之外都记」；截断下移到渲染层（工具 arguments 全文进库） |
| 2026-09-29 | fold 不再丢弃**程序化注入**与**未提交尝试**：折成 `Injected`/`Attempt` 行 | 记录与显示分开——**显示策略归 UI**（`model.rs` 过滤），库与导出是全量的 |
| 2026-09-29 | **复原**：`Store::load_snapshot` / `load_session_ids`，`sessions.meta_json` 存会话级聚合 | 用户要求「关掉再打开还能复原记录」；契约是**逐字段相等**（`restore_roundtrip`） |
| 2026-09-29 | token 六桶列**可空**（NULL ≠ 0） | 官方 adapter 缺报的桶是 unknown；写成 0 会让复原从「未知」变「0」（往返测试当场抓到） |
| 2026-09-29 | schema 引入 `PRAGMA user_version` + 加列迁移 | 用户的历史会话在库里，升级不能靠删库；`MIGRATIONS_V2` 先探测后 ALTER（幂等） |
| 2026-09-29 | `requests` 表接入第二个写入方（engine 记每次 prompt 的耗时/成败/原因） | 「发送失败也要记」用户明确要求；`Store::ensure_session` 解决父行外键（§2.6⑧） |
| 2026-09-29 | M6 只做「拉」这一半（`ReadSnapshot`/`SessionLoaded`/`ListSessions`），**推送保持不动** | 「事件只发轻通知」必须与消费侧同批改：只改执行者会让界面静止（而静默是本项目最贵的失败形态）。切换清单见 §4.9 |
| 2026-09-29 | 拉取结果用**独立事件** `SessionLoaded` 而不是复用 `Snapshot` | 语义不同（我问你要 vs 变了顺手给你）；且 `runtime: Option<RuntimeId>` 要能表达「库里复原的历史会话不属于任何运行中 runtime」——硬塞一个 id 会让 UI 建出假节点 |
| 2026-09-29 | 拉取**只读**（不脏检测、不落库）、历史态**不缓存** | 拉不该有副作用；读几百行是毫秒级，缓存反而引入失效问题（会话可能又被跑起来） |
| 2026-09-30 | 注释规范补「函数内下沉到块级」：方法 / 为什么 / 目的 | 用户明确要求。理由：这批代码（S4/M6）的**意图**只在提交信息与 AI-LOG 里，源码里读不出来——而接手的人先读源码 |
| 2026-09-30 | 同一次梳理里**修 6 处过期注释**、记 2 处「代码与注释不一致」（§4.10） | 注释最大的危害不是少而是**说了假话**；这两处顺带发现的问题只记录、不动行为（用户本轮只要注释） |
| 2026-09-30 | 文件页/工作区这块**先写文档**（DESIGN §7.4 + §12.3 的 4 条待办），代码不动 | 用户选了「先写文档」。核对结论：`workspace.rs` 的问题不是行数少，而是**能力缺**（无新建/重命名/删除、保存非原子），这些都改行为或要引依赖 → 值得单独拍板（§4.11 列了 6 条要决策的点） |

---

## 4. 未落地的设想

### 4.1 state 分层改造（M2 / M3 已完成，M4–M6 待做）

完整方案见 DESIGN.md 的「state 分层」章与 §12.2。进度：

- **M2 ✅ 已完成**：`SessionDriver` trait（`raw/driver.rs`）+ `ClientDriver` 适配
  （`raw/client_driver.rs`），`Runtime` 改持 `Box<dyn SessionDriver>`。
  两处非显然的实现决定：① trait 用**手写 `Pin<Box<dyn Future>>`**（`BoxFuture` 别名）
  而不是 `async fn`——后者在 trait 中不满足 dyn 兼容性，而「可替换驱动」正是这个 trait
  存在的理由；也没引入 `async-trait`（不在依赖树、需联网），因为只有 3 个 async 方法。
  ② `HarnessClient::shutdown` 消费 `self` 而 trait 要 `&mut self`，
  故 `ClientDriver` 用 `Option<HarnessClient>` 消化这个所有权差异并把收尾做成幂等。
- **M3 ✅ 已完成**：`engine.rs`（类型）+ `engine/registry.rs`（注册表 + 事件循环）+
  `engine/session.rs`（每会话态）。`raw` 收窄为**纯进程句柄**，只产出未折叠事实
  （`RuntimeEvent`）。`EngineCmd`/`EngineEvent` 迁到 engine 并带**双层标识**
  （`RuntimeId` + `SessionId`，都用 newtype 防写反）。
  多路复用用 `futures::future::select_all` 语义（`FuturesUnordered`）聚合所有 runtime 的
  事件，而不是每 runtime 起一个转发任务。
- **M3 附带完成**：stderr 与退出**落盘接通**（`runtime_logs` / `runtimes` 表首次有写入方）；
  `record::Recorder` 重新接入（此前它的唯一消费者是被删掉的 `main.rs`，一直是死代码）。
- **M4**（待做）：raw 改发 wire 级事件批（带标）。**部分已提前完成**：
  `session.rs` 已删除、`raw.rs` 已是纯进程句柄、WireLog 路径生成已归 engine。
- **M5**（待做）：UI 侧仍是单 runtime 视图（`RT_ID` 固定）。
- **M6**（待做）：快照读取改「拉 + 变更通知」（B 方案，已定）。

### 4.2 多 runtime / 多会话（用户明确要的能力）

**现状**：协议层与 client 层**天然支持**（每个 `HarnessClient` = 一个 runtime 子进程；
`session/prompt` 的 `sessionId` 官方注释是「unknown id lazily creates the agent+session pair」，
即一个 runtime 内可并存多个会话）。**但 state 层不支持**：`Runtime` 的字段全是单数
（`client` / `events` / `session_id` / `bridge`），`EngineCmd` 连一个 id 都没有。

**关键错配**（讨论时定位）：`EngineCmd` 是**运行时级**（无 session 标识），
`EngineEvent::Snapshot` 是**会话级**（带 session_id），中间没有一层能把两者对上。
`last_sent: Option<SessionSnapshot>` 的单会话去重就是这个假设的产物。

**待做**：`runtime_id → session_id → 会话态` 的两级结构。`Bridge::feed` 现在是
**过滤阶段直接丢弃**非当前会话的通知，多会话化时要改成路由。

### 4.3 数据落盘完整性（用户 2026-09-28 明确要求）

原则：**尽可能多地暴露并落盘数据**。已定的取舍：

- **逐 chunk 不落盘**（空间代价不可接受——用户历史上想过逐 chunk，评估后放弃），
  改为**按会话记录完整 chunk 序列**：一条 `assistant/message.stream` 保留该次消息的全部
  `AssistantStreamRecord`，而不是每个 chunk 一行数据库记录。
- **`stderr` 必须落盘**：`store.rs` 已有 `runtime_logs` 表，但当前**零写入**——
  因为 `HarnessClient::take_stderr()` **无人调用**。等于建了表没数据。
- **进程退出必须落盘**（exit code + stderr 尾部）为审计事实，不能只在
  `Error::TransportClosed` 里间接可见。

### 4.4 其它设想

- **💡 portable node 自动安装**：当前 node 缺失时报清晰错误；`runtime.rs` 注释提到自动安装是下一步。
- **💡 监控页（s4）**：DESIGN 的数据罗盘章有完整的统计域设计，页面未做。
- **💡 `store.rs` 的表集扩展点**：`requests` / `runtimes` / `runtime_logs` 三张表已建但无写入方
  （分别等 s1 请求层折叠、多 runtime 管理、stderr 通道）。
- **💡 README 双语 + 发布准备**：发布等 SDK 全做完 + 测试完（用户明确暂缓）。

### 4.5 会话历史的记录与「跨会话」（用户 2026-09-29 提出，**待讨论**）

**用户的诉求**：会话历史要能记下来、特别是**跨会话**可查（现在 UI 只看得见当前会话）。

**现状盘点**（三处存放，职责不同，别混）：

| 源 | 内容 | 覆盖 | 读取方式 |
|---|---|---|---|
| `dshr/data/dsh-home/sessions/<ws>/<id>/session.jsonl.zstd` | runtime 自己的**会话日志**（多独立 zstd frame，首行是 SessionHeader） | 每个会话一份，官方格式 | 需解 zstd + 按官方 schema 读 |
| `dshr/data/wire-logs/*.jsonl` | 宿主侧**全程记录**（cat=dsh 线级 + cat=app 轨迹） | **全集**：本次实测回放出 **42 个会话**，而库里只有 3 行 session | `Folder::push_wire_line` 回放（已落地，见 §4.8 的导出） |
| `dshr/data/dshr.db` | 加工**事实表**（sessions/turns/tool_calls/file_ops） | 只有**当前**跑过的会话 | SQL 聚合 |

**关键观察（本次实测）**：库里 3 个会话 vs wire-log 里 42 个——**wire-log 才是历史的全集**，
库只装「引擎跑过并落盘过的」。原因是设计如此（§8.2 不建 events 全量表），但代价是
「重启后列历史会话」目前没有索引可查（要扫全部 wire-log 才能知道有哪些会话）。

**✅ 本轮（2026-09-29）已落地的部分**：库里现在有**逐条对话**（`messages` 表：turn/step/source/
text/reasoning/error/token 六桶/流摘要/工具全文），并且 `Store::load_snapshot` 能**关掉再打开复原**
（契约是逐字段相等）。也就是说「历史记录」这件事从「只能回放 wire-log」升级成「库里就有」。
**仍未做**的是下面的**索引**（A/B）：库里现在只装「引擎跑过并落盘过的会话」，
「应用启动时列出所有历史会话」还需要索引或扫描。

**三个可选方向（代价递增）**：

| 方向 | 做法 | 代价 / 风险 |
|---|---|---|
| A 每次会话结束写一行 `sessions`（含起始时间/wire-log 文件名） | 最小改动，让「历史会话目录」有索引 | 库会随会话数线性增长（可接受）；要处理「同 id 重复落盘」（幂等已有） |
| B 启动时增量导入：扫 wire-log 把见过的 session 补进库（带书签，不重复扫） | 索引与全集一致，且能回答「哪个会话在哪个日志文件里」 | 启动成本；要定书签格式与损坏日志的容错 |
| C 直接读官方 session.jsonl.zstd | 与官方语义完全一致（含 compaction 后的 surface 重写） | 要解 zstd 多 frame + 跟官方 schema 漂移；与 wire-log 回放可能给出**不同**结果（surface 重写 vs 原始流） |

**我的倾向**：先 A（便宜、立刻让历史可列），B 留到监控页要「历史浏览」时做；
C 作为「与官方对齐」的独立议题——它能拿到 wire 上看不到的东西（surface 重写后的真实历史），
但要接受官方格式漂移的维护成本。**注意**：C 与 A/B 不是替代关系，是两个视角。

**已落地的第一步**：`dshr-state/src/export.rs` 的 `replay_sessions` 已经能做「跨会话重建历史」
（按 sessionId 分组回放，见 §4.8）——上面 A/B/C 都是在它之上补「索引」与「官方视角」。

### 4.6 多 runtime ≈ 多智能体？（用户 2026-09-29 提出，**只是记录**）

**用户的理解**：官方一个 runtime 对应我们这里的一个会话；想知道 dshr 能不能做多个 runtime，
类似多智能体。

**事实澄清（很重要，我核对过协议与官方包）**：

- **一个 runtime ≠ 一个会话**。官方 `HarnessClient`（= 一个 node 子进程 = 一个 dsh 实例）内可以
  并存多个会话：`session/prompt` 的官方注释写明「unknown id lazily creates the agent+session pair」。
- 官方的「多智能体」是 **runtime 内的父子会话**（`subagent/*` 事件族 + `parent` 血缘，
  `dsh-agent` 的 subagent 驱动），不是多进程；dshr 的库里 `sessions.parent` 列已为此留位。
- dshr 的 **state 层已经支持多 runtime**（注册表 + 按 sessionId 路由 + 每会话态 + 路由隔离测试）；
  缺的是 **UI 侧**（`dshr-ui/src/app.rs` 的 `RT_ID = "rt-1"` 固定单视图）——这正是 M5。

**两条路的代价对比（记录备查）**：

| 路线 | 形态 | 代价 | 收益 |
|---|---|---|---|
| 多 runtime | N 个 node 进程，各自独立 dsh 与 DSH_HOME 视角 | 每 runtime 一个 node 进程（实测常驻 ≈100–200 MB 量级）；安装/启动 ×N | 强隔离（崩一个不影响其它）；不同 provider/model 并存 |
| 单 runtime 多会话 + subagent | 官方原生血缘树 | 共享一个进程的 agent 注册表；子代理的模型/权限受父会话策略约束 | 便宜；与官方生态一致（子代理工具、任务面板都是官方已有的） |

**结论（暂定）**：先把 **M5**（多 runtime/多会话的 UI）打通——它是两条路的共同底座；
「多智能体」的产品形态（要不要让两个 runtime 互相通信？共享工作区？谁来做调度？）
是**产品决策**，等 M5 落地、真实用起来之后再单独议。此处只留档，不排期。

### 4.7 消息来源 kind 的覆盖缺口（✅ 已落地，留档）

**现状**：`MessageSource` 建模 **20 种** kind（2026-09-29 起），且**已无「可达但未建模」的缺口**。
**未建模的 kind 会让含它的整条消息降级 `Unknown`**——机制、代价与本次取证过程见下。

**先把「权威判据」这件事定了**（2026-09-29 现场取证）：

0.1.2 时代官方在 `packages/llm/llm/src/message.ts` 里有一份基座 `MessageSourceMap`（user/plugin/model/tool），
**0.1.7-rc.2 已没有**——kind 分散到各包的 `declare module '@deepseek-ai/dsh-llm'`。
所以核对只能用这三处，**优先级从高到低**：

| 判据 | 位置 | 性质 |
|---|---|---|
| **真实帧** | `data/wire-logs/*.jsonl` 的 `*.message.source` | 最强：这是本机真跑出来的 |
| **合并后的类型声明** | `dsh/node_modules/@deepseek-ai/dsh-llm/lib/typert.host.js`（typert 注册表里嵌了完整 `.d.ts`） | 强：官方自己生成的合并声明，可 grep 出 map 的 key 与 `kind` 字面量 |
| **迁移表** | `packages/session/session-format-v3-to-v4/src/sources.ts` 的两张表 | 中：V3 插件名 → 现 kind 的官方映射，能推断还有哪些生产者 |
| ❌ 已失效 | `message.ts` 的基座 map | 别再用（我在 §2.1 就是这么读错的） |

**取证结果（扫已安装 runtime = 0.1.7-rc.2，共 291 个 `@deepseek-ai` 包）**：
真实可达、但 dshr **未建模**的 kind 只有 5 种，且形状都极简（从已安装代码里逐条挖出）：

| kind | 产出方（是否挂在本 profile） | wire 形状 |
|---|---|---|
| `plan-mode` | `dsh-plan-mode`（**在 dsh-base 树里**） | `{ kind:'plan-mode', form:'notice', summary }` |
| `model-selection` | `dsh-agent`（**在树里**） | `{ kind:'model-selection', form:'notice', summary }` |
| `user-approval` | `dsh-user-approval`（**在树里**） | `{ kind:'user-approval' }` |
| `ptc-mode` | `dsh-tools` / `dsh-spill-policy`（**在树里**） | `{ kind:'ptc-mode' }` |
| `compact-checkpoint` | `dsh-compaction`（经 `compaction-basic`，**在树里**） | `{ kind:'compact-checkpoint' }`（+ 事务关联字段） |

不在本 profile 的（`time-context` / `tmux-context` / `coordinator` / `subagent-report` /
`tool-cordis` / `cordis-host-runner` / `schedule` / `hooks-*` / `tools-ptc` 等）不进树，**不会出现**。

**代价（精确到「丢什么」）**：`fallback::known()` 的宽容策略会让**整条事件**降级
`SessionEvent::Unknown`——丢的是**结构化视图**，不丢数据：

- 留下的：wire log 里的原始 JSONL 一行（无损）、信封字段（seq/time/ignorable/source_event_seqs/surface_op）；
- 丢掉的：`event_type()` 变 `"unknown"`（日志/统计工具无法按类型分类）、`fold` 的
  `Unknown { .. } => {}` 什么都不做、SQLite 里没有对应行（**库里本来就没有 events 表**，
  7 张表是 runtimes/sessions/requests/turns/tool_calls/file_ops/runtime_logs）。
- **界面上目前看不出差别**：`fold` 只把 `role=user 且 source.kind=user` 折成 User 行，
  `system/message`、`developer/message` 也都不折（s1 策略）。所以这 5 个 kind 即使补上，
  今天的聊天视图也**不会变化**——它们是「s3 按 source.kind 分类渲染」与统计域的**前置条件**。

**✅ 已落地（2026-09-29，方案 B + D 经用户批准）**——过程与坑见 §2.1，这里只留结论与纪律：

- 补了 5 个真实可达的 kind（现共 20 种）；`ContextFormed` 系字段一律按**可选**处理。
- 降级**可识别**：`SessionEvent::Unknown.degraded` 区分「已知类型解析失败」与「类型本身未知」；
  engine 写 `event.degraded` app 轨迹。
- 新增 `scripts/scan-message-sources.mjs`（发现机制）+ `tests/frame_shape.rs` 的真实帧护栏（守已知）。

**纪律（重要，别只做一次）**：**换 runtime 版本或改动 profile 插件集之后，跑一次
`node scripts/scan-message-sources.mjs`**（退出码 1 = 有新 kind 或字段漂移）。
它是对官方真源的发现机制；测试只能守 fixture 里已知的样本，覆盖不到运行时新冒出来的 kind。

**未采纳**：选项 C（`MessageSource::Other { kind }` catch-all）。理由：它会让我们失去「官方封闭集」
这个契约，而 B + D 已经把「真实缺口」清零且能自动发现新的；若将来发现某个 kind 频繁变动、
每次都来不及补，再回来考虑 C（那时它和 D 配合得很自然）。

**遗留待核（同一类风险，已用脚本核过一轮：当前全绿）**：已建模的 `ContextFormed` 系变体
（webhook / skill-catalog / skill-invocation / agent-instructions / session-reference …）
早年把 `form` 声明成**必填**，而官方 `ContextFormed` 允许省略。脚本的逐字段比对现在把这些
条目按「官方声明 = 全可选」来查——**当前没有差异**（那些条目在合并声明里没出现，故未逐字段核；
真要落到证据上，得去各包 `declare module` 逐个看）。⚠️ 这一条是**已知的检查盲区**，
记在这里而不是假装已经验过。

### 4.8 落盘数据的导出（CSV）与「等待可见」（2026-09-29 落地）

**用户诉求**：库里落盘的数据现在没地方看（监控页未做），先能用一种方式**看数据**；
将来这套导出直接变成监控页的历史导出，并且要能导出**会话历史**。

**✅ 已落地**（`dshr-state/src/export.rs` + `tests/export_csv.rs`）：

| 能力 | 数据源 | 产物 |
|---|---|---|
| 库表导出 | `dshr.db` 六张事实表（runtimes/sessions/turns/tool_calls/file_ops/runtime_logs） | 六个 CSV（显式列序 + 稳定排序 → 两次导出逐字节可比） |
| 会话历史 | 内存/回放快照 | `<session>.messages.csv` + `<session>.turns.csv` |
| **跨会话历史** | `data/wire-logs/*.jsonl` | 按 `sessionId` 分组回放，**每个会话一条独立时间线** |

**关键实测（本次真跑）**：从 wire-log 回放出 **42 个会话**，而库里只有 **3 行 session**——
说明**历史的全集在 wire-log 里**，库只装「引擎跑过并落盘过的」。这条事实直接喂给了 §4.5
的 A/B/C 三个方向。真跑入口：`$env:DSHR_EXPORT='1'; cargo test -p dshr-state --test export_csv -- --nocapture`
（导出到 `data/exports/<epoch>-<pid>/`，打印每份文件的行数与几条样本）。

**为什么历史必须靠回放而不是查库**：库里按设计**没有 messages 表**（§8.2 明确不建 events 全量表，
wire-log 已是 lossless 源）；`turns`/`tool_calls` 只是聚合事实。所以「这个会话到底聊了什么」
只有回放能得到——而回放用的 `Folder` 与在线是同一套折叠语义（`fold_projection.rs` 有同源同巡断言），
导出与 UI 不会分叉。

**顺带落地的「等待可见」（数据侧）**：`request/header` 与 `request/context` 此前都折成 `{}`，
**快照无变化 → 脏检测不发事件 → UI 一片静止**（用户无法区分「在等模型」与「卡死了」）。
现在折进 `SessionSnapshot.last_request`（起点时刻/seq/原因/工具数 + provider/model/上下文窗口），
等待这段没有事件的时间**在数据层可观测**。UI 侧的显示（状态栏「正在等待 model · Ns」）
留给 M5 那一轮——本轮只做核心逻辑（用户要求 UI 之前先做核心逻辑）。

### 4.9 M5 待办清单（UI 轮；数据面都已就绪，逐条可勾）

**A. 多 runtime / 多会话（用户明确要的能力）**
1. `dshr-ui/src/app.rs` 的 `const RT_ID = "rt-1"` 固定视图 → 按 `EngineEvent` 里的 runtime 标
   维护 `RuntimeView` 列表（侧边栏已是树形，`data.runtimes` 已能容纳多条）。
2. 「当前会话」概念：`data.chat` 目前是**单会话**状态，要改成 `selected: (RuntimeId, SessionId)`
   + 每会话一份 `ChatState`（或按需拉取，见 B）。
3. 侧边栏交互：切换会话、每条 runtime 的 ⋯ 菜单（新建会话 / 停止）。

**B. M6 的另一半：事件只发轻通知 + UI 按需拉**
4. engine：新增 `EngineEvent::SnapshotChanged { id, session }`（轻），把变更路径的整份快照
   换成它；`start_runtime`/`reset_session`/`ReadSnapshot` 仍发整份（那是「首屏」不是「变更」）。
5. UI：收到 `SnapshotChanged` → 发 `EngineCmd::ReadSnapshot { session }` → 应用 `SessionLoaded`。
   **必须与 4 同批**（只做一侧 = 界面静止）。
6. 切换完成后可以省掉 bridge 里对整份快照的搬运，多会话下的 clone 开销随之消失。

**C. 历史会话（复原）的界面**
7. `EngineCmd::ListSessions` → `EngineEvent::Sessions`：侧边栏挂「历史会话」一层
   （当前 `Sessions` 事件只写一行状态提示，见 `app.rs` 的臂注释）。
8. 点历史会话 → `ReadSnapshot` → `SessionLoaded { runtime: None }` → 以**只读**视图打开
   （此时 composer 应禁用或提示「这是历史会话」——否则用户会往一个不存在的 runtime 发消息）。

**D. 等待可见（§4.8 的数据侧已完成）**
9. 状态栏显示「正在等待 <model> · Ns」：`last_request.started_at` + 与最后一条 assistant 行的
   seq 比较即可判定「还在等」；这是唯一需要 UI 起定时器的地方（其余都事件驱动）。
10. `Injected` / `Attempt` 行已有数据但被 `model.rs` 过滤——若要做「显示注入/尝试」开关，
    把过滤条件参数化即可（渲染臂已写好，见 `task/chat.rs`）。

### 4.10 注释梳理时发现的两处「代码与注释不一致」（**待拍板，本轮只记不改**）

这两条都是 2026-09-30 做块级注释梳理时**顺出来的**：写「为什么这么写」逼着人去看它到底怎么用，
于是看到下面两处。行为都没错，但留着会误导后来者。

1. **`SessionState::last_persisted` 是 write-only**（`engine/session.rs`）：字段被写、**没有任何
   地方读**；更麻烦的是主路径落库发生在 `registry::emit_changed`（直接调 `persist_snapshot`），
   不经过 `take_changed`，所以连「记录落库进度」这层作用都不完整。
   两个选项：**删**（少一个会骗人的字段，推荐）／**留到做落库节流时用**（落库比发 UI 贵，
   将来「每 N 次变化落一次」正好需要这个基线）。当前处理：注释里写明真相，不改行为。
2. **`infer_op` 的 `str_replace` 分支对 `str_replace_editor` 不可达**（`store/write.rs`）：
   工具名里的 `editor` 含 `edit`，所以先命中第一个分支归成 `edit`。`op` 只是展示归类（`path`
   与行数才是事实列），所以不算 bug；要精确得有一张「工具名 → 操作」映射表——**等官方工具名
   清单**（现在没有权威清单，猜表更糟）。当前处理：注释里写明这个顺序是有意的。

### 4.11 文件页 / `state::workspace` 的缺口（**待拍板，2026-09-30 记，只记不做**）

用户 2026-09-30 观察「state 里的 workspace 好像比较少」，逐行核对后确认：**行数不多（157 行），
少的是三件能力**。现状与缺口表已写进 `DESIGN.md` §7.4，待办进了 §12.3。决策清单如下：

| 缺口 | 需要的决定 | 代价 / 影响面 |
|---|---|---|
| 无新建/重命名/**删除** | 删除要不要进回收站（`trash` crate 会引入依赖）？还是只允许删「工作区内自己刚建的文件」？ | 引入依赖 vs 误删不可逆——**这条必须你先定** |
| 保存**非原子** | 接受 temp + rename 吗（会改变文件权限/硬链接语义：rename 后 inode 变了，某些工具（watch 类）会看到「文件被替换」而不是「被修改」） | 编辑器最该守的一条；但改的是写语义，不是纯优 |
| `IGNORED` 硬编码 5 个名字 | 改成读 `.gitignore`？还是保留这张表 + 加一条「子目录自己的 .gitignore 也读」？ | 现在这张表与 `.gitignore` 的 `/data/`、`/dsh/`、`/target/` 基本重合；改造不难，但 `.git` 与 `node_modules` 要单独兜底 |
| 只认 UTF-8 / ≤2 MiB | 要不要「非 UTF-8 与大文件用只读预览打开」（而不是只报错）？ | UI 侧要做一条降级路径（可用 `lossy` 解码 + 截断显示） |
| 无文件监听 | 外部改动被 `Save` 直接覆盖，接受吗？还是保存前比一次 mtime（检测到变了就提示）？ | 监听要平台 API/依赖；mtime 比对是零依赖的折中 |
| **零测试** | 先补契约测试（安全边界）——**推荐先做这条**，与其它决策不冲突 | 低成本，补掉 `_conventions.md` 已列的待测点 |

**顺带记一条事实**（写文档时核的）：`workspace` 读写的是**用户工作区**（`dshr/` 仓库本身），
而 `store`/`record`/`config`/`secrets`/`runtime` 读写的是 `data/` 与 `dsh/`——**两套路径来源**，
`IGNORED` 里的 `data`/`dsh`/`target` 就是为了不让文件页暴露后者。

---

## 5. 与官方仓库对照的方法论

**官方源码是唯一权威，本地克隆在 `D:\dsh\deepseek-harness`。** 核对时：

1. **先查版本**：`git -C <harness> log -1` + 各 `package.json` 的 `version`。
   注意 npm 的 `dist-tags` 可能落后于 master（`latest` 常常不是最新）。
2. **要权威 diff 就用标签**：`git diff dsh-vX dsh-vY -- <文件>`。别读工作树当基线
   （会混入自己的在制品——我在 2.1 就是这么错的）。
3. **事件名集合用脚本对账**：`node scripts/compare-session-events.mjs`（59 项规模手算必错）。
4. **字段级形状要读源文件**，重点这几个「协议真源」：
   - `packages/sdk/protocol/src/types.ts` —— 三进四出、`InitializeParams/Result`
   - `packages/core/session/src/types.ts` —— `SessionEventMap`（核心事件）
   - `packages/core/session/src/known-event-types.ts` —— 事件全集（**上游生成物，勿手改**）
   - `packages/llm/llm/src/{types,message}.ts` —— 内容块、消息、`FinishReason`、`LlmFailure`
   - 各插件包 `declare module '@deepseek-ai/dsh-session/types'` —— 扩展事件的归属处
5. **`app.asar` 里的已发布实现**：`dsh-desktop/resources/app.asar` 是当前运行的 DSH 版本，
   排查「实际行为」时以它为准（源码可能是更新的 master）。它是文本可读的打包文件，
   可以用 Node 按 UTF-8 读取并搜行号（`Get-Content` 也能读，但注意中文显示会乱码）。

---

## 6. 环境与工具链事实

| 项 | 值 | 备注 |
|---|---|---|
| 工作区根 | `D:\dsh` | 非 git 仓库；下辖两个 git 仓库 |
| `dshr` | `D:\dsh\dshr`（git，分支 `main`） | 项目本体 |
| 官方克隆 | `D:\dsh\deepseek-harness`（git，分支 `master`） | 协议权威 |
| 官方桌面端 | `D:\dsh\dsh-desktop` | 用户当前与我沟通所用的客户端 |
| 路径依赖 | `D:\dsh\包\iced-code-editor` | 有本地补丁，重新下载会丢 |
| Rust | 1.95.0（`C:\Users\qiaoy\.cargo\bin`） | |
| Node | v24.13.0 | 官方要求 `^22.19 \|\| >=24` |
| pnpm | 11.21.0 | store 在 `D:\.pnpm-store\v11` |
| Windows 原生工具链 | **无** MSVC / cmake / gcc | 官方 `build:native-system` 在 Windows 走
  `--host-addon-only` 分支直接退出 0，所以**不影响**构建 |
| 卷 | C: `OS`（NTFS）、D: `新加卷`（NTFS） | 两卷的根 ACL 不同，见 2.2 |

**工作区级脚本**（`D:\dsh\scripts\`，详情见 `pipeline.json`）：

| 脚本 | 用途 |
|---|---|
| `compare-session-events.mjs` | 官方 ↔ dshr 事件名集合对账（退出码 1 = 有差异） |
| `scan-message-sources.mjs` | 消息来源（MessageSource）形状对账 + 新 kind 发现（退出码 1 = 有差异；`--write-fixture` 刷新测试样本） |
| `split-fold.mjs` / `split-store.mjs` | 两次 God file 拆分的可复核记录（一次性，已执行） |
| `pipeline.json` / `README.md` | 步骤索引与目录约定 |
