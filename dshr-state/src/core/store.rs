//! 写库薄封装：把 dshr-data 的函数收口成一个 Store。
//!
//! state 内唯一持有 rusqlite 连接的地方；RuntimeTask 通过 Arc<Mutex<Store>> 共享
//! （SQLite 连接不是 Sync，Mutex 包一层；本地写入微秒级，锁竞争可忽略）。

use std::sync::{Arc, Mutex};

use crate::Error;

/// 数据库薄封装。
#[derive(Debug)]
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    /// 打开（或创建）数据库，建表。
    pub fn open(path: &str) -> Result<Self, Error> {
        Ok(Self {
            conn: dshr_data::open(path)?,
        })
    }

    /// 包装成共享句柄（消费循环跨任务用）。
    pub fn share(self) -> Arc<Mutex<Store>> {
        Arc::new(Mutex::new(self))
    }

    // ---- runtime ----
    pub fn insert_runtime(
        &self,
        id: &str,
        name: &str,
        state: &str,
        created_at: i64,
        command: &str,
        args: Option<&str>,
        current_dir: Option<&str>,
        env: Option<&str>,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_runtime(
            &self.conn,
            id,
            name,
            state,
            created_at,
            command,
            args,
            current_dir,
            env,
        )?)
    }

    pub fn update_runtime_name(&self, id: &str, name: &str) -> Result<(), Error> {
        Ok(dshr_data::write::update_runtime_name(&self.conn, id, name)?)
    }

    pub fn archive_runtime(&self, id: &str) -> Result<(), Error> {
        Ok(dshr_data::write::archive_runtime(&self.conn, id)?)
    }

    pub fn delete_runtime(&self, id: &str) -> Result<(), Error> {
        Ok(dshr_data::write::delete_runtime(&self.conn, id)?)
    }

    pub fn archive_session(&self, id: &str) -> Result<(), Error> {
        Ok(dshr_data::write::archive_session(&self.conn, id)?)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), Error> {
        Ok(dshr_data::write::delete_session(&self.conn, id)?)
    }

    // ---- session ----
    #[allow(clippy::too_many_arguments)]
    pub fn insert_session(
        &self,
        id: &str,
        runtime_id: &str,
        cwd: &str,
        parent_session_id: Option<&str>,
        created_at: i64,
        status: Option<&str>,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_session(
            &self.conn,
            id,
            runtime_id,
            cwd,
            parent_session_id,
            created_at,
            status,
        )?)
    }

    pub fn update_session_status(&self, id: &str, status: &str) -> Result<(), Error> {
        Ok(dshr_data::write::update_session_status(
            &self.conn, id, status,
        )?)
    }

    pub fn update_session_title(&self, id: &str, title: &str) -> Result<(), Error> {
        Ok(dshr_data::write::update_session_title(
            &self.conn, id, title,
        )?)
    }

    pub fn update_session_last_seq(&self, id: &str, last_seq: i64) -> Result<(), Error> {
        Ok(dshr_data::write::update_session_last_seq(
            &self.conn, id, last_seq,
        )?)
    }

    // ---- request ----
    #[allow(clippy::too_many_arguments)]
    pub fn insert_request(
        &self,
        runtime_id: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        time: i64,
        method: &str,
        duration_ms: Option<i64>,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_request(
            &self.conn,
            runtime_id,
            session_id,
            turn_id,
            time,
            method,
            duration_ms,
            success,
            error_message,
        )?)
    }

    pub fn update_request_turn_id(
        &self,
        runtime_id: &str,
        session_id: &str,
        turn_id: &str,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::update_request_turn_id(
            &self.conn, runtime_id, session_id, turn_id,
        )?)
    }

    // ---- turn ----
    pub fn insert_turn(
        &self,
        turn_id: &str,
        runtime_id: &str,
        session_id: &str,
        turn: i64,
        started_at: i64,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_turn(
            &self.conn, turn_id, runtime_id, session_id, turn, started_at,
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_turn(
        &self,
        turn_id: &str,
        ended_at: Option<i64>,
        duration_ms: Option<i64>,
        reason: Option<&str>,
        usage_input: Option<i64>,
        usage_output: Option<i64>,
        usage_cache_read: Option<i64>,
        usage_cache_write: Option<i64>,
        usage_reasoning: Option<i64>,
        user_text: Option<&str>,
        assistant_text: Option<&str>,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::finish_turn(
            &self.conn,
            turn_id,
            ended_at,
            duration_ms,
            reason,
            usage_input,
            usage_output,
            usage_cache_read,
            usage_cache_write,
            usage_reasoning,
            user_text,
            assistant_text,
        )?)
    }

    // ---- event（lossless 底线，永远先写这张表） ----
    pub fn insert_event(
        &self,
        session_id: &str,
        seq: i64,
        event_type: &str,
        time: i64,
        turn: Option<i64>,
        step: Option<i64>,
        payload: &str,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_event(
            &self.conn, session_id, seq, event_type, time, turn, step, payload,
        )?)
    }

    // ---- tool ----
    #[allow(clippy::too_many_arguments)]
    pub fn insert_tool_call(
        &self,
        runtime_id: &str,
        session_id: &str,
        turn_id: Option<&str>,
        call_id: &str,
        name: &str,
        arguments: Option<&str>,
        result_text: Option<&str>,
        is_error: bool,
        duration_ms: Option<i64>,
        meta: Option<&str>,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_tool_call(
            &self.conn,
            runtime_id,
            session_id,
            turn_id,
            call_id,
            name,
            arguments,
            result_text,
            is_error,
            duration_ms,
            meta,
        )?)
    }

    // ---- log ----
    pub fn insert_log(
        &self,
        runtime_id: &str,
        time: i64,
        level: &str,
        message: &str,
    ) -> Result<(), Error> {
        Ok(dshr_data::write::insert_log(
            &self.conn, runtime_id, time, level, message,
        )?)
    }
}
