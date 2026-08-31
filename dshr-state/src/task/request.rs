//! dshr → dsh 请求侧：RuntimeTask 处理 UI 命令（send → prompt、工作区、会话管理）。
//!
//! 与 [`event`]（dsh → dshr 返回侧）相对。协议形状在 dshr-protocol/requests，
//! 这里只做"命令 → Bridge 调用 → 落库 → 事件回报"。

use std::time::Instant;

use crate::task::events::{UiEvent, UiStatus};
use crate::{Error, inline_err, task::RtCmd, task::RuntimeTask, toast};

impl RuntimeTask {
    /// 处理一条 runtime 命令；返回 false 表示任务应结束。
    pub async fn handle_cmd(&mut self, cmd: RtCmd) -> bool {
        match cmd {
            RtCmd::NewSession { session_id } => {
                self.ensure_session(&session_id);
                true
            }
            RtCmd::Send { session_id, text } => {
                self.ensure_session(&session_id);
                let t0 = Instant::now();
                let result = match self.bridge.as_mut() {
                    Some(bridge) => bridge.prompt(&session_id, &text).await,
                    None => Err(Error::NotStarted),
                };
                match result {
                    Ok(outcome) => {
                        let duration = t0.elapsed().as_millis() as i64;
                        let _ = self.store.lock().unwrap().insert_request(
                            &self.info.id,
                            Some(&session_id),
                            None, // turn_id 等 turn/start 回填
                            crate::task::now_ms(),
                            "session_prompt",
                            Some(duration),
                            true,
                            None,
                        );
                        // tracker 记录 prompt 时间（备用；turn/start 回填在 handle_event 做）
                        if let Some(t) = self.trackers.get_mut(&session_id) {
                            t.on_prompt(crate::task::now_ms());
                        }
                        let _ = outcome.message_id;
                    }
                    Err(e) => {
                        let _ = self.store.lock().unwrap().insert_request(
                            &self.info.id,
                            Some(&session_id),
                            None,
                            crate::task::now_ms(),
                            "session_prompt",
                            None,
                            false,
                            Some(&e.to_string()),
                        );
                        // 对话内错误：红色显示在对话流。
                        inline_err!(&self.ev_tx, "发送失败: {e}");
                    }
                }
                true
            }
            RtCmd::Rename { name } => {
                // 手动改名：落库 + 停自动命名 + 同步 UI。
                self.auto_named = false;
                let _ = self
                    .store
                    .lock()
                    .unwrap()
                    .update_runtime_name(&self.info.id, &name);
                self.info.name = name.clone();
                let _ = self.ev_tx.send(UiEvent::RuntimeRenamed {
                    runtime_id: self.info.id.clone(),
                    name,
                });
                true
            }
            RtCmd::SetWorkspace {
                cwd,
                provider,
                model,
                max_tokens,
            } => {
                // 决策 21：工作区一旦设置锁死；补设 = 更新 info + 重新 initialize（官方握手幂等）。
                if self.info.workspace.is_some() {
                    toast!(&self.ev_tx, "工作区已设置，不能修改");
                    return true;
                }
                self.info.workspace = Some(cwd.clone());
                match self.bridge.as_mut() {
                    Some(bridge) => {
                        match bridge.initialize(&provider, &model, Some(max_tokens)).await {
                            Ok(_) => {
                                let _ = self.ev_tx.send(UiEvent::Status {
                                    runtime_id: self.info.id.clone(),
                                    status: UiStatus::Ready,
                                    name: self.info.name.clone(),
                                    workspace: Some(cwd),
                                });
                            }
                            Err(e) => {
                                self.info.workspace = None;
                                toast!(&self.ev_tx, "设置工作区失败: {e}");
                            }
                        }
                    }
                    None => {
                        toast!(&self.ev_tx, "runtime 未启动，无法设置工作区");
                    }
                }
                true
            }
            RtCmd::RenameSession { session_id, name } => {
                let _ = self
                    .store
                    .lock()
                    .unwrap()
                    .update_session_title(&session_id, &name);
                let _ = self.ev_tx.send(UiEvent::Title {
                    runtime_id: self.info.id.clone(),
                    session_id,
                    title: name,
                });
                true
            }
            RtCmd::ArchiveSession { session_id } => {
                let _ = self.store.lock().unwrap().archive_session(&session_id);
                let _ = self.ev_tx.send(UiEvent::SessionRemoved {
                    runtime_id: self.info.id.clone(),
                    session_id,
                });
                true
            }
            RtCmd::DeleteSession { session_id } => {
                let _ = self.store.lock().unwrap().delete_session(&session_id);
                self.trackers.remove(&session_id);
                let _ = self.ev_tx.send(UiEvent::SessionRemoved {
                    runtime_id: self.info.id.clone(),
                    session_id,
                });
                true
            }
            RtCmd::ListWorkspace { path } => {
                crate::task::workspace::list(&self.info, &path, &self.ev_tx);
                true
            }
            RtCmd::Archive => {
                let _ = self.store.lock().unwrap().archive_runtime(&self.info.id);
                if let Some(bridge) = self.bridge.take() {
                    let _ = bridge.shutdown().await;
                }
                let _ = self.ev_tx.send(UiEvent::Status {
                    runtime_id: self.info.id.clone(),
                    status: UiStatus::Closed,
                    name: self.info.name.clone(),
                    workspace: self.info.workspace.clone(),
                });
                false
            }
            RtCmd::Delete => {
                // 彻底删除（决策 20）：先收进程，再物理删库（连坐全部数据）。
                if let Some(bridge) = self.bridge.take() {
                    let _ = bridge.shutdown().await;
                }
                let _ = self.store.lock().unwrap().delete_runtime(&self.info.id);
                let _ = self.ev_tx.send(UiEvent::Status {
                    runtime_id: self.info.id.clone(),
                    status: UiStatus::Closed,
                    name: self.info.name.clone(),
                    workspace: self.info.workspace.clone(),
                });
                false
            }
            RtCmd::Shutdown => {
                let _ = self
                    .store
                    .lock()
                    .unwrap()
                    .update_runtime_name(&self.info.id, "closed");
                let _ = self.store.lock().unwrap().archive_runtime(&self.info.id);
                if let Some(bridge) = self.bridge.take() {
                    let _ = bridge.shutdown().await;
                }
                false
            }
        }
    }

    /// 会话首次出现时：建 tracker + 落库 sessions + 告知 UI。
    fn ensure_session(&mut self, session_id: &str) {
        if self.trackers.contains_key(session_id) {
            return;
        }
        self.trackers.insert(
            session_id.to_string(),
            crate::core::session::SessionTracker::new(),
        );
        let _ = self.store.lock().unwrap().insert_session(
            session_id,
            &self.info.id,
            self.info.workspace.as_deref().unwrap_or_default(),
            None,
            crate::task::now_ms(),
            Some("idle"),
        );
        let _ = self.ev_tx.send(UiEvent::SessionCreated {
            runtime_id: self.info.id.clone(),
            session_id: session_id.to_string(),
        });
    }
}
