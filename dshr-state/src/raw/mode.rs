//! raw 层的「运行模式判定 + spawn 装配」。
//!
//! 主要用途：`Runtime::start` 调 [`resolve_mode`] 决定走 Fake 还是 Real，再调 [`kit`] 拿到
//! 完整的 `SpawnKit`（spawn 配置 + initialize 参数 + 模式 + 展示 label）。
//! 为什么需要（独立成模块）：这是**唯一的策略判定点**——Fake 还是 Real、用哪个 bin、
//! 装什么环境变量，全部集中在此。散开就会让「为什么这次跑的是 fake」难以回答。
//! 它也是 raw 层里唯一知道「config 与 runtime 安装」的存在（`Runtime` 本身不关心）。
//!
//! 运行模式：
//! - **Fake**（开发/默认路径）：node 直跑 `dsh-sdk-client` 测试用的 fake runtime
//!   （`tests/fixtures/fake_runtime.mjs`，回显 `"hello from fake"`），**无需 API key**。
//! - **Real**：`config.json` 存在且 api-key 非空时才走；provider/model/api-key/dsh-version
//!   来自 [`crate::config::load`]，dsh 本体经 [`crate::runtime::ensure`] 拿 bin，
//!   env 与 initialize 参数对齐 `session.rs` 的全链路语义。
//!   `config.json` 缺失/畸形/api-key 为空 → **自动回落 Fake**，并在 label 里说明原因（UI 状态行可见）。
//!
//! 上接：`raw.rs`（`Runtime::start`）。
//! 下接：`config`（配置）、`runtime`（锁版本安装与 bin 路径）、`dsh_sdk_client`（spawn 配置类型）、
//!       `dsh_sdk_protocol`（initialize 参数类型）。
//! 官方对应：无（官方 runtime 由用户自行启动；本模块是 dshr 为「开发用假 runtime / 生产用真 runtime」
//! 双路径所做的装配层）。
//!
//! 注：`DSH_HOME` 指向 `data/dsh-home/`，**不碰**用户 `~/.dsh`；工作区由 `DSH_CWD` 与
//! `InitializeParams.cwd` 双重锁定。
use std::path::{Path, PathBuf};

use dsh_sdk_client::client::HarnessSpawnConfig;
use dsh_sdk_protocol::requests::InitializeParams;

use crate::config;
use crate::runtime;

/// workspace 根（= 本 crate（dshr-state）的父目录——DSHR 根）。
/// 与迁入前 dshr-ui 版同语义：env!("CARGO_MANIFEST_DIR") 的 parent；
/// config.json / dsh/ / data/ 都在这里。编译期绝对路径 → 运行时可稳定使用。
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dshr-state 应在 workspace 成员目录")
        .to_path_buf()
}

/// 运行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// node 直跑 fake runtime（无 API key 的开发路径）。
    Fake,
    /// 真实 dsh（config.json 配 provider/model/api-key/dsh-version + runtime ensure）。
    Real,
}

/// 判定结果：模式 + 回落说明（Fake 的 label 后缀 / 无说明）。
#[derive(Debug)]
pub struct ResolvedMode {
    pub mode: RuntimeMode,
    /// Fake 时的回落原因（"无 config.json" / "未配置 api-key" 等）。
    pub note: String,
}

/// 模式判定（集中点）：仅当 workspace 根 config.json 存在且 api-key 非空 → Real；
/// 否则自动回落 Fake。软判定：文件缺失/解析失败一律回落 Fake（开发机无 key 的常态）。
pub fn resolve_mode() -> ResolvedMode {
    let path = workspace_root().join("config.json");
    if !path.exists() {
        return ResolvedMode {
            mode: RuntimeMode::Fake,
            note: "无 config.json".to_string(),
        };
    }
    match config::try_load(&path) {
        Ok(cfg) if cfg.has_api_key() => ResolvedMode {
            mode: RuntimeMode::Real,
            note: String::new(),
        },
        Ok(_) => ResolvedMode {
            mode: RuntimeMode::Fake,
            note: "未配置 API key".to_string(),
        },
        Err(_) => ResolvedMode {
            mode: RuntimeMode::Fake,
            note: "config.json 解析失败".to_string(),
        },
    }
}

/// spawn 装配结果：进程配置 + initialize 参数 + 侧边栏 label。
#[derive(Debug)]
pub struct SpawnKit {
    pub mode: RuntimeMode,
    pub label: String,
    pub config: HarnessSpawnConfig,
    pub init: InitializeParams,
}

/// 按判定模式取装配（resolve_mode 已保证 Real 分支的 config.json 可解析）。
/// Real 的 `runtime::ensure` 可能跑 pnpm install（网络/分钟级）——同步阻塞，
/// 调用方（engine::Engine::start）须用 tokio::task::spawn_blocking 包裹。
pub fn kit(mode: RuntimeMode, ws: &Path) -> Result<SpawnKit, String> {
    match mode {
        RuntimeMode::Fake => Ok(fake_kit(ws)),
        RuntimeMode::Real => real_kit(ws),
    }
}

/// Fake：node 跑 dsh-sdk-client 的 fake_runtime.mjs（进程存活期长，按行读 stdin）。
fn fake_kit(ws: &Path) -> SpawnKit {
    // env!("CARGO_MANIFEST_DIR") 编译期绝对路径 → 运行时可稳定找到 fixture。
    let sdk_dir = ws.join("dsh-sdk-client");
    let script = sdk_dir.join("tests/fixtures/fake_runtime.mjs");
    SpawnKit {
        mode: RuntimeMode::Fake,
        label: "Fake runtime".to_string(),
        config: HarnessSpawnConfig {
            command: "node".to_string(),
            args: vec![script.to_string_lossy().into_owned()],
            current_dir: sdk_dir.to_string_lossy().into_owned(),
            env: vec![],
            request_timeout_ms: 5_000,
            dispose_eof_grace_ms: 2_000,
            dispose_kill_grace_ms: 1_000,
            wire_log_path: None, // engine Start 时接线（engine.rs spawn_client：WireLog 全程记录）。
        },
        init: InitializeParams {
            cwd: ws.to_string_lossy().into_owned(),
            provider: "fake".to_string(),
            model: "fake-model".to_string(),
            reasoning_effort: None,
            max_tokens: None,
        },
    }
}

/// Real：config.json → dsh 本体 ensure → spawn（env/initialize 对齐 crate::session
/// 全链路：DSH_HOME 独立、DSH_CWD=workspace、DSH_SESSION_ROOT 数据目录）。
fn real_kit(ws: &Path) -> Result<SpawnKit, String> {
    let cfg = config::try_load(&ws.join("config.json"))?;
    let api_key = cfg
        .api_key
        .clone()
        .ok_or_else(|| "未配置 API key".to_string())?;
    // dsh/ 目录锁版本安装/升级（runtime.rs ensure：版本不匹配时 pnpm install --force）。
    let dsh_dir = ws.join("dsh");
    let bin = runtime::ensure(&dsh_dir, &cfg.dsh_version)?;
    Ok(SpawnKit {
        mode: RuntimeMode::Real,
        label: format!("dsh runtime · {}", cfg.model),
        config: HarnessSpawnConfig {
            command: "node".to_string(),
            args: vec![
                bin.to_string_lossy().into_owned(),
                "--profile".to_string(),
                "sdk".to_string(),
            ],
            current_dir: ws.to_string_lossy().into_owned(),
            env: vec![
                ("DEEPSEEK_API_KEY".to_string(), api_key),
                (
                    "DSH_HOME".to_string(),
                    ws.join("data/dsh-home").to_string_lossy().into_owned(),
                ),
                ("DSH_CWD".to_string(), ws.to_string_lossy().into_owned()),
                (
                    "DSH_SESSION_ROOT".to_string(),
                    ws.join("data/sessions").to_string_lossy().into_owned(),
                ),
            ],
            request_timeout_ms: 30_000,
            dispose_eof_grace_ms: 2_000,
            dispose_kill_grace_ms: 1_000,
            wire_log_path: None, // engine Start 时接线（engine.rs spawn_client：WireLog 全程记录）。
        },
        init: InitializeParams {
            cwd: ws.to_string_lossy().into_owned(),
            provider: cfg.provider.clone(),
            model: cfg.model.clone(),
            reasoning_effort: None,
            max_tokens: None,
        },
    })
}
