use dshr_runtime::client::{HarnessClient, HarnessSpawnConfig};

#[tokio::test]
async fn init_smoke() {
    let config = HarnessSpawnConfig {
        command: "node".to_string(),
        args: vec![
            "--import".to_string(),
            "tsx".to_string(),
            "packages/examples/jsonrpc-demo/src/bin.ts".to_string(),
            "examples/jsonrpc-agent/cordis.yml".to_string(),
        ],
        current_dir: r"D:\DeepseekHarness\deepseek-harness".to_string(),
        env: vec![
            ("DEEPSEEK_API_KEY".to_string(), "dummy-key".to_string()),
            (
                "DSH_CWD".to_string(),
                r"D:\DeepseekHarness\dshr\.tmp-ws".to_string(),
            ),
            (
                "DSH_SESSION_ROOT".to_string(),
                r"D:\DeepseekHarness\dshr\.tmp-ws\.sessions".to_string(),
            ),
        ],
    };
    let mut client = HarnessClient::spawn(config).await.unwrap();
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"cwd":"D:\\DeepseekHarness\\dshr\\.tmp-ws","provider":"deepseek-official","model":"deepseek-v4-pro","maxTokens":100}}"#;
    let response = client.initialize(req).await.unwrap();
    let prompt_req = r#"{"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":"test-session-1","contentBlocks":[{"type":"text","text":"Hello!"}]}}"#;
    let prompt_response = client.prompt(prompt_req, 2).await.unwrap();
    println!("response: {response}");
    println!("prompt_response: {prompt_response}");
    client.shutdown().await.unwrap();
}
