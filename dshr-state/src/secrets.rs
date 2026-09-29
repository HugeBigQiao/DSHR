//! 本地密钥：`data/secrets.json`（与 `config.json` 分离，Unix 下 0600）。
//!
//! 主要用途：API key 的唯一读写入口——`raw/mode.rs` 在 Real 模式下取它填进 runtime 的环境变量；
//! UI 设置页用它读写（保存/清除）。
//! 为什么需要（与 config 分开）：`config.json` 是可提交风格的产品配置，而 key 是凭据。
//! 分开后 key 可以单独设文件权限（0600）、单独 gitignore，且**不会被 UI 的"保存配置"顺手带出去**。
//! 兼容：旧版把 `api-key` 写在 `workspace/config.json`，首次读取时自动迁移到本文件并从
//! `config.json` 删除该字段。空值（`""`/全空白）一律视为未配置。
//! 上接：`config.rs`（`load` 时读取并迁移）、`raw/mode.rs`（Real 模式取用）、`dshr-ui` 设置页。
//! 下接：文件系统（读写 `data/secrets.json`）。
//! 官方对应：无（官方把凭据放在自己的 `dsh-home` 内，dshr 不碰用户的 `~/.dsh`，故自管一份）。
use std::io;
use std::path::{Path, PathBuf};

/// 相对 workspace 的密钥文件路径。
///
/// 入参 `workspace`：工作区根。返回：`<workspace>/data/secrets.json`。
/// 为什么需要：路径拼接只写一处，避免 config/UI 各自拼出不同路径。
pub fn secrets_path(workspace: &Path) -> PathBuf {
    workspace.join("data").join("secrets.json")
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn read_secrets(workspace: &Path) -> Option<String> {
    let text = std::fs::read_to_string(secrets_path(workspace)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    non_empty(
        value
            .get("api-key")
            .or_else(|| value.get("apiKey"))
            .and_then(serde_json::Value::as_str),
    )
}

/// 读取 API key；优先 `data/secrets.json`，缺失时尝试迁移旧 `config.json`。
pub fn load_api_key(workspace: &Path) -> Option<String> {
    if let Some(key) = read_secrets(workspace) {
        return Some(key);
    }
    migrate_legacy(workspace).ok().flatten()
}

/// 写入 API key；空字符串等价于删除本地密钥文件。
pub fn save_api_key(workspace: &Path, key: &str) -> io::Result<()> {
    let path = secrets_path(workspace);
    if key.trim().is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let value = serde_json::json!({ "api-key": key.trim() });
    let mut text = serde_json::to_string_pretty(&value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    text.push('\n');
    std::fs::write(&path, text)?;
    set_owner_only(&path)?;
    Ok(())
}

/// Unix 下把文件权限收紧到 0600；其他平台暂不改变 ACL。
#[cfg(unix)]
fn set_owner_only(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// 把旧 config.json 的 `api-key` 迁到 secrets.json，并从 config.json 删除该字段。
fn migrate_legacy(workspace: &Path) -> io::Result<Option<String>> {
    let config_path = workspace.join("config.json");
    let text = std::fs::read_to_string(&config_path)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let key = non_empty(
        value
            .get("api-key")
            .or_else(|| value.get("apiKey"))
            .and_then(serde_json::Value::as_str),
    );
    let Some(key) = key else {
        return Ok(None);
    };
    save_api_key(workspace, &key)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("api-key");
        object.remove("apiKey");
    }
    let mut rewritten = serde_json::to_string_pretty(&value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    rewritten.push('\n');
    std::fs::write(config_path, rewritten)?;
    Ok(Some(key))
}
