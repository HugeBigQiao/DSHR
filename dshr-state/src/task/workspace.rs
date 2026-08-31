//! 工作区文件树：Command::ListWorkspace → 后台读目录 → UiEvent::FileTree。
//!
//! 读取在 RuntimeTask 的 async 上下文（tokio::fs，不阻塞 UI 线程）。
//! 无工作区的 runtime 返回空列表（UI 显示"未设置工作区"）。

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::task::bridge::RtInfo;
use crate::task::events::{UiEvent, UiFileEntry};

/// 读工作区目录（path 相对工作区，"" = 根），结果发 FileTree 事件。
/// 接收：runtime 信息 + 相对路径 + 事件通道。
/// 处理：tokio::fs 列目录 → 排序（目录在前）→ 组装 UiFileEntry。
/// 生成：UiEvent::FileTree（无工作区/读取失败时发空列表）。
pub fn list(info: &RtInfo, path: &str, ev_tx: &mpsc::UnboundedSender<UiEvent>) {
    let runtime_id = info.id.clone();
    let path = path.to_string();
    let workspace = info.workspace.clone();
    let ev_tx = ev_tx.clone();
    // 异步任务：目录读取可能慢（网络盘等），不阻塞 RuntimeTask 主循环。
    tokio::spawn(async move {
        let entries = match workspace {
            Some(ws) => read_dir_entries(&ws, &path).await,
            None => Vec::new(),
        };
        let _ = ev_tx.send(UiEvent::FileTree {
            runtime_id,
            path,
            entries,
        });
    });
}

/// 读一个目录的直接子项（目录在前，各自按名字排序）。
async fn read_dir_entries(workspace: &str, rel: &str) -> Vec<UiFileEntry> {
    let root = PathBuf::from(workspace);
    let dir = if rel.is_empty() {
        root.clone()
    } else {
        root.join(rel)
    };
    let mut items = Vec::new();
    // 目录不存在/不可读 → 空列表（UI 兜底显示）。
    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue; // 隐藏文件不进文件树
            }
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            items.push(UiFileEntry { name, is_dir });
        }
    }
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    items
}
