//! runtime 获取：确保 node 可用 + 安装/校验锁定版本的 `@deepseek-ai/dsh`（落在 `dsh/`）。
//!
//! 主要用途：`raw/mode.rs` 在 Real 模式装配 spawn 前调用 [`ensure`]，拿到 dsh 的 bin 入口路径。
//! 为什么需要：dsh 本体不随 dshr 发布，用户机器上可能完全没有；而 runtime 版本**必须锁定**
//! （npm `latest` 长期不含 sdk profile——见 `DESIGN.md` §10）。本模块把「检测 → 安装/升级 → 返回入口」
//! 收敛成一次调用，并保证失败时给出可诊断的错误而不是让 spawn 神秘失败。
//!
//! 目录设计：
//!   `dsh/`           程序本体（发布不带；运行时检测/下载；删除可重下）
//!   `data/dsh-home/` runtime 的独立 HOME（profiles/sessions 等状态；**不碰**用户 `~/.dsh`）
//!   `data/`          dshr 自己的库与配置（sqlite、wire-logs、secrets）
//!
//! 实现要点：包管理器用 **pnpm**（共享全局 store 跨项目去重 + `--ignore-scripts`：
//! 实测 node-pty/koffi 的 tarball 自带预编译产物，跳过构建完全可用，从而免掉 node-gyp 工具链）；
//! `pnpm-workspace.yaml` 用 `nodeLinker: hoisted`（对齐官方 profile 的解析与平铺方式）。
//! 版本校验读**已安装包的 `package.json`**，不用 `InitializeResult.serverInfo.version`
//! （官方把它硬编码成 `'0.0.1'`，见 `AI-LOG.md` §2.1）。
//!
//! **下载稳定性（2026-09-29 加固）**：`@deepseek-ai/dsh` 本体只有 69 KB，但它声明的
//! 81 个依赖会解析成 **587 个包**，其中含 `sharp` / `sherpa-onnx` / `koffi` /
//! `libreoffice-kit` 的**全平台**二进制（且都不是 optional，无法跳过）。弱网下
//! 单次安装必然偶发失败（实测：两次都在 356/587 处 socket 超时）。因此这里做了三件事：
//! 1. **重试循环 + 指数退避**——关键前提是 pnpm 的 content-addressable store 是
//!    **断点续传式**的：重试时已下载的 tarball 会复用，所以重试不是从头再来；
//! 2. **写入 `.npmrc`** 提高取数韧性（npm 默认 `fetch-retries=2`，对弱网太弱）；
//! 3. **registry 回退链**：官方 registry 失败后换镜像（弱网/国内网络下这是决定性的）。
//! 上接：`raw/mode.rs`（`kit`）。
//! 下接：`config.rs`（锁版本号与 registry 覆盖来源）、node / pnpm 可执行文件、文件系统。
//! 官方对应：无（官方不提供宿主侧的 runtime 安装器；这是 dshr 为自己的分发形态所做的封装）。
use std::path::{Path, PathBuf};
use std::process::Command;

/// dsh 安装目录下的 bin 入口（相对 `dsh/`）。
///
/// 官方 `apps/cli/package.json` 的 bin 指向编译产物；此处是它的安装后位置。
pub const DSH_BIN_REL: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";

/// node 最低版本（官方 engines：`^22.19 || >=24`）。
const NODE_MIN: (u32, u32) = (22, 19);

/// 每个 registry 的安装尝试次数（失败后指数退避重试）。
///
/// 为什么是 3：pnpm 的重试是**续传式**的，同 registry 多试几次的边际收益递减；
/// 真正有效的是换 registry（见 [`registries`] 的回退链），所以单个 registry 不必恋战。
const ATTEMPTS_PER_REGISTRY: u32 = 3;

/// 内置 registry 回退链：官方 → npmmirror。
///
/// 为什么需要回退而不是只认官方：实测弱网下 `registry.npmjs.org` 会连续 socket 超时
/// （`error (23)`），两次安装都在 356/587 处中断；而镜像（`registry.npmmirror.com`）
/// 已镜像 `@deepseek-ai` scope（实测能取到 `@deepseek-ai/dsh@0.1.7-rc.2`，
/// dist-tags 与官方一致），换过去通常能一次装完。
///
/// 顺序把官方放第一，是为了不让第三方镜像成为默认信任源——只有在官方失败后才回退。
const DEFAULT_REGISTRIES: [&str; 2] = [
    "https://registry.npmjs.org/",
    "https://registry.npmmirror.com/",
];

fn parse_node_version(output: &str) -> Option<(u32, u32)> {
    let v = output.trim().trim_start_matches('v');
    let mut parts = v.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// 检测 node：PATH 上有满足版本的 node 才继续；否则报清晰错误。
/// （自动安装 portable node 是下一步：缺失时下载官方 zip 到 `dsh/node-<ver>/`。）
pub fn ensure_node() -> Result<(), String> {
    let out = Command::new("node")
        .arg("--version")
        .output()
        .map_err(|e| {
            format!(
                "未找到 node（dsh 需要 Node.js ≥{}.{}，请安装并加入 PATH，或等自动安装支持）: {e}",
                NODE_MIN.0, NODE_MIN.1
            )
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    let Some((major, minor)) = parse_node_version(&text) else {
        return Err(format!("node --version 输出无法解析: {text}"));
    };
    if (major, minor) >= NODE_MIN || major >= 24 {
        Ok(())
    } else {
        Err(format!(
            "node 版本过低: {}（需要 ≥{}.{}，推荐 24+）",
            text.trim(),
            NODE_MIN.0,
            NODE_MIN.1
        ))
    }
}

/// 已安装的 `@deepseek-ai/dsh` 版本（读安装包 package.json；缺失/损坏 = None）。
fn installed_version(dsh_dir: &Path) -> Option<String> {
    let manifest = dsh_dir.join("node_modules/@deepseek-ai/dsh/package.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// 写入/覆盖 `dsh/package.json` 与 `pnpm-workspace.yaml`，声明目标版本。
fn write_manifest(dsh_dir: &Path, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(dsh_dir).map_err(|e| format!("建 dsh 目录失败: {e}"))?;
    let manifest = serde_json::json!({
        "name": "dshr-dsh-runtime",
        "private": true,
        "type": "module",
        "dependencies": { "@deepseek-ai/dsh": version },
    });
    std::fs::write(
        dsh_dir.join("package.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("序列化 package.json 失败: {e}"))?,
    )
    .map_err(|e| format!("写 package.json 失败: {e}"))?;
    // 对齐官方 profile（packages/boot/app-boot/src/profile.ts 的 PROFILE_PNPM_WORKSPACE）：hoisted 平铺。
    std::fs::write(
        dsh_dir.join("pnpm-workspace.yaml"),
        "packages:\n  - .\n\nnodeLinker: hoisted\n",
    )
    .map_err(|e| format!("写 pnpm-workspace.yaml 失败: {e}"))?;
    Ok(())
}

/// 要尝试的 registry 列表（按优先级）。
///
/// # 来源与优先级
/// 1. `DSHR_NPM_REGISTRY` 环境变量（临时调试用；设了就**只试它**，便于内网代理）；
/// 2. `config.json` 的 `npm-registry`（跟着工作区走；设了也只试它）；
/// 3. 否则用内置回退链 [`DEFAULT_REGISTRIES`]（官方 → npmmirror）。
///
/// 为什么环境变量/配置**覆盖**而不是"插到最前面"：内网代理往往连不上公网，
/// 若还去试官方 registry 只会白等一轮超时；显式指定就是"只走这条"。
fn registries(dsh_dir: &Path) -> Vec<String> {
    for (name, val) in [
        ("DSHR_NPM_REGISTRY", std::env::var("DSHR_NPM_REGISTRY").ok()),
        (
            "config.json 的 npm-registry",
            crate::config::try_load(
                &dsh_dir
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("config.json"),
            )
            .ok()
            .and_then(|c| c.npm_registry),
        ),
    ] {
        if let Some(v) = val {
            let v = v.trim();
            if !v.is_empty() {
                let mut url = v.to_string();
                if !url.ends_with('/') {
                    url.push('/'); // npm 的 registry 约定以 / 结尾。
                }
                eprintln!("[runtime] 使用指定 registry（{name}）：{url}");
                return vec![url];
            }
        }
    }
    DEFAULT_REGISTRIES.iter().map(|s| s.to_string()).collect()
}

/// 写 `dsh/.npmrc`：提高取数韧性 + 指明本次使用的 registry。
///
/// # 为什么用 `.npmrc` 而不是命令行旗标
/// `pnpm install --help` 里**没有** `--fetch-retries`（实测 `--config.fetch-retries=10`
/// 被接受，但 `--config.network-concurrency` 会因"期望数字得到字符串"而报错）。
/// 写成文件最稳：pnpm 会自动读项目根的 `.npmrc`，且不受命令行类型转换影响。
///
/// # 各项为什么这么设
/// - `fetch-retries / *-mintimeout / *-maxtimeout`：npm 默认 `fetch-retries=2`，
///   对弱网太弱（日志里的"2 retries left"就是它）；10 次 + 10s~120s 退避。
/// - `fetch-timeout`：默认偏短；放大到 10 分钟，容忍慢速 tarball。
/// - `network-concurrency`：默认并发较高，弱网下**并发越低越稳**（每次连接更少被掐断）。
/// - `prefer-offline`：优先用 store 里的既有 tarball——配合"重试即续传"这一事实，
///   让第二次尝试真的跳过已下好的部分。
/// - `strict-ssl=true`：保持默认校验，不因为要"稳"而降低安全性。
fn write_npmrc(dsh_dir: &Path, registry: &str) -> Result<(), String> {
    let body = format!(
        "# 由 dshr 生成（runtime.rs::write_npmrc）——重启/换 registry 时会被覆盖，手改无效。\n\
         registry={registry}\n\
         fetch-retries=10\n\
         fetch-retry-mintimeout=10000\n\
         fetch-retry-maxtimeout=120000\n\
         fetch-timeout=600000\n\
         network-concurrency=4\n\
         prefer-offline=true\n\
         strict-ssl=true\n"
    );
    std::fs::write(dsh_dir.join(".npmrc"), body).map_err(|e| format!("写 .npmrc 失败: {e}"))
}

/// 在 `dsh_dir` 执行**一次** `pnpm install --force`（重试由 [`pnpm_install`] 负责）。
///
/// 为什么保留 `--force`：切换版本后必须真正重装（否则旧版本的 node_modules 会被留着）；
/// 代价是每次都重新"链接"一遍——但 tarball 走 store 复用，实际网络开销只有缺的那些。
fn pnpm_install_once(dsh_dir: &Path) -> Result<(), String> {
    let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
    let mut cmd = Command::new(pnpm);
    cmd.args([
        "install",
        "--force",
        "--ignore-scripts",
        "--config.minimumReleaseAge=0",
    ]);
    if let Ok(store) = std::env::var("DSHR_PNPM_STORE") {
        if !store.is_empty() {
            cmd.arg(format!("--store-dir={store}"));
        }
    }
    let status = cmd
        .current_dir(dsh_dir)
        .status()
        .map_err(|e| format!("运行 {pnpm} install 失败（请确认 pnpm 在 PATH）: {e}"))?;
    if !status.success() {
        return Err(format!("pnpm install 失败（exit {status}）"));
    }
    Ok(())
}

/// 带重试与 registry 回退的安装：把「一次 `pnpm install` 可能因网络失败」变成
/// 「同 registry 重试几次，仍不行就换镜像」。
///
/// # 为什么重试是有效的（而不是把失败往后推）
/// pnpm 的 store 是 **content-addressable** 的：已下载的 tarball 在下次尝试时会
/// `reused` 而不是重下（实测日志里 `reused 204` 就是这个）。所以重试**不是从头再来**，
/// 每轮都在上一次的进度上继续——这正是弱网下最有性价比的加固手段。
///
/// # 退避
/// 第 n 次失败后睡 `5s * 2^n`（5s/10s/20s），并打印进度：
/// 用户（与日志）能看出"在重试"而不是"卡住了"。
fn pnpm_install(dsh_dir: &Path, registries: &[String]) -> Result<(), String> {
    let mut last_err = String::from("未尝试任何 registry");
    for (ri, registry) in registries.iter().enumerate() {
        if let Err(e) = write_npmrc(dsh_dir, registry) {
            return Err(e); // 写不了 .npmrc 是环境问题，重试无意义。
        }
        for attempt in 1..=ATTEMPTS_PER_REGISTRY {
            eprintln!(
                "[runtime] pnpm install（registry {}/{registries_len} = {registry}，第 {attempt}/{ATTEMPTS_PER_REGISTRY} 次）",
                ri + 1,
                registries_len = registries.len()
            );
            match pnpm_install_once(dsh_dir) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = format!("{registry} 第 {attempt} 次失败：{e}");
                    eprintln!("[runtime] {last_err}");
                    if attempt < ATTEMPTS_PER_REGISTRY {
                        // 指数退避：5s / 10s / 20s（弱网抖动通常几秒到几十秒）。
                        let backoff = 5u64 * (1 << (attempt - 1));
                        eprintln!("[runtime] {backoff}s 后重试（已下载的包会被复用，不会重头来）");
                        std::thread::sleep(std::time::Duration::from_secs(backoff));
                    }
                }
            }
        }
        if ri + 1 < registries.len() {
            eprintln!("[runtime] 换下一个 registry 重试");
        }
    }
    Err(format!(
        "所有 registry 均安装失败（共 {} 个）：{last_err}",
        registries.len()
    ))
}

/// 确保 `dsh_dir` 下的 `@deepseek-ai/dsh` 是 `version`：
/// - 版本匹配且 bin 存在 → 直接返回（**不碰网络**）；
/// - 版本缺失/不匹配 → 重写 package.json，按 registry 回退链安装，安装后再次校验。
/// 返回 bin 绝对路径。
pub fn ensure(dsh_dir: &Path, version: &str) -> Result<PathBuf, String> {
    ensure_node()?;
    let bin = dsh_dir.join(DSH_BIN_REL);
    let installed = installed_version(dsh_dir);
    if installed.as_deref() == Some(version) && bin.exists() {
        return Ok(bin);
    }

    write_manifest(dsh_dir, version)?;
    let registries = registries(dsh_dir);
    pnpm_install(dsh_dir, &registries)?;

    let installed = installed_version(dsh_dir);
    if installed.as_deref() != Some(version) {
        return Err(format!(
            "dsh runtime 版本不匹配：期望 {version}，实际 {}",
            installed
                .as_deref()
                .unwrap_or("未知（未读到安装包 package.json）")
        ));
    }
    if !bin.exists() {
        return Err(format!(
            "pnpm install 成功但 dsh bin 缺失（包结构变化？）: {}",
            bin.display()
        ));
    }
    Ok(bin)
}

/// 只装依赖、不校验 bin：**冷启动预热**用（UI 可先调它把下载做掉，再 `ensure` 秒过）。
///
/// 为什么需要与 `ensure` 分开：`ensure` 的语义是「拿到可用 bin」，失败即整个 Real 模式
/// 不可用；而预热是"尽量先把 587 个包下好"。二者失败处理不同，混在一起会让调用方
/// 难以区分「没装」与「装坏了」。
pub fn prefetch(dsh_dir: &Path, version: &str) -> Result<(), String> {
    ensure_node()?;
    write_manifest(dsh_dir, version)?;
    let registries = registries(dsh_dir);
    pnpm_install(dsh_dir, &registries)
}
