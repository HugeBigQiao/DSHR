//! 会话状态机：消费循环为每个会话维护的内存态。
//!
//! 职责：turn 开行/回填、tool 配对（算 duration）、token 累积（跨步汇总）、
//! 最近 prompt 时间（requests.turn_id 延迟回填）、last_seq 增量书签。

use std::collections::HashMap;

use dshr_protocol::llm::TokenUsage;

/// 一轮结束时需要回填 turns 表的全部数据。
#[derive(Debug)]
pub struct TurnFinalize {
    pub turn_id: String,
    pub turn: u64,
    pub started_at: i64,
    pub ended_at: i64,
    pub user_text: Option<String>,
    pub assistant_text: Option<String>,
    pub usage: Option<TokenUsage>,
}

/// 一个会话的推进状态（消费循环持有）。
#[derive(Debug, Default)]
pub struct SessionTracker {
    /// 当前打开的 turn（None = 空闲）。
    pub current_turn: Option<u64>,
    /// turn 开始时间（epoch ms），turn/end 算耗时用。
    pub turn_started_at: Option<i64>,
    /// 当前 turn 的 turn_id（`runtime_id-session_id-turn`，turns 表主键）。
    turn_id: Option<String>,
    /// 本 turn 的用户消息文本（user/message 时记）。
    user_text: Option<String>,
    /// 本 turn 的助手消息文本（assistant/message 时记）。
    assistant_text: Option<String>,
    /// 最近一次 session_prompt 的发出时间（turn/start 回填 requests.turn_id 用）。
    pub last_prompt_at: Option<i64>,
    /// 最近事件 seq（增量书签，写 sessions.last_seq）。
    pub last_seq: i64,
    /// 本会话累计的 token 用量（assistant/message.usage 跨步累加，turn/end 带出）。
    pub usage: Option<TokenUsage>,
    /// 挂起的工具调用：call_id → (time, name)（tool/result 配对算 duration）。
    pending_tool_calls: HashMap<String, (i64, String)>,
}

impl SessionTracker {
    /// 新会话的空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次 prompt（请求计量：time 写入，等 turn/start 回填 turn_id）。
    pub fn on_prompt(&mut self, time: i64) {
        self.last_prompt_at = Some(time);
    }

    /// turn/start：开行。
    /// 接收：归属（runtime/session）+ turn 号 + 事件时间。
    /// 处理：记录 current_turn/started_at/turn_id，重置本轮文本缓存。
    /// 生成：turn_id（调用方开 turns 行用）。
    pub fn on_turn_start(
        &mut self,
        runtime_id: &str,
        session_id: &str,
        turn: u64,
        time: i64,
    ) -> String {
        self.current_turn = Some(turn);
        self.turn_started_at = Some(time);
        self.turn_id = Some(format!("{runtime_id}-{session_id}-{turn}"));
        self.user_text = None;
        self.assistant_text = None;
        self.turn_id.clone().expect("刚写入")
    }

    /// 记录本轮用户消息文本。
    pub fn on_user_message(&mut self, text: &str) {
        self.user_text = Some(text.to_string());
    }

    /// 记录本轮助手消息文本。
    pub fn on_assistant_message(&mut self, text: &str) {
        self.assistant_text = Some(text.to_string());
    }

    /// 累计一次 usage（assistant/message 的账目，跨步累加）。
    pub fn add_usage(&mut self, usage: Option<&TokenUsage>) {
        let Some(u) = usage else { return };
        let cur = self.usage.get_or_insert_with(|| TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        });
        cur.input_tokens += u.input_tokens;
        cur.output_tokens += u.output_tokens;
        cur.cache_read_tokens = add_opt(cur.cache_read_tokens, u.cache_read_tokens);
        cur.cache_write_tokens = add_opt(cur.cache_write_tokens, u.cache_write_tokens);
        cur.reasoning_tokens = add_opt(cur.reasoning_tokens, u.reasoning_tokens);
    }

    /// turn/end：收行，取回填数据并重置轮状态。
    /// 接收：结束时间。
    /// 生成：TurnFinalize（调用方 finish_turn + update_request_turn_id 用）。
    pub fn take_turn_finalize(&mut self, ended_at: i64) -> Option<TurnFinalize> {
        let turn_id = self.turn_id.take()?;
        let turn = self.current_turn.take()?;
        let started_at = self.turn_started_at.take()?;
        Some(TurnFinalize {
            turn_id,
            turn,
            started_at,
            ended_at,
            user_text: self.user_text.take(),
            assistant_text: self.assistant_text.take(),
            usage: self.usage.take(),
        })
    }

    /// 登记一次 tool/call（等 result 配对）。
    pub fn on_tool_call(&mut self, call_id: &str, time: i64, name: &str) {
        self.pending_tool_calls
            .insert(call_id.to_string(), (time, name.to_string()));
    }

    /// tool/result 配对：取出 call 的 (time, name)，算 duration。
    /// 生成：Option<(name, duration_ms)>——调用方拼 UiToolUse。
    pub fn take_tool_start(&mut self, call_id: &str, result_time: i64) -> Option<(String, i64)> {
        self.pending_tool_calls
            .remove(call_id)
            .map(|(time, name)| (name, result_time.saturating_sub(time)))
    }
}

fn add_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}
