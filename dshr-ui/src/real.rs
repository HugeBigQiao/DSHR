//! bridge 的 real 部分（原设计 src/bridge/real.rs；顶层模块化，目录整理留待后续）：
//! Fake/Real runtime 判定 + spawn/initialize 装配 + 折叠桥（持有 Folder）。
//!
//! 运行模式（判定集中在本文件 resolve_mode，注释清楚语义）：
//! - `Fake`（开发/默认路径）：node 直跑 dsh-sdk-client 测试用 fake runtime
//!   （tests/fixtures/fake_runtime.mjs，回显 "hello from fake"），无需 API key。
//! - `Real`（真实 dsh 分支）：workspace 根 config.json 存在且 api-key 非空时才走；
//!   provider/model/api-key/dsh-version 来自 dshr-state::config::load，dsh 本体经
//!   dshr-state::runtime::ensure（dsh/ 目录锁版本安装）拿 bin，env/initialize 对齐
//!   dshr-state session.rs 的全链路语义。config.json 缺失/畸形/api-key 空 → 自动
//!   回落 Fake（label 里说明原因，UI 状态行可见）。
use std::path::{Path, PathBuf};

use dsh_sdk_client::client::HarnessSpawnConfig;
use dsh_sdk_protocol::notifications::{self, Kind, SessionStatus, SessionStatusNotification};
use dsh_sdk_protocol::requests::InitializeParams;
use dsh_sdk_protocol::rpc::Notification;
use dsh_sdk_protocol::session_event::SessionEvent;
use dshr_state::fold::Folder;
use dshr_state::runtime;

/// workspace 根（= dshr-ui 的上一级；config.json / dsh/ 都在这里）。
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dshr-ui 应在 workspace 成员目录")
        .to_path_buf()
}

/// 运行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// node 直跑 fake runtime（无 API key 的开发路径）。
    Fake,
    /// 真实 dsh（config.json 配 provider/model/api-key/dsh-version + runtime ensure）。
    Real,
}

/// 判定结果：模式 + 回落说明（Fake 的 label 后缀 / 无说明）。
#[derive(Debug)]
pub struct ResolvedMode {
    pub mode: RuntimeMode,
    /// Fake 时的回落原因（"无 config.json" / "未配置 api-key" 等）。
    pub note: String,
}

/// 模式判定（集中点）：仅当 workspace 根 config.json 存在且 api-key 非空 → Real；
/// 否则自动回落 Fake。软判定：文件缺失/解析失败一律回落 Fake（开发机无 key 的常态）。
pub fn resolve_mode() -> ResolvedMode {
    let path = workspace_root().join("config.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            return ResolvedMode {
                mode: RuntimeMode::Fake,
                note: "无 config.json".to_string(),
            };
        }
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => match v.get("api-key").and_then(serde_json::Value::as_str) {
            Some(key) if !key.trim().is_empty() => ResolvedMode {
                mode: RuntimeMode::Real,
                note: String::new(),
            },
            _ => ResolvedMode {
                mode: RuntimeMode::Fake,
                note: "config.json 未配置 api-key".to_string(),
            },
        },
        Err(_) => ResolvedMode {
            mode: RuntimeMode::Fake,
            note: "config.json 解析失败".to_string(),
        },
    }
}

/// spawn 装配结果：进程配置 + initialize 参数 + 侧边栏 label。
#[derive(Debug)]
pub struct SpawnKit {
    pub mode: RuntimeMode,
    pub label: String,
    pub config: HarnessSpawnConfig,
    pub init: InitializeParams,
}

/// 按判定模式取装配（resolve_mode 已保证 Real 分支的 config.json 可解析）。
/// Real 的 `runtime::ensure` 可能跑 pnpm install（网络/分钟级）——同步阻塞，
/// 调用方（worker）须用 tokio::task::spawn_blocking 包裹。
pub fn kit(mode: RuntimeMode, ws: &Path) -> Result<SpawnKit, String> {
    match mode {
        RuntimeMode::Fake => Ok(fake_kit(ws)),
        RuntimeMode::Real => real_kit(ws),
    }
}

/// Fake：node 跑 dsh-sdk-client 的 fake_runtime.mjs（进程存活期长，按行读 stdin）。
fn fake_kit(ws: &Path) -> SpawnKit {
    // env!("CARGO_MANIFEST_DIR") 编译期绝对路径 → 运行时可稳定找到 fixture。
    let sdk_dir = ws.join("dsh-sdk-client");
    let script = sdk_dir.join("tests/fixtures/fake_runtime.mjs");
    SpawnKit {
        mode: RuntimeMode::Fake,
        label: "Fake runtime".to_string(),
        config: HarnessSpawnConfig {
            command: "node".to_string(),
            args: vec![script.to_string_lossy().into_owned()],
            current_dir: sdk_dir.to_string_lossy().into_owned(),
            env: vec![],
            request_timeout_ms: 5_000,
            dispose_eof_grace_ms: 2_000,
            dispose_kill_grace_ms: 1_000,
            wire_log_path: None, // 全程记录（s2 recorder）接线时再开。
        },
        init: InitializeParams {
            cwd: ws.to_string_lossy().into_owned(),
            provider: "fake".to_string(),
            model: "fake-model".to_string(),
            reasoning_effort: None,
            max_tokens: None,
        },
    }
}

/// Real：config.json → dsh 本体 ensure → spawn（env/initialize 对齐 dshr-state
/// session.rs 全链路：DSH_HOME 独立、DSH_CWD=workspace、DSH_SESSION_ROOT 数据目录）。
fn real_kit(ws: &Path) -> Result<SpawnKit, String> {
    let cfg = dshr_state::config::load(&ws.join("config.json"));
    // dsh/ 目录锁版本安装（runtime.rs ensure：缺 bin 时 pnpm install）。
    let dsh_dir = ws.join("dsh");
    let bin = runtime::ensure(&dsh_dir, &cfg.dsh_version);
    Ok(SpawnKit {
        mode: RuntimeMode::Real,
        label: format!("dsh runtime · {}", cfg.model),
        config: HarnessSpawnConfig {
            command: "node".to_string(),
            args: vec![
                bin.to_string_lossy().into_owned(),
                "--profile".to_string(),
                "sdk".to_string(),
            ],
            current_dir: ws.to_string_lossy().into_owned(),
            env: vec![
                ("DEEPSEEK_API_KEY".to_string(), cfg.api_key.clone()),
                (
                    "DSH_HOME".to_string(),
                    ws.join("data/dsh-home").to_string_lossy().into_owned(),
                ),
                ("DSH_CWD".to_string(), ws.to_string_lossy().into_owned()),
                (
                    "DSH_SESSION_ROOT".to_string(),
                    ws.join("data/sessions").to_string_lossy().into_owned(),
                ),
            ],
            request_timeout_ms: 30_000,
            dispose_eof_grace_ms: 2_000,
            dispose_kill_grace_ms: 1_000,
            wire_log_path: None, // 同上：s2 recorder 接线时开。
        },
        init: InitializeParams {
            cwd: ws.to_string_lossy().into_owned(),
            provider: cfg.provider.clone(),
            model: cfg.model.clone(),
            reasoning_effort: None,
            max_tokens: None,
        },
    })
}

/// 折叠桥：单会话 Folder 持有者 + wire 通知喂入。
///
/// s3 简化：一次只折叠「一个当前会话」（多 runtime/多会话目录与血缘 = s4，
/// 届时按 session_id 各持一个 Folder 即可，折叠语义不变）。Start/ResetSession
/// 时 reset()（换新 session id），Stop 不清（UI 保留已收消息）。
#[derive(Debug, Default)]
pub struct RealBridge {
    pub folder: Folder,
}

impl RealBridge {
    pub fn new() -> Self {
        Self {
            folder: Folder::new(),
        }
    }

    /// 清空折叠态（新会话/新 runtime）。
    pub fn reset(&mut self) {
        self.folder = Folder::new();
    }

    /// 喂一条 SDK 通知（wire 帧 → notifications::parse → Folder::push_notification）。
    /// 只折 `session_id` 对应会话的通知：session.event/session.status 先按
    /// params.sessionId 过滤（子代理血缘会话 s3 不显示，直接跳过）；
    /// subagent.* 等其余通知 Folder 侧本身忽略。
    pub fn feed(&mut self, session_id: &str, n: &Notification) {
        if n.method == "session.event" || n.method == "session.status" {
            let sid = n
                .params
                .get("sessionId")
                .and_then(serde_json::Value::as_str);
            if sid != Some(session_id) {
                return;
            }
        }
        match notifications::parse(n) {
            Ok(Some(kind)) => self.folder.push_notification(&kind),
            // Ok(None)：未知通知方法（协议演进跳过）；Err：内容畸形（跳过，wire 保真留 recorder）。
            _ => {}
        }
    }

    /// 直接置会话状态（worker 用于 prompt 后乐观 running / 新会话 idle）。
    /// 走与 wire 同一条折叠路径（合成一条 session.status 通知）。
    pub fn set_status(&mut self, session_id: &str, status: SessionStatus) {
        self.folder
            .push_notification(&Kind::SessionStatus(SessionStatusNotification {
                session_id: session_id.to_string(),
                status,
            }));
    }

    /// 本地补一条用户消息行（fake runtime 不发送 user/message——回显用户自己的
    /// prompt；真实 runtime 会自己发 user/message，不补、防重复）。
    /// 走官方 wire 形状 JSON → SessionEvent → push_event（与 fold.rs 测试同构）。
    pub fn push_local_user_message(&mut self, text: &str, seq: u64, time: u64) {
        let ev: SessionEvent = serde_json::from_value(serde_json::json!({
            "type": "user/message", "seq": seq, "time": time,
            "data": {
                "id": format!("m-ui-{seq}"),
                "role": "user",
                "content": [{ "type": "text", "text": text }],
                "source": { "kind": "user" },
            },
        }))
        .expect("官方 user/message 形状应可解析（fold.rs 测试同构）");
        self.folder.push_event(&ev);
    }

    /// 当前快照（每次调用重建，轻量）。
    pub fn snapshot(&self) -> dshr_state::snapshot::SessionSnapshot {
        self.folder.snapshot()
    }
}
