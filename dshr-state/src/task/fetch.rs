//! dsh 无头运行时下载（决策 23）：`npm install` 到 `workspace/dsh/`。
//!
//! 与官方仓库解耦：装完的 `node_modules` 就是 dsh 本体，spawn 直接
//! `node <bin.js> <cordis.yml>`，不再需要 harness_root + tsx。
//! 首次运行自动触发；配置页可手动「下载/更新」。

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::task::events::UiEvent;

/// 需要安装的包：jsonrpc-demo（无头入口）+ cordis.yml 的全部插件（与 data/cordis.yml 同步）。
/// 版本用 latest：更新 = 重新 npm install 拉最新；版本稳定后可锁 rc 号。
const PACKAGES: &[&str] = &[
    "@deepseek-ai/dsh-sdk-jsonrpc-demo",
    "@deepseek-ai/dsh-sdk-jsonrpc-server",
    "@deepseek-ai/dsh-llm-deepseek",
    "@deepseek-ai/dsh-subprocess-local",
    "@deepseek-ai/dsh-bash-local",
    "@deepseek-ai/dsh-agent-spine-demo",
    "@deepseek-ai/dsh-session-persistence-jsonl",
    "@deepseek-ai/dsh-session-checkpoint-policy",
    "@deepseek-ai/dsh-subagent",
    "@deepseek-ai/dsh-subagent-spawn-in-process",
    "@deepseek-ai/dsh-tool-subagent",
    "@deepseek-ai/dsh-tool-todo",
    "@deepseek-ai/dsh-fs-local",
    "@deepseek-ai/dsh-fs-observation-policy",
    "@deepseek-ai/dsh-tool-fs",
    "@deepseek-ai/dsh-token-meter",
    "@deepseek-ai/dsh-compaction-basic",
];

/// 下载后 spawn 用的入口（node 直接跑，相对 dsh 目录）。
pub const BIN_ENTRY: &str = "node_modules/@deepseek-ai/dsh-sdk-jsonrpc-demo/lib/bin.js";

/// 是否已安装（bin 入口存在即视为就绪）。
pub fn is_installed(dsh_dir: &Path) -> bool {
    dsh_dir.join(BIN_ENTRY).exists()
}

/// 执行下载/更新：建目录 → 写锁版本 package.json → npm install → 校验。
/// 进度逐行经 `FetchProgress` 上报，结束发 `FetchDone`。
/// 接收：dsh 目录 + npm 镜像源（config.json 的 npm_registry，空 = 官方 registry）。
/// 处理：registry 非空时给 npm 加 `--registry=<url>`（如 https://registry.npmmirror.com）。
pub async fn fetch(dsh_dir: PathBuf, registry: &str, ev_tx: mpsc::UnboundedSender<UiEvent>) {
    let mut lines: Vec<String> = Vec::new();
    let mut report = |line: String| {
        lines.push(line.clone());
        let _ = ev_tx.send(UiEvent::FetchProgress(line));
    };
    let finish = |ok: bool, message: String| {
        let _ = ev_tx.send(UiEvent::FetchDone { ok, message });
    };

    report("检测 node 环境…".to_string());
    let node_ver = Command::new("node")
        .arg("--version")
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let node_ok = node_ver.as_deref().is_some_and(|v| v.starts_with('v'));
    if !node_ok {
        finish(
            false,
            "未找到 node（dsh 需要 Node.js ≥ 22.19，请先安装并加入 PATH）".to_string(),
        );
        return;
    }
    report(format!("node {}\n", node_ver.unwrap_or_default()));

    // 建目录 + 写依赖清单（锁版本文件，更新 = 改版本重装）。
    if let Err(e) = std::fs::create_dir_all(&dsh_dir) {
        finish(false, format!("创建 dsh 目录失败: {e}"));
        return;
    }
    let manifest = serde_json::json!({
        "name": "dshr-dsh-runtime",
        "private": true,
        "type": "module",
        "dependencies": PACKAGES
            .iter()
            .map(|p| {
                (
                    p.to_string(),
                    serde_json::Value::String("latest".to_string()),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    });
    let manifest_text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    if let Err(e) = std::fs::write(dsh_dir.join("package.json"), manifest_text) {
        finish(false, format!("写 package.json 失败: {e}"));
        return;
    }

    report("npm install（拉取 dsh 运行时，视网络而定）…".to_string());
    if !registry.is_empty() {
        report(format!("镜像源：{registry}"));
    }
    let mut child = match Command::new("npm")
        .arg("install")
        .args(["--no-audit", "--no-fund", "--no-progress"])
        .args((!registry.is_empty()).then(|| format!("--registry={registry}")))
        .current_dir(&dsh_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            finish(false, format!("启动 npm 失败: {e}"));
            return;
        }
    };

    // 逐行读 npm 输出 → 进度（只保留最近 N 行，弹窗不至于刷屏）。
    let stdout = child.stdout.take();
    if let Some(out) = stdout {
        let tx = ev_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(out).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let line = line.trim().to_string();
                if !line.is_empty() {
                    let _ = tx.send(UiEvent::FetchProgress(line));
                }
            }
        });
    }
    let status = child.wait().await;

    match status {
        Ok(s) if s.success() => {
            if is_installed(&dsh_dir) {
                finish(true, "dsh 运行时下载完成 ✓".to_string());
            } else {
                finish(
                    false,
                    "npm 成功但未找到 bin 入口（包结构变化？）".to_string(),
                );
            }
        }
        Ok(s) => finish(false, format!("npm install 失败（exit {s}）")),
        Err(e) => finish(false, format!("npm 进程异常: {e}")),
    }
}
