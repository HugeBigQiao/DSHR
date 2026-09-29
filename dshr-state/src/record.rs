//! 全程记录（WireLog）：一个 JSONL 文件承载「与 dsh 的线级交互」与「本应用轨迹」。
//!
//! 文件里每行一个 JSON 对象，用 `cat` 区分两类：
//! - `cat="dsh"`：与 dsh 的线级交互（请求/响应/通知；`session.event` 细到 `eventType` + 原始 payload）
//! - `cat="app"`：本应用自己的运行轨迹（配置加载 / runtime 安装 / spawn / initialize / run / shutdown 等）
//!
//! 主要用途：**问题复现的唯一 lossless 源**——sqlite 加工库只是结论，出问题时以本文件为准
//! （可离线回放：`fold::Folder` 吃得下 `cat="dsh"` 行，不 spawn runtime、不烧 token）。
//! 为什么需要：分两个文件（一个 dsh 线级、一个 app 轨迹）会让「同一时刻发生了什么」需要
//! 交叉对齐时间戳；合成一个文件后，**时间序天然就是因果序**。
//! 实现要点：SDK 的 `transport` 负责写 `cat="dsh"`（经 `HarnessSpawnConfig.wire_log_path`
//! 指向本文件），本层持**同一个** `WireLog` 句柄写 `cat="app"`——两路互不冲突。
//! 上接：`main.rs`（CLI 入口开一条记录）、`raw.rs`（把路径填进 spawn 配置）。
//! 下接：`dsh_sdk_client::transport::WireLog`（实际写盘者）、文件系统。
//! 官方对应：无（官方的会话日志是 `data/dsh-home/sessions/<workspace>/<id>/session.jsonl.zstd`，
//! 那是**runtime 自己的**事件日志，不含 wire 帧与宿主轨迹；本文件是宿主侧记录，两者互补）。
use std::path::{Path, PathBuf};

use dsh_sdk_client::transport::WireLog;

/// 记录器：持有 SDK 的线级日志句柄，向同一个文件追加 app 记录。
///
/// 为什么需要：`WireLog` 的构造被 SDK 内部持有（一次 open 一个句柄），
/// 而宿主也要往同一文件写——所以这里保存句柄做「二次写入入口」，
/// 而不是各自 open 同一路径（那会互相截断）。
#[derive(Debug)]
pub struct Recorder {
    /// SDK 侧的线级日志句柄（dsh 交互由它写）。
    pub wire_log: WireLog,
    /// 记录文件路径（也是交给 `HarnessSpawnConfig.wire_log_path` 的值）。
    pub path: PathBuf,
}

impl Recorder {
    /// 打开记录文件（父目录需已存在；engine/CLI 负责先建 `data/wire-logs/`）。
    ///
    /// 入参 `path`：JSONL 目标路径。返回：`Recorder`；I/O 失败时返回错误（调用方决定是否致命）。
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let wire_log = WireLog::open(&path.to_string_lossy())?;
        Ok(Self { wire_log, path })
    }

    /// 记录一条 app 事件（非 dsh 交互）。
    ///
    /// 入参 `kind`：事件名（如 `"config.loaded"` / `"spawn.ok"`）；`data`：任意 JSON 摘要。
    /// 为什么需要：把宿主侧的关键节点也留在同一时间线上，便于对齐「当时应用在做什么」。
    pub fn app(&self, kind: &str, data: &serde_json::Value) {
        self.wire_log.record_app(kind, data);
    }

    /// 线级日志路径（给 `HarnessSpawnConfig.wire_log_path`，让 SDK 写 dsh 记录进同一文件）。
    ///
    /// 返回：文件路径字符串。这是「两份记录合成一个文件」的接线点。
    pub fn wire_log_path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

/// 工作区 data 目录（记录文件落盘处）。
///
/// 入参 `workspace`：工作区根。返回：`<workspace>/data`。集中在此以免各调用点各写一次路径拼接。
pub fn data_dir(workspace: &Path) -> PathBuf {
    workspace.join("data")
}
