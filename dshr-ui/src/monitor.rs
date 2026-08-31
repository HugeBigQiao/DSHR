//! 监控页：数据看板（M3 规划，先占位）。
//!
//! 数据来源：dshr-data（dshr.db）查询，UI 主动拉——不走事件流（配置/监控都是命令/查询式）。

use iced::Element;
use iced::widget::{column, text};

use crate::app::{App, MUTED, fs};
use crate::message::Message;

/// 监控页状态（App.monitor；看板数据 M3 填充）。
pub struct MonitorPane {
    /// 预留：看板查询结果缓存（token 账务 / 工具调用 / 文件变更）。
    _placeholder: (),
}

impl Default for MonitorPane {
    fn default() -> Self {
        Self { _placeholder: () }
    }
}

/// 监控页：数据看板占位（M3 填充）。
pub fn monitor_page(app: &App) -> Element<'_, Message> {
    let mut col = column![text("监控").size(fs(app, 18))]
        .padding(20)
        .spacing(10);
    if app.task.runtimes.is_empty() {
        col = col.push(
            text("还没有 runtime。到「任务」页添加后，这里将展示 token 账务 / 工具调用 / 文件变更看板（M3）。")
                .size(fs(app, 13))
                .color(MUTED),
        );
    } else {
        for rt in &app.task.runtimes {
            col = col.push(text(format!(
                "• {}（{} 个会话）",
                rt.name,
                rt.sessions.len()
            )));
        }
    }
    col.into()
}
