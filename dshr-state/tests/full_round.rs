//! 完整一轮真实链路测试（dshr-state 作为中间人驱动）。
//!
//! 配置从 `dshr/.env` 读取（dotenvy 自动加载）：
//! - `DEEPSEEK_API_KEY`：真实 key（platform.deepseek.com 申请）
//! - `DSH_HARNESS_ROOT`：官方仓库根（node carrier）
//! - `DSH_CWD` / `DSH_SESSION_ROOT`：工作目录与会话根
//! - `DSH_MODEL` / `DSH_MAX_TOKENS`：模型与输出上限
//!
//! 流程：spawn → initialize（校验 serverInfo）→ prompt（发消息）
//! → 消费事件流直到 turn/end → shutdown。
use std::path::Path;
use std::time::Duration;

use dshr_protocol::content_block::{ContentBlock, TextBlock};
use dshr_protocol::requests::{InitializeParams, SessionPromptParams};
use dshr_runtime::client::{HarnessClient, HarnessSpawnConfig};

#[tokio::test]
async fn full_round() {
    // 显式定位 dshr 根的 .env（cargo test 的 CWD 是包根 dshr-state/，不能依赖 CWD）
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.env");
    dotenvy::from_path(&env_path).expect("dshr/.env 未找到（请创建并填写配置）");

    let harness_root =
        std::env::var("DSH_HARNESS_ROOT").expect("DSH_HARNESS_ROOT 未设置（见 dshr/.env）");
    let cwd = std::env::var("DSH_CWD").expect("DSH_CWD 未设置");
    let session_root = std::env::var("DSH_SESSION_ROOT").expect("DSH_SESSION_ROOT 未设置");
    let api_key =
        std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY 未设置（先在 .env 填入 key）");
    let provider =
        std::env::var("DSH_PROVIDER").unwrap_or_else(|_| "deepseek-official".to_string());
    let model = std::env::var("DSH_MODEL").expect("DSH_MODEL 未设置");
    let max_tokens = std::env::var("DSH_MAX_TOKENS")
        .ok()
        .and_then(|s| s.parse().ok());

    // 1. spawn：node carrier（官方仓库 + tsx + jsonrpc-demo bin + cordis.yml）
    let config = HarnessSpawnConfig {
        command: "node".to_string(),
        args: vec![
            "--import".to_string(),
            "tsx".to_string(),
            "packages/examples/jsonrpc-demo/src/bin.ts".to_string(),
            "examples/jsonrpc-agent/cordis.yml".to_string(),
        ],
        current_dir: harness_root,
        env: vec![
            ("DEEPSEEK_API_KEY".to_string(), api_key),
            ("DSH_CWD".to_string(), cwd.clone()),
            ("DSH_SESSION_ROOT".to_string(), session_root),
        ],
    };
    let mut client = HarnessClient::spawn(config)
        .await
        .expect("spawn runtime 失败");

    // 2. initialize：握手，校验 serverInfo
    let info = client
        .initialize(&InitializeParams {
            cwd,
            provider,
            model,
            max_tokens,
        })
        .await
        .expect("initialize 失败");
    println!(
        "serverInfo: {} v{}",
        info.server_info.name, info.server_info.version
    );
    assert_eq!(
        info.server_info.name, "deepseek-harness-sdk-runtime",
        "serverInfo 名称应与官方一致"
    );

    // 3. prompt：发一条真实消息（session id 用时间戳保证唯一，避免与历史日志冲突）
    let session_id = format!(
        "dshr-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let result = client
        .prompt(&SessionPromptParams {
            session_id: session_id.clone(),
            content_blocks: vec![ContentBlock::Text(TextBlock {
                text: "你好，请用一句话回复。".to_string(),
            })],
        })
        .await
        .expect("prompt 失败");
    println!("messageId: {}", result.message_id);

    // 4. 消费事件流直到 turn/end（120s 超时）
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "等待 turn/end 超时（120s）"
        );
        match tokio::time::timeout(Duration::from_millis(500), client.events().recv()).await {
            Ok(Some(notification)) => {
                if notification.method != "session.event" {
                    println!("[notify] {}", notification.method);
                    continue;
                }
                // 事件本体在 params.event（session.event 信封）
                let event = &notification.params["event"];
                let event_type = event["type"].as_str().unwrap_or("?");
                println!("[event] {event_type}");
                if event_type == "turn/end" {
                    // 断言回合是 completed（不是 error/aborted）
                    assert_eq!(
                        event["data"]["reason"]["kind"].as_str(),
                        Some("completed"),
                        "turn/end 不是 completed（回合失败）：{event}"
                    );
                    break;
                }
            }
            Ok(None) => panic!("事件通道关闭（runtime 提前退出）"),
            Err(_) => continue, // 500ms 无事件，继续等
        }
    }

    // 5. shutdown：优雅关闭
    client.shutdown().await.expect("shutdown 失败");
    println!("✅ 完整一轮跑通");
}
