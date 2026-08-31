//! 监控页查询聚合层（对应 UI 监控页；决策 22）。
//!
//! 职责：dshr-data 的 read 只放"读取函数"，本层做"查询 + 聚合 + 算账"（token 汇总、
//! 工具调用统计、按时间线整理），被 Engine 的命令处理调用，结果经 UiEvent 回 UI。
//! 现状：M3 规划，先留空壳（监控页 UI 也是占位）。

/// 查询/聚合入口（M3 填充：token 账务 / 工具调用 / 文件变更看板）。
pub struct Monitor;

impl Monitor {
    /// 占位：未来按 session/runtime 聚合看板数据。
    #[allow(dead_code)]
    pub fn placeholder() {}
}
