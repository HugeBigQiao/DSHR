//! 配置加载：`config.json`（workspace 根，gitignore）。
//!
//! 主要用途：`runtime::ensure` 与 `raw::mode` 从这里取 provider / model / dsh runtime 锁版本；
//! UI 配置页也读写同一份（保存按钮显式落盘，非自动保存）。
//! 为什么需要：dsh runtime 需要显式指定 provider 与 model 才能 `initialize`，
//! 且 runtime 版本必须**锁定**（npm `latest` 长期不含 sdk profile）——这些是产品级配置，
//! 不能散落在各调用点或硬编码。
//! 密钥不在这里：自 2026-09-10 起 `api-key` 迁到 `data/secrets.json`（见 `secrets.rs`）；
//! 旧 `config.json` 里的 `api-key` 字段会在读取时自动迁移并删除。
//! `api_key: None` = 未配置 → Real 模式自动回退 Fake。
//! 上接：`raw/mode.rs`（spawn 装配）、`main.rs`、`dshr-ui` 配置页。
//! 下接：`secrets.rs`（读 key）、文件系统。
//! 官方对应：无（官方配置在 `dsh-home/settings.yaml` 与 cordis profile patch；dshr 自己这一层
//! 只解决「用哪个 provider/model/哪个 runtime 版本」）。
use std::path::Path;

/// dshr 运行时配置。
///
/// 为什么需要：把「读一个 JSON 文件 + 缺省值 + 密钥迁移」收敛成一次调用，
/// 使调用方（`raw/mode`、UI）不必各自处理缺字段与旧格式。
#[derive(Debug, Clone)]
pub struct Config {
    /// DeepSeek API key（来自 `data/secrets.json`；`None` = 未配置）。
    pub api_key: Option<String>,
    /// provider（默认 deepseek-official）。
    pub provider: String,
    /// model（默认 deepseek-v4-flash，与官方 `dsh-home` 的 agent-default-model 默认一致）。
    pub model: String,
    /// dsh runtime 锁版本（npm `latest` 无 sdk profile——见 `DESIGN.md` §10）。
    pub dsh_version: String,
    /// npm registry 覆盖（可选）：下载 runtime 时**优先**用它。
    /// 空 = 用内置顺序（官方 → npmmirror 回退）。
    ///
    /// 为什么做成配置而不是只认环境变量：内网镜像/私有代理是常见需求，写进
    /// `config.json` 能跟着工作区走；环境变量更适合临时调试。
    /// 优先级：`DSHR_NPM_REGISTRY` 环境变量 > 本字段 > 内置顺序。
    pub npm_registry: Option<String>,
}

impl Config {
    /// 是否已配置非空 API key。
    pub fn has_api_key(&self) -> bool {
        self.api_key
            .as_deref()
            .map(str::trim)
            .is_some_and(|key| !key.is_empty())
    }
}

/// 从 `config.json` 加载；`provider`/`model`/`dsh-version` 缺省回退默认值。
/// API key 从 `data/secrets.json` 读取（并自动迁移旧 `config.json` 字段）。
pub fn try_load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读 {path:?} 失败: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 {path:?} 失败: {e}"))?;
    let workspace = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Config {
        api_key: crate::secrets::load_api_key(workspace),
        provider: value
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("deepseek-official")
            .to_string(),
        model: value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("deepseek-v4-flash")
            .to_string(),
        dsh_version: value
            .get("dsh-version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("0.1.7-rc.2")
            .to_string(),
        npm_registry: value
            .get("npm-registry")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// CLI 路径使用的 panic 包装；UI/engine 请优先用 [`try_load`]。
pub fn load(path: &Path) -> Config {
    try_load(path).unwrap_or_else(|error| panic!("{error}"))
}
