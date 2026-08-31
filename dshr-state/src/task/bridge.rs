//! runtime 对接层：把 dshr-runtime 的 HarnessClient 包一层（任务页专用）。
//!
//! 对上层（Engine/RuntimeTask）只暴露这里定义的数据格式（RtInfo/SendOutcome），
//! 上层不直接看到 dshr-runtime 的类型——换 runtime 实现时只改本文件。

use dshr_runtime::client::{HarnessClient, HarnessSpawnConfig};
use tokio::sync::mpsc;

use crate::Error;
use dshr_protocol::rpc::Notification;

/// 一个已启动 runtime 的对接信息（state 生成 id，落库 runtimes 表也用这份）。
#[derive(Debug, Clone)]
pub struct RtInfo {
    /// state 生成的唯一 id（uuid）。
    pub id: String,
    /// 显示名（用户可改）。
    pub name: String,
    /// 可执行文件（阶段 A 是 "node"）。
    pub command: String,
    /// node carrier 的启动参数（tsx + bin + cordis.yml）。
    pub args: Vec<String>,
    /// 进程工作目录（= 官方仓库根，node 要在这跑）。
    pub process_dir: String,
    /// 工作区（用户选的；None = 尚未设置，设置后锁死，决策 21）。
    pub workspace: Option<String>,
    /// 启动时间（epoch ms）。
    pub created_at: i64,
}

/// 一次 prompt 的结算信息（请求计量，写 requests 表）。
#[derive(Debug, Clone)]
pub struct SendOutcome {
    /// runtime 回执的 messageId。
    pub message_id: String,
    /// 请求耗时（ms，state 侧 await 前后计时）。
    pub duration_ms: u64,
}

/// 对接层本体：持有 HarnessClient，向上层暴露类型化方法。
#[derive(Debug)]
pub struct Bridge {
    pub info: RtInfo,
    client: HarnessClient,
}

impl Bridge {
    /// 拉起进程（spawn）。
    /// 接收：RtInfo + 外部配置（api key / session root）。
    /// 处理：组 HarnessSpawnConfig（进程目录 = process_dir，工作区进 DSH_CWD env）→ HarnessClient::spawn。
    /// 生成：Bridge（持有进程 + 管道）。
    pub async fn spawn(info: RtInfo, api_key: &str, session_root: &str) -> Result<Self, Error> {
        let mut env = vec![
            ("DEEPSEEK_API_KEY".to_string(), api_key.to_string()),
            ("DSH_SESSION_ROOT".to_string(), session_root.to_string()),
        ];
        // 工作区可选：有则锁进 DSH_CWD；无则交给 dsh 兜底（process.cwd()）。
        if let Some(ws) = &info.workspace {
            env.push(("DSH_CWD".to_string(), ws.clone()));
        }
        let client = HarnessClient::spawn(HarnessSpawnConfig {
            command: info.command.clone(),
            args: info.args.clone(),
            current_dir: info.process_dir.clone(),
            env,
        })
        .await?;
        Ok(Self { info, client })
    }

    /// initialize 握手（幂等，官方 server.ts 每次调用都会更新 cwd —— 补设工作区靠它）。
    /// 接收：provider/model/maxTokens + 工作区（None 时传 ""，dsh 用 process.cwd() 兜底）。
    /// 处理：序列化 → 发送 → 校验 serverInfo。
    /// 生成：serverInfo（name/version）。
    pub async fn initialize(
        &mut self,
        provider: &str,
        model: &str,
        max_tokens: Option<u64>,
    ) -> Result<(String, String), Error> {
        use dshr_protocol::requests::InitializeParams;
        let cwd = self.info.workspace.clone().unwrap_or_default();
        let result = self
            .client
            .initialize(&InitializeParams {
                cwd,
                provider: provider.to_string(),
                model: model.to_string(),
                max_tokens,
            })
            .await?;
        Ok((result.server_info.name, result.server_info.version))
    }

    /// 发一条消息（请求计量在 core 层做：这里只返回回执 + 不涉及计时）。
    pub async fn prompt(&mut self, session_id: &str, text: &str) -> Result<SendOutcome, Error> {
        use dshr_protocol::content_block::{ContentBlock, TextBlock};
        use dshr_protocol::requests::SessionPromptParams;
        let result = self
            .client
            .prompt(&SessionPromptParams {
                session_id: session_id.to_string(),
                content_blocks: vec![ContentBlock::Text(TextBlock {
                    text: text.to_string(),
                })],
            })
            .await?;
        Ok(SendOutcome {
            message_id: result.message_id,
            duration_ms: 0, // TODO(3f)：core 层 await 前后计时后回填
        })
    }

    /// 事件流接收端（透传，move 所有权供后台任务 select）。
    pub fn take_events(&mut self) -> mpsc::UnboundedReceiver<Notification> {
        self.client.take_events()
    }

    /// stderr 行接收端（透传，move 所有权供后台任务 select）。
    pub fn take_stderr(&mut self) -> mpsc::UnboundedReceiver<String> {
        self.client.take_stderr()
    }

    /// 关闭（shutdown + kill）。消费 self。
    pub async fn shutdown(self) -> Result<(), Error> {
        self.client.shutdown().await?;
        Ok(())
    }
}
