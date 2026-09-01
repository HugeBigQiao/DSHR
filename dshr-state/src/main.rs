//! dshr-state 可执行入口：全链路运行 + 全程记录 + 汇总。
//!
//! 用法：`cargo run -p dshr-state [config.json 路径] [日志目录]`
//! 产出：`<日志目录>/wire-<ts>.jsonl`——一个 JSONL 承载全部记录：
//!   `cat="dsh"`  与 dsh 的线级交互（请求/响应/通知；session.event 细到 eventType）
//!   `cat="app"`  本应用的运行轨迹
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dshr_state::config;
use dshr_state::record::{data_dir, Recorder};
use dshr_state::runtime;
use dshr_state::session;

/// workspace 根 = 本 crate 的父目录（D:\dsh\dshr）。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace 根")
        .to_path_buf()
}

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string()
}

/// 汇总记录文件：dsh 交互（发出方法/响应/通知按 method、session.event 按事件类型）
/// + app 轨迹（按 kind 序列）。
fn summarize(log_path: &Path) {
    let text = std::fs::read_to_string(log_path).expect("读记录文件");
    let mut sent: Vec<String> = Vec::new();
    let mut resp_count = 0usize;
    let mut resp_errors = 0usize;
    let mut notif_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut event_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut app_kinds: Vec<(String, String)> = Vec::new(); // (kind, 摘要)
    let mut unparseable = 0usize;

    for line in text.lines() {
        let rec: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("记录行坏: {e}"));
        match rec["cat"].as_str() {
            Some("dsh") => match rec["kind"].as_str() {
                Some("request") => sent.push(rec["method"].as_str().unwrap_or("?").to_string()),
                Some("response") => {
                    resp_count += 1;
                    if rec["raw"]["error"].is_object() {
                        resp_errors += 1;
                    }
                }
                Some("notification") => {
                    let method = rec["method"].as_str().unwrap_or("?").to_string();
                    *notif_counts.entry(method.clone()).or_insert(0) += 1;
                    if method == "session.event" {
                        let ty = rec["eventType"].as_str().unwrap_or("?").to_string();
                        *event_counts.entry(ty).or_insert(0) += 1;
                    }
                }
                _ => unparseable += 1,
            },
            Some("app") => {
                let kind = rec["kind"].as_str().unwrap_or("?").to_string();
                let summary = rec["data"].to_string();
                app_kinds.push((kind, summary));
            }
            _ => unparseable += 1,
        }
    }

    println!("\n===== 记录汇总: {} =====", log_path.display());
    println!("== app 运行轨迹（cat=app）==");
    for (kind, data) in &app_kinds {
        println!("  {kind}  {data}");
    }
    println!("== 发出请求（cat=dsh, send/request，共 {}）==", sent.len());
    for m in &sent {
        println!("  {m}");
    }
    println!("== 收到响应（共 {resp_count}，其中 error {resp_errors}）==");
    println!("== 收到通知（按 method）==");
    for (m, c) in &notif_counts {
        println!("  {m} × {c}");
    }
    println!("== session.event 事件类型（共 {} 种）==", event_counts.len());
    for (t, c) in &event_counts {
        println!("  {t} × {c}");
    }
    if unparseable > 0 {
        println!("== 无法分类 × {unparseable} ==");
    }
    println!("原始 JSONL 已落盘：{}", log_path.display());
}

#[tokio::main]
async fn main() {
    let root = workspace_root();
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("config.json"));
    let log_dir = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir(&root).join("wire-logs"));

    println!("== workspace root: {}", root.display());
    let cfg = config::load(&config_path);
    std::fs::create_dir_all(&log_dir).expect("建日志目录");
    let log_path = log_dir.join(format!("wire-{}.jsonl", now_ts()));
    let recorder = Recorder::open(log_path.clone()).expect("打开记录文件");
    println!("== 记录文件: {}", log_path.display());

    recorder.app(
        "config.loaded",
        &serde_json::json!({
            "provider": cfg.provider, "model": cfg.model, "dshVersion": cfg.dsh_version,
        }),
    );
    // node 环境检查 → dsh 本体（workspace/dsh，运行时下载）。
    runtime::ensure_node().expect("node 环境检查失败");
    let dsh_bin = runtime::ensure(&root.join("dsh"), &cfg.dsh_version);
    recorder.app("runtime.ready", &serde_json::json!({ "bin": dsh_bin }));

    let responses = session::run_full_round(&cfg, &root, &dsh_bin, &recorder).await;
    for (i, r) in responses.iter().enumerate() {
        println!("== run#{i} finalResponse: {r:?}");
    }
    summarize(&log_path);
}
