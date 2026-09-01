//! 端到端 smoke：假 runtime 进程（Node 脚本，见 fixtures/fake_runtime.mjs）→
//! spawn → initialize → run()（receipt-to-idle）→ shutdown（dispose 阶梯）。
//! 对应官方先例：packages/sdk/client/tests/sdk-client.spec.ts（fake-runtime 驱动）。

use dsh_sdk_client::client::{HarnessClient, HarnessSpawnConfig};
use dsh_sdk_protocol::requests::{InitializeParams, SdkPromptContentBlock};
use dsh_sdk_protocol::session_event::SessionEvent;

fn fake_runtime_config() -> HarnessSpawnConfig {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_runtime.mjs");
    HarnessSpawnConfig {
        command: "node".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        current_dir: env!("CARGO_MANIFEST_DIR").to_string(),
        env: vec![],
        request_timeout_ms: 5_000,
        dispose_eof_grace_ms: 2_000,
        dispose_kill_grace_ms: 1_000,
        wire_log_path: None,
    }
}

#[tokio::test]
async fn initialize_prompt_shutdown_smoke() {
    let mut client = HarnessClient::spawn(fake_runtime_config())
        .await
        .expect("spawn");
    let info = client
        .initialize(&InitializeParams {
            cwd: ".".to_string(),
            provider: "fake".to_string(),
            model: "fake-model".to_string(),
            reasoning_effort: None,
            max_tokens: None,
        })
        .await
        .expect("initialize");
    assert_eq!(info.server_info.name, "fake-runtime");

    let result = client
        .run("s1", vec![SdkPromptContentBlock::text("hi")], 5_000)
        .await
        .expect("run");
    assert_eq!(result.session_id, "s1");
    // finalResponse = 最后一条根会话助手文本
    assert_eq!(result.final_response.as_deref(), Some("hello from fake"));
    // 事件里应含回执与 assistant/message
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, SessionEvent::AgentInboxSpliced { .. })),
        "应收到 agent/inbox/spliced 回执"
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, SessionEvent::AssistantMessage { .. })),
        "应收到 assistant/message"
    );

    client.shutdown().await.expect("shutdown");
}
