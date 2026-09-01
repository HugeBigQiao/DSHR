//! 全链路运行（full round）：spawn → initialize → run ×2 → shutdown。
//! 参考旧 dshr-state/tests/full_round.rs 的流程，按正式桌面端写：
//! 独立 DSH_HOME、工作区锁 DSH_CWD、全程记录（app 轨迹 + dsh 线级）。
use std::path::Path;

use dsh_sdk_client::client::{HarnessClient, HarnessSpawnConfig};
use dsh_sdk_protocol::requests::{InitializeParams, SdkPromptContentBlock};

use crate::config::Config;
use crate::record::Recorder;

/// 跑一轮完整对话（两轮 prompt：第一轮懒创建会话，第二轮复用），返回各轮 finalResponse。
pub async fn run_full_round(
    config: &Config,
    workspace: &Path,
    dsh_bin: &Path,
    recorder: &Recorder,
) -> Vec<Option<String>> {
    recorder.app("spawn.start", &serde_json::json!({ "bin": dsh_bin }));

    let harness = HarnessSpawnConfig {
        command: "node".to_string(),
        args: vec![
            dsh_bin.to_string_lossy().into_owned(),
            "--profile".to_string(),
            "sdk".to_string(),
        ],
        current_dir: workspace.to_string_lossy().into_owned(),
        env: vec![
            ("DEEPSEEK_API_KEY".to_string(), config.api_key.clone()),
            (
                "DSH_HOME".to_string(),
                workspace.join("data/dsh-home").to_string_lossy().into_owned(),
            ),
            ("DSH_CWD".to_string(), workspace.to_string_lossy().into_owned()),
            (
                "DSH_SESSION_ROOT".to_string(),
                workspace.join("data/sessions").to_string_lossy().into_owned(),
            ),
        ],
        request_timeout_ms: 30_000,
        dispose_eof_grace_ms: 2_000,
        dispose_kill_grace_ms: 1_000,
        wire_log_path: Some(recorder.wire_log_path()),
    };
    let mut client = HarnessClient::spawn(harness).await.expect("spawn runtime");
    recorder.app("spawn.ok", &serde_json::json!({}));

    let info = client
        .initialize(&InitializeParams {
            cwd: workspace.to_string_lossy().into_owned(),
            provider: config.provider.clone(),
            model: config.model.clone(),
            reasoning_effort: None,
            max_tokens: None,
        })
        .await
        .expect("initialize");
    recorder.app(
        "initialize.ok",
        &serde_json::json!({
            "serverInfo": { "name": info.server_info.name, "version": info.server_info.version },
        }),
    );

    let mut responses = Vec::new();
    // 会话 id 唯一化：复用固定 id 会撞上磁盘持久化日志（DESIGN R7：id 冲突 → error 回合，
    // 实测 turn/end 报 "session already has a persisted log ... id collision"）。
    let session_id = format!("s-{}", now_ms());
    for (i, text) in ["用一句话回答：ping", "好的，那再说一次 ping"]
        .into_iter()
        .enumerate()
    {
        recorder.app("run.start", &serde_json::json!({ "index": i, "prompt": text }));
        let result = client
            .run(&session_id, vec![SdkPromptContentBlock::text(text)], 120_000)
            .await
            .unwrap_or_else(|e| panic!("run#{i} 失败: {e}"));
        recorder.app(
            "run.end",
            &serde_json::json!({
                "index": i,
                "sessionId": result.session_id,
                "finalResponse": result.final_response,
                "events": result.events.len(),
                "notifications": result.notifications.len(),
            }),
        );
        responses.push(result.final_response);
    }

    recorder.app("shutdown.start", &serde_json::json!({}));
    client.shutdown().await.expect("shutdown");
    recorder.app("shutdown.ok", &serde_json::json!({}));
    responses
}

/// 当前 epoch 毫秒（会话 id 唯一化用）。
fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
