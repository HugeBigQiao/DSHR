//! 配置读取：.env → Config。
//!
//! 数据路径（DESIGN 决策 14）：`DSH_DATA_DIR`（.env）→ 缺省 `dshr/data/dshr.db`；
//! 未来 env 并入 setting（toml）后本文件是唯一要改的地方。

use std::path::{Path, PathBuf};

use crate::Error;

/// 启动所需的全部配置（从 .env 读，见 dshr/.env.example）。
#[derive(Debug, Clone)]
pub struct Config {
    /// 本地数据库文件路径（DSH_DATA_DIR 或 dshr/data/dshr.db）。
    pub db_path: PathBuf,
    /// 官方仓库根（node carrier 的 cwd 来源）。
    pub harness_root: String,
    pub api_key: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    /// 会话日志根（传给 DSH_SESSION_ROOT）。
    pub session_root: String,
}

/// 从 workspace 根加载 .env 并组装 Config。
/// 接收：workspace 根路径（dshr/）。
/// 处理：dotenvy 加载 .env → 逐项读环境变量；DSH_DATA_DIR 缺省回退 dshr/data/dshr.db；
///       根路径先 canonicalize 规范化（去掉 ../ 段，避免 Windows 路径拼接踩坑）。
/// 生成：Config（或缺关键项时的配置错误）。
pub fn load(workspace_root: &Path) -> Result<Config, Error> {
    // 规范化：CARGO_MANIFEST_DIR/.. → 真实根目录（dshr/）
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let env_path = root.join(".env");
    dotenvy::from_path(&env_path).map_err(|e| Error::Config(format!("加载 .env 失败: {e}")))?;

    let harness_root = std::env::var("DSH_HARNESS_ROOT")
        .map_err(|_| Error::Config("DSH_HARNESS_ROOT 未设置".into()))?;
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .map_err(|_| Error::Config("DEEPSEEK_API_KEY 未设置".into()))?;
    let model = std::env::var("DSH_MODEL").map_err(|_| Error::Config("DSH_MODEL 未设置".into()))?;
    let provider =
        std::env::var("DSH_PROVIDER").unwrap_or_else(|_| "deepseek-official".to_string());
    let session_root = std::env::var("DSH_SESSION_ROOT")
        .map_err(|_| Error::Config("DSH_SESSION_ROOT 未设置".into()))?;
    let max_tokens = std::env::var("DSH_MAX_TOKENS")
        .ok()
        .and_then(|s| s.parse().ok());

    // 数据路径：DSH_DATA_DIR 优先，缺省 dshr/data/dshr.db
    let db_path = match std::env::var("DSH_DATA_DIR") {
        Ok(dir) => PathBuf::from(dir).join("dshr.db"),
        Err(_) => {
            let dir = root.join("data");
            std::fs::create_dir_all(&dir)
                .map_err(|e| Error::Config(format!("建 data 目录失败: {e}")))?;
            dir.join("dshr.db")
        }
    };

    Ok(Config {
        db_path,
        harness_root,
        api_key,
        provider,
        model,
        max_tokens,
        session_root,
    })
}
