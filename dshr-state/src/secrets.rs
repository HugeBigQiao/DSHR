//! 本地密钥：`data/secrets.json`（与 config.json 分离，Unix 下 0600）。
//!
//! 兼容旧版把 `api-key` 写在 workspace/config.json 的方式：首次读取时自动迁移到
//! `data/secrets.json`，并从 config.json 删除 `api-key` 字段。空值视为未配置。
use std::io;
use std::path::{Path, PathBuf};

/// 相对 workspace 的密钥文件路径。
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "dshr-secrets-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_load_and_clear_roundtrip() {
        let dir = temp_workspace("roundtrip");
        save_api_key(&dir, "  sk-test  ").unwrap();
        assert_eq!(load_api_key(&dir).as_deref(), Some("sk-test"));
        save_api_key(&dir, "   ").unwrap();
        assert_eq!(load_api_key(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_legacy_config_key_and_removes_it() {
        let dir = temp_workspace("migrate");
        std::fs::write(
            dir.join("config.json"),
            r#"{ "api-key": "sk-legacy", "provider": "deepseek-official" }"#,
        )
        .unwrap();
        assert_eq!(load_api_key(&dir).as_deref(), Some("sk-legacy"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap())
                .unwrap();
        assert!(config.get("api-key").is_none());
        assert_eq!(
            config.get("provider").and_then(serde_json::Value::as_str),
            Some("deepseek-official")
        );
        assert_eq!(load_api_key(&dir).as_deref(), Some("sk-legacy"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
