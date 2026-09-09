//! 配置加载（决策 12：`config.json` 在 workspace 根，gitignore）。
//!
//! 自 2026-09-10 起，`api-key` 不再存放在 `config.json`：优先读 `data/secrets.json`，
//! 旧字段会自动迁移并从 `config.json` 删除。`api_key: None` = 未配置，Real 模式回退 Fake。
use std::path::Path;

/// dshr 运行时配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// DeepSeek API key（来自 `data/secrets.json`；`None` = 未配置）。
    pub api_key: Option<String>,
    /// provider（默认 deepseek-official）。
    pub provider: String,
    /// model（默认 deepseek-v4-flash，与 `~/.dsh/settings.yaml` 的 agent-default-model 一致）。
    pub model: String,
    /// dsh runtime 锁版本（npm latest 无 sdk profile——DESIGN §7）。
    pub dsh_version: String,
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
            .unwrap_or("0.1.5-alpha.1")
            .to_string(),
    })
}

/// CLI 路径使用的 panic 包装；UI/engine 请优先用 [`try_load`]。
pub fn load(path: &Path) -> Config {
    try_load(path).unwrap_or_else(|error| panic!("{error}"))
}
