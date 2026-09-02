//! 配置加载（决策 12：`config.json` 在 workspace 根，gitignore）。
use std::path::Path;

/// dshr 运行时配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// DeepSeek API key（config.json 的 `api-key` 字段）。
    pub api_key: String,
    /// provider（默认 deepseek-official）。
    pub provider: String,
    /// model（默认 deepseek-v4-flash，与 `~/.dsh/settings.yaml` 的 agent-default-model 一致）。
    pub model: String,
    /// dsh runtime 锁版本（npm latest 无 sdk profile——DESIGN §7）。
    pub dsh_version: String,
}

/// 从 config.json 加载；`provider`/`model`/`dsh-version` 缺省回退默认值。
pub fn load(path: &Path) -> Config {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读 {path:?} 失败: {e}"));
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("解析 {path:?} 失败: {e}"));
    Config {
        api_key: v
            .get("api-key")
            .and_then(serde_json::Value::as_str)
            .expect("config.json 缺 api-key 字段")
            .to_string(),
        provider: v
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("deepseek-official")
            .to_string(),
        model: v
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("deepseek-v4-flash")
            .to_string(),
        dsh_version: v
            .get("dsh-version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("0.1.2-alpha.5")
            .to_string(),
    }
}
