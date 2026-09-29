//! 工作区文件访问：Files 页的目录列举与读写（**仅限工作区内相对路径**）。
//!
//! 主要用途：`dshr-ui` 的 Files 页经此列出目录、打开文件、保存编辑。
//! 为什么需要（而不是让 UI 直接 `std::fs`）：这是**安全边界**——绝对路径与 `..` 组件一律拒绝，
//! 每个解析后的路径都规范化并与工作区根比对后才使用。把这条约束收在一处，
//! UI 就不可能因为某个入口忘记校验而读写到工作区之外。
//! 上接：`dshr-ui/src/files.rs`（目录树与编辑器）。
//! 下接：`raw::workspace_root`（工作区根的唯一定义处）、文件系统。
//! 官方对应：无（官方对应能力在 `packages/api/workspace-files`，经 HTTP API 暴露给 Web 前端；
//! dshr 是桌面端，直接走本地文件系统，因此需要自己的路径夹紧逻辑）。
use std::path::{Component, Path, PathBuf};

/// [`list_dir`] 返回的一个直接子项。
///
/// 为什么需要：UI 需要「显示名」与「可用于后续操作的工作区相对路径」两个字段——
/// 前者是叶子名，后者是 `/` 分隔的相对路径（用于再列举/打开）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Display name (leaf).
    pub name: String,
    /// Workspace-relative path using `/` separators.
    pub path: String,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// File size in bytes; `0` for directories.
    pub size: u64,
}

/// Names ignored by the first Files-page implementation.
const IGNORED: &[&str] = &[".git", "target", "node_modules", "data", "dsh"];

/// Maximum text file size loaded by the viewer, in bytes.
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

/// Absolute dshr workspace root.
pub fn root() -> PathBuf {
    crate::raw::workspace_root()
}

fn relative_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn ensure_relative(rel: &str) -> Result<(), String> {
    if rel.is_empty() {
        return Ok(());
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err("path must be relative to the workspace root".to_string());
    }
    for component in path.components() {
        match component {
            Component::ParentDir => return Err("path must not contain ..".to_string()),
            Component::Prefix(_) | Component::RootDir => {
                return Err("path must not be absolute".to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve(rel: &str) -> Result<PathBuf, String> {
    ensure_relative(rel)?;
    let root = root();
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("workspace root unavailable: {error}"))?;
    let joined = canonical_root.join(rel);
    let canonical = joined
        .canonicalize()
        .map_err(|error| format!("path does not exist or is not accessible: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("path escapes the workspace root".to_string());
    }
    Ok(canonical)
}

/// List one directory under the workspace root. `rel` may be empty for the root.
pub fn list_dir(rel: &str) -> Result<Vec<FileEntry>, String> {
    let root = root();
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("workspace root unavailable: {error}"))?;
    let dir = resolve(rel)?;
    if !dir.is_dir() {
        return Err(format!("not a directory: {rel}"));
    }
    let mut entries = Vec::new();
    let read = std::fs::read_dir(&dir).map_err(|error| format!("read dir failed: {error}"))?;
    for item in read {
        let item = item.map_err(|error| format!("read dir entry failed: {error}"))?;
        let name = item.file_name().to_string_lossy().into_owned();
        if IGNORED.contains(&name.as_str()) {
            continue;
        }
        let path = item.path();
        let metadata = item
            .metadata()
            .map_err(|error| format!("read metadata failed for {name}: {error}"))?;
        let is_dir = metadata.is_dir();
        entries.push(FileEntry {
            name,
            path: relative_string(&canonical_root, &path),
            is_dir,
            size: if is_dir { 0 } else { metadata.len() },
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

/// Read one UTF-8 text file under the workspace root.
pub fn read_text_file(rel: &str) -> Result<String, String> {
    let path = resolve(rel)?;
    let metadata =
        std::fs::metadata(&path).map_err(|error| format!("read metadata failed: {error}"))?;
    if !metadata.is_file() {
        return Err("not a regular file".to_string());
    }
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "file too large ({} bytes, limit {} bytes)",
            metadata.len(),
            MAX_TEXT_BYTES
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("read file failed: {error}"))?;
    String::from_utf8(bytes).map_err(|_| "file is not UTF-8 text".to_string())
}

/// Overwrite one existing UTF-8 text file under the workspace root.
pub fn write_text_file(rel: &str, content: &str) -> Result<(), String> {
    let path = resolve(rel)?;
    let metadata =
        std::fs::metadata(&path).map_err(|error| format!("read metadata failed: {error}"))?;
    if !metadata.is_file() {
        return Err("only existing regular files can be saved".to_string());
    }
    if content.len() as u64 > MAX_TEXT_BYTES {
        return Err(format!(
            "content too large (limit {} bytes)",
            MAX_TEXT_BYTES
        ));
    }
    std::fs::write(path, content).map_err(|error| format!("write file failed: {error}"))
}
