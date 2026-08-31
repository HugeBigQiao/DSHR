//! 配置系统（去 .env，决策 12）：dshr 自身配置 + secrets + dsh 传输配置。
//!
//! 三个文件都在 `data/` 下：
//! - `config.json`：dshr 自身配置（写死 Default → 首次生成 pretty-JSON 模板 → 加载失败回退 Default）
//! - `secrets.json`：敏感项（API key 等，gitignore；模板默认空，用户填）
//! - `cordis.yml`：dsh 传输配置（官方基线文本模板，dshr 不解析，spawn 时把路径传给 dsh）

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

/// cordis.yml 基线模板（官方 examples/jsonrpc-agent/cordis.yml）。
/// 供 data/cordis.yml 首次生成与配置页恢复默认用。
pub const CORDIS_TEMPLATE: &str = include_str!("cordis_template.yml");

/// UI 外观配置（`data/config.json` 的 ui 字段，配置页「外观」区块可改）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// 主题 id：light / dark / gray / tokyo-night / dracula / solarized（未知回退 tokyo-night）。
    #[serde(default)]
    pub theme: String,
    /// 全局字号基准（UI 文本 size 按它缩放，默认 14）。
    #[serde(default)]
    pub font_size: u16,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "tokyo-night".to_string(),
            font_size: 14,
        }
    }
}

/// dshr 自身配置（`data/config.json` 可覆盖，写死 Default 兜底）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshrConfig {
    pub provider: String,
    pub model: String,
    pub max_tokens: u64,
    /// 官方仓库根（A1 阶段；resolve_runtime 落地后换成管理目录）。
    pub harness_root: String,
    /// 会话日志根（传给 DSH_SESSION_ROOT）。
    pub session_root: String,
    /// UI 外观（老 config.json 缺字段时回退默认）。
    #[serde(default)]
    pub ui: UiConfig,
    /// npm 镜像源（决策 23 下载 dsh 用；空 = 官方 registry，如 https://registry.npmmirror.com）。
    #[serde(default)]
    pub npm_registry: String,
}

impl Default for DshrConfig {
    fn default() -> Self {
        Self {
            provider: "deepseek-official".to_string(),
            model: "deepseek-v4-flash".to_string(),
            max_tokens: 4096,
            harness_root: String::new(),
            session_root: String::new(),
            ui: UiConfig::default(),
            npm_registry: String::new(),
        }
    }
}

/// 敏感配置（`data/secrets.json`，gitignore；模板默认全空，用户填）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Secrets {
    pub api_key: Option<String>,
}

/// 启动时用的完整配置（由三个文件组装）。
#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    /// cordis.yml 路径（spawn 时传给 dsh）。
    pub cordis_path: PathBuf,
    pub dshr: DshrConfig,
    pub secrets: Secrets,
}

/// 极简 confy 思路：加载 JSON 配置，不存在/解析失败 → Default 并生成 pretty-JSON 模板。
/// 接收：文件路径。
/// 处理：读文件 → serde 解析；缺失 → 序列化 Default 写模板。
/// 生成：生效值（损坏时打日志回退 Default，模板保留供用户修复）。
pub fn load_or_default_json<T>(path: &Path) -> T
where
    T: Serialize + serde::de::DeserializeOwned + Default,
{
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("配置解析失败（{}），回退默认值：{e}", path.display());
                T::default()
            }
        },
        Err(_) => {
            let t = T::default();
            write_json_template(path, &t);
            t
        }
    }
}

/// 把 Default 值写为 pretty-JSON 模板（覆盖原文件，配置页"恢复默认"用）。
pub fn write_json_template<T: Serialize>(path: &Path, value: &T) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(path, json);
    }
}

/// 写文本文件（配置页"保存"用，直接落盘用户编辑的内容）。
pub fn save_text(path: &Path, text: &str) -> Result<(), Error> {
    std::fs::write(path, text).map_err(Error::from)
}

/// cordis.yml 不存在时写入官方基线模板。
fn ensure_cordis_template(path: &Path) -> Result<(), Error> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, CORDIS_TEMPLATE).map_err(Error::from)
}

/// 从 workspace 根加载全部配置（去 .env，决策 12）。
/// 接收：workspace 根（dshr/）。
/// 处理：canonicalize → data 目录 → 三文件（config/secrets/cordis）加载/生成。
/// 生成：Config（文件缺失/损坏回退 Default，模板保留可改）。
pub fn load(workspace_root: &Path) -> Result<Config, Error> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| Error::Config(format!("建 data 目录失败: {e}")))?;

    let mut dshr = load_or_default_json::<DshrConfig>(&data_dir.join("config.json"));
    let secrets = load_or_default_json::<Secrets>(&data_dir.join("secrets.json"));
    let cordis_path = data_dir.join("cordis.yml");
    ensure_cordis_template(&cordis_path)?;

    // session_root 缺省到 data/sessions：空字符串会原样传给 DSH_SESSION_ROOT，
    // 官方持久化插件的 `?? './.sessions'` 不会兜底（空串非 undefined），root="" 会报错。
    if dshr.session_root.is_empty() {
        dshr.session_root = data_dir.join("sessions").to_string_lossy().to_string();
    }

    Ok(Config {
        data_dir: data_dir.clone(),
        db_path: data_dir.join("dshr.db"),
        cordis_path,
        dshr,
        secrets,
    })
}
