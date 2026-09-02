//! bridge 的 worker 部分（原设计 src/bridge/worker.rs；顶层模块化，目录整理留待后续）：
//! 常驻驱动循环（Machine）。总线流（bridge.rs 的 bus）持有本 Machine，命令
//! （Start/Stop/Prompt/ResetSession）与 SDK 通知在同一个 select 循环里处理。
//!
//! 结构：
//! - `next()`：一次 select（命令通道 ∨ SDK 通知广播）→ 0..n 个 BridgeEvent；
//!   命令通道关闭（App 退出）返回 None → 总线流结束。
//! - Start：real.rs 判定 Fake/Real → HarnessClient::spawn + initialize →
//!   新会话（新 session id + Folder 重置 + idle 合成状态）。
//! - Prompt：session/prompt 请求（入队回执即刻返回，不等 agent）→ Fake 模式本地
//!   回显用户行 + 乐观 running 状态；之后 SDK 通知流驱动折叠 → 快照整发 UI。
//!   （不做 client.run() 的 receipt-to-idle 阻塞收圈：UI 是活的事件消费者；
//!   事件在 prompt 期间经 4096 容量广播缓冲，超限 lagging 丢最旧——单会话
//!   低频场景可接受，报告已注明。）
use std::future::Future;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use dsh_sdk_client::client::HarnessClient;
use dsh_sdk_protocol::notifications::SessionStatus;
use dsh_sdk_protocol::requests::{SdkPromptContentBlock, SessionPromptParams};
use dsh_sdk_protocol::rpc::Notification;
use dshr_state::snapshot::SessionSnapshot;
use iced::futures::StreamExt;
use iced::futures::channel::mpsc;
use tokio::sync::broadcast;

use crate::bridge::{BridgeCmd, BridgeEvent};
use crate::real::{self, RealBridge, RuntimeMode};

/// worker 状态机：持有折叠桥 + 客户端 + 会话元信息。
pub struct Machine {
    cmd_rx: mpsc::Receiver<BridgeCmd>,
    /// 折叠桥（Folder 持有；Start/ResetSession 重置、Stop 保留给 UI 继续看）。
    bridge: RealBridge,
    /// SDK 客户端（None = runtime 未启动/已停）。
    client: Option<HarnessClient>,
    /// SDK 事件广播接收端（与 client 同生共死；进程退出时收到 Closed）。
    events: Option<broadcast::Receiver<Notification>>,
    /// 当前会话 id（worker 生成：s-<epoch>。真实 dsh 按 id 落盘日志，必须唯一化，
    /// 见 dshr-state session.rs 注释；Start/ResetSession 换新）。
    session_id: String,
    /// 当前模式（Fake 需本地回显用户行——fake runtime 不发送 user/message）。
    mode: Option<RuntimeMode>,
    /// 上次整发的快照（无变化不重复发，忽略性事件零 UI 开销）。
    last_sent: Option<SessionSnapshot>,
    /// 本地合成事件 seq（回显行用；worker 本地单调即可）。
    seq: u64,
    /// 测试接缝：强制 Fake（headless 集成测试用；生产恒 false）。
    fake_only: bool,
}

impl Machine {
    pub fn new(cmd_rx: mpsc::Receiver<BridgeCmd>) -> Self {
        Self {
            cmd_rx,
            bridge: RealBridge::new(),
            client: None,
            events: None,
            session_id: String::new(),
            mode: None,
            last_sent: None,
            seq: 0,
            fake_only: false,
        }
    }

    /// 测试接缝：强制 Fake 模式（跳过 resolve_mode 的 config.json 判定——测试机
    /// 若配了真 api-key，不能让它去拉真实 dsh）。仅测试调用。
    pub fn force_fake(&mut self) {
        self.fake_only = true;
    }

    /// 一步驱动：select（命令 ∨ 通知）→ 事件批。None = 命令通道关闭（总线结束）。
    pub async fn next(&mut self) -> Option<Vec<BridgeEvent>> {
        type NotifOut = Result<Notification, broadcast::error::RecvError>;
        // 先取两个接收端的借用（字段级拆分借用，select 结束后释放）。
        let cmd_rx = &mut self.cmd_rx;
        let notif: Pin<Box<dyn Future<Output = NotifOut> + Send>> = match &mut self.events {
            Some(rx) => Box::pin(rx.recv()),
            None => Box::pin(iced::futures::future::pending()),
        };
        enum Input {
            Cmd(BridgeCmd),
            Notif(NotifOut),
            End,
        }
        let input = tokio::select! {
            cmd = cmd_rx.next() => match cmd {
                Some(cmd) => Input::Cmd(cmd),
                None => Input::End,
            },
            n = notif => Input::Notif(n),
        };
        match input {
            Input::End => None,
            Input::Cmd(cmd) => Some(self.on_cmd(cmd).await),
            Input::Notif(Ok(n)) => Some(self.on_notification(n)),
            // 消费太慢被广播丢事件：跳过继续（官方 lagging 语义）。
            Input::Notif(Err(broadcast::error::RecvError::Lagged(_))) => Some(vec![]),
            // 广播关闭 = runtime 进程退出（读循环 EOF）。
            Input::Notif(Err(broadcast::error::RecvError::Closed)) => {
                Some(self.teardown("runtime 进程已退出".to_string()))
            }
        }
    }

    async fn on_cmd(&mut self, cmd: BridgeCmd) -> Vec<BridgeEvent> {
        match cmd {
            BridgeCmd::Start => self.start().await,
            BridgeCmd::Stop => self.stop().await,
            BridgeCmd::ResetSession => self.reset_session(),
            BridgeCmd::Prompt { text } => self.prompt(text).await,
        }
    }

    /// 启动 runtime（模式判定在 real::resolve_mode，集中一处）。
    async fn start(&mut self) -> Vec<BridgeEvent> {
        if self.client.is_some() {
            return vec![]; // 已在运行：忽略（重启 = 先 Stop 再 Start）。
        }
        let resolved = if self.fake_only {
            real::ResolvedMode {
                mode: RuntimeMode::Fake,
                note: String::new(),
            }
        } else {
            real::resolve_mode()
        };
        let ws = real::workspace_root();
        let mut kit = match resolved.mode {
            RuntimeMode::Fake => real::kit(RuntimeMode::Fake, &ws).expect("fake 装配不应失败"),
            RuntimeMode::Real => {
                // ensure 可能 pnpm install（网络/分钟级）→ spawn_blocking 免得卡 UI 执行器。
                let ws = ws.clone();
                match tokio::task::spawn_blocking(move || real::kit(RuntimeMode::Real, &ws)).await {
                    Ok(Ok(kit)) => kit,
                    Ok(Err(e)) => return vec![BridgeEvent::Failed { reason: e }],
                    Err(e) => {
                        return vec![BridgeEvent::Failed {
                            reason: format!("装配任务失败：{e}"),
                        }];
                    }
                }
            }
        };
        // 回落说明挂到 label（状态栏可见："Fake runtime（无 config.json）"）。
        if !resolved.note.is_empty() {
            kit.label = format!("{}（{}）", kit.label, resolved.note);
        }
        self.spawn_client(kit).await
    }

    /// spawn + initialize → 就绪（新会话：新 id + Folder 重置 + idle 合成状态）。
    async fn spawn_client(&mut self, kit: real::SpawnKit) -> Vec<BridgeEvent> {
        let label = kit.label.clone();
        let mut client = match HarnessClient::spawn(kit.config).await {
            Ok(c) => c,
            Err(e) => {
                return vec![BridgeEvent::Failed {
                    reason: format!("runtime 启动失败：{e}"),
                }];
            }
        };
        if let Err(e) = client.initialize(&kit.init).await {
            // 进程活着但握手失败（真实 dsh 鉴权/参数错）→ 优雅收尾再报。
            let _ = client.shutdown().await;
            return vec![BridgeEvent::Failed {
                reason: format!("initialize 失败：{e}"),
            }];
        }
        let events_rx = client.take_events(); // 广播接收端归 Machine（读循环在 transport）。
        self.session_id = gen_session_id();
        self.mode = Some(kit.mode);
        self.bridge.reset();
        self.seq = 0;
        self.last_sent = None;
        self.client = Some(client);
        self.events = Some(events_rx);
        // 新会话初始 idle（合成通知走折叠 → 空快照带 session id，UI 清场）。
        self.bridge
            .set_status(&self.session_id, SessionStatus::Idle);
        let mut out = vec![BridgeEvent::Started {
            label,
            session_id: self.session_id.clone(),
        }];
        if let Some(ev) = self.emit_snapshot() {
            out.push(ev);
        }
        out
    }

    /// 停止：协议 shutdown + dispose 阶梯（失败忽略：进程已死则当收尸）。
    /// 折叠态保留 → UI 消息流不丢（下次 Start 才重置）。
    async fn stop(&mut self) -> Vec<BridgeEvent> {
        let Some(client) = self.client.take() else {
            return vec![];
        };
        let _ = client.shutdown().await;
        self.events = None;
        vec![BridgeEvent::Stopped {
            reason: String::new(),
        }]
    }

    /// 会话级异常拆除（进程退出/请求失败）：客户端置空，保留折叠态供查看。
    fn teardown(&mut self, reason: String) -> Vec<BridgeEvent> {
        self.client = None;
        self.events = None;
        vec![BridgeEvent::Failed { reason }]
    }

    /// 重置当前会话（「新建会话/删除会话」语义：Folder 清空 + 新 session id；
    /// 未来多会话管理 = 旧 Folder 归档/落库，这里只清当前）。
    fn reset_session(&mut self) -> Vec<BridgeEvent> {
        if self.client.is_none() {
            return vec![]; // 未启动：忽略。
        }
        self.bridge.reset();
        self.seq = 0;
        self.last_sent = None;
        self.session_id = gen_session_id();
        self.bridge
            .set_status(&self.session_id, SessionStatus::Idle);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 发一条用户消息：session/prompt 入队回执即刻返回，agent 产出走通知流。
    async fn prompt(&mut self, text: String) -> Vec<BridgeEvent> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return vec![];
        }
        if self.client.is_none() {
            return vec![]; // runtime 未启动（App 侧已挡）。
        }
        let session_id = self.session_id.clone();
        let is_fake = self.mode == Some(RuntimeMode::Fake);
        let result = self
            .client
            .as_mut()
            .expect("已判 Some")
            .prompt(&SessionPromptParams {
                session_id: session_id.clone(),
                content_blocks: vec![SdkPromptContentBlock::text(text.clone())],
            })
            .await;
        if let Err(e) = result {
            // 进程侧已挂/协议错 → 拆除并报错（快照保留最后状态）。
            return self.teardown(format!("prompt 发送失败：{e}"));
        }
        // Fake 不回发 user/message → 本地回显用户行（真实 runtime 会自己发，不补防重复）。
        if is_fake {
            self.seq += 1;
            self.bridge
                .push_local_user_message(&text, self.seq, epoch_ms());
        }
        // 乐观 running：真实 dsh 的 session.status 未到前先置 running，idle 到达即回落。
        self.bridge.set_status(&session_id, SessionStatus::Running);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 通知 → 折叠（real.rs feed 按当前会话过滤）→ 变化才整发快照。
    fn on_notification(&mut self, n: Notification) -> Vec<BridgeEvent> {
        if self.client.is_none() {
            return vec![]; // 理论不发生（events 只在运行时存在）。
        }
        self.bridge.feed(&self.session_id, &n);
        match self.emit_snapshot() {
            Some(ev) => vec![ev],
            None => vec![],
        }
    }

    /// 折叠快照整发（与上次相同则跳过；Snapshot 事件带 Box 走 UI 消息）。
    fn emit_snapshot(&mut self) -> Option<BridgeEvent> {
        let snap = self.bridge.snapshot();
        if self.last_sent.as_ref() == Some(&snap) {
            return None;
        }
        self.last_sent = Some(snap.clone());
        Some(BridgeEvent::Snapshot(Box::new(snap)))
    }
}

/// 会话 id 唯一化（epoch ms；真实 dsh 按 id 持久化会话日志，固定 id 会撞冲突）。
fn gen_session_id() -> String {
    format!("s-{}", epoch_ms())
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    //! headless 集成测试：不依赖 GUI，直接驱动 Machine 跑 Fake runtime 全链路——
    //! Start → Started+空快照 → Prompt → 本地回显用户行 → SDK 通知折叠 →
    //! assistant "hello from fake" + usage 入桶 + idle → Stop。s3 胶水层回归。

    use std::time::Duration;

    use dshr_state::snapshot::MsgKind;
    use iced::futures::channel::mpsc;
    use iced::futures::SinkExt;

    use super::*;
    use crate::bridge::BridgeEvent;

    /// 轮询 next() 直到出现满足 expect 的事件（超时 8s 防挂死；已收事件随 panic 打印）。
    async fn poll_until(
        machine: &mut Machine,
        expect: impl Fn(&BridgeEvent) -> bool,
    ) -> Vec<BridgeEvent> {
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if got.iter().any(&expect) {
                return got;
            }
            match tokio::time::timeout(Duration::from_millis(200), machine.next()).await {
                Ok(Some(events)) => got.extend(events),
                Ok(None) => panic!("命令通道提前关闭"),
                Err(_) => {} // 通知未到：继续等
            }
        }
        panic!("等待超时；已收事件：\n{got:#?}");
    }

    /// 发命令 + poll_until。
    async fn drive(
        machine: &mut Machine,
        tx: &mut mpsc::Sender<BridgeCmd>,
        cmd: BridgeCmd,
        expect: impl Fn(&BridgeEvent) -> bool,
    ) -> Vec<BridgeEvent> {
        tx.send(cmd).await.expect("总线未关闭");
        poll_until(machine, expect).await
    }

    fn first_snapshot<'a>(events: &'a [BridgeEvent]) -> Option<&'a dshr_state::snapshot::SessionSnapshot> {
        events.iter().find_map(|e| match e {
            BridgeEvent::Snapshot(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    fn has_user<'a>(s: &'a dshr_state::snapshot::SessionSnapshot, text: &str) -> bool {
        s.messages
            .iter()
            .any(|m| m.kind == MsgKind::User && m.text.trim() == text)
    }

    fn has_fake_reply(s: &dshr_state::snapshot::SessionSnapshot) -> bool {
        s.messages
            .iter()
            .any(|m| m.kind == MsgKind::Assistant && m.text.contains("hello from fake"))
    }

    #[tokio::test]
    async fn fake_runtime_full_round_drives_machine() {
        let (mut tx, rx) = mpsc::channel(crate::bridge::CHANNEL_CAP);
        let mut machine = Machine::new(rx);
        machine.force_fake();

        // 1) Start：Started（Fake label）+ 空会话快照（idle）。
        let evs = drive(&mut machine, &mut tx, BridgeCmd::Start, |e| {
            matches!(e, BridgeEvent::Started { .. })
        })
        .await;
        assert!(
            evs.iter().any(|e| matches!(e, BridgeEvent::Started { label, .. } if label.contains("Fake"))),
            "Started 应带 Fake label"
        );
        let snap = first_snapshot(&evs).expect("Start 后应发空快照");
        assert_eq!(snap.status, Some(SessionStatus::Idle));
        assert!(snap.messages.is_empty());

        // 2) Prompt：命令批内先见本地回显用户行（fake 不回发 user/message）。
        let evs = drive(
            &mut machine,
            &mut tx,
            BridgeCmd::Prompt {
                text: "hi".to_string(),
            },
            |e| matches!(e, BridgeEvent::Snapshot(s) if has_user(s, "hi")),
        )
        .await;
        let snap = first_snapshot(&evs).expect("Prompt 后应发快照");
        assert_eq!(snap.status, Some(SessionStatus::Running));

        // 3) 等 SDK 通知流：assistant "hello from fake" + usage 入桶 + idle 回落。
        let evs = poll_until(&mut machine, |e| {
            matches!(e, BridgeEvent::Snapshot(s) if has_fake_reply(s) && s.status == Some(SessionStatus::Idle))
        })
        .await;
        let snap = first_snapshot(&evs).expect("应收到含回复的快照");
        let reply = snap
            .messages
            .iter()
            .find(|m| m.kind == MsgKind::Assistant)
            .expect("应有一条 assistant 消息");
        assert!(reply.text.contains("hello from fake"), "回复文本：{}", reply.text);
        // fixture 的 assistant/message usage = input 1 / output 2。
        assert_eq!(snap.stats.usage.input, 1);
        assert_eq!(snap.stats.usage.output, 2);
        assert_eq!(snap.stats.messages, 2); // User + Assistant 各一

        // 4) Stop：协议 shutdown → Stopped。
        let evs = drive(&mut machine, &mut tx, BridgeCmd::Stop, |e| {
            matches!(e, BridgeEvent::Stopped { .. })
        })
        .await;
        assert!(evs.iter().any(|e| matches!(e, BridgeEvent::Stopped { .. })));
    }
}
