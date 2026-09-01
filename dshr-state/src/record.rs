//! 全程记录：一个 JSONL 文件（data/ 下），记录分成两类——
//!   `cat="dsh"`：与 dsh 的线级交互（请求/响应/通知；session.event 细到 eventType + 原始 payload）
//!   `cat="app"`：本应用自己的运行轨迹（配置/安装/spawn/initialize/run/shutdown 等）
//!
//! SDK 的 transport 写 dsh 记录（经 `HarnessSpawnConfig.wire_log_path` 指向同一文件），
//! 本层持同一个 WireLog 句柄写 app 记录——一个文件承载全部。
use std::path::{Path, PathBuf};

use dsh_sdk_client::transport::WireLog;

/// 记录器：持有 SDK 的线级日志句柄，向同一个文件追加 app 记录。
#[derive(Debug)]
pub struct Recorder {
    pub wire_log: WireLog,
    pub path: PathBuf,
}

impl Recorder {
    /// 打开（父目录需已存在）。
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let wire_log = WireLog::open(&path.to_string_lossy())?;
        Ok(Self { wire_log, path })
    }

    /// 记录一条 app 事件（非 dsh 交互）。
    pub fn app(&self, kind: &str, data: &serde_json::Value) {
        self.wire_log.record_app(kind, data);
    }

    /// 线级日志路径（给 `HarnessSpawnConfig.wire_log_path`，让 SDK 写 dsh 记录进同一文件）。
    pub fn wire_log_path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// 工作区 data 目录（记录文件落盘处）。
pub fn data_dir(workspace: &Path) -> PathBuf {
    workspace.join("data")
}
