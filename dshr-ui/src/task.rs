//! 任务页：三区布局（左侧边栏 / 中间对话 / 右侧预留）。
//! 对标官方三栏壳（AppFrame: sidebar | conversation | details）；侧边栏可收起（底部图标栏）。
use iced::widget::row;
use iced::{Element, Length};

use crate::app::App;

pub mod chat;
pub mod details;
pub mod sidebar;

/// 任务页消息（侧边栏树 + 对话区）。
#[derive(Debug, Clone)]
pub enum Message {
    /// 新建 runtime（= 新开一个 dsh 子进程）。
    NewRuntime,
    /// 展开/收起 ⋯ 菜单（runtime_id, session_id?；None = runtime 行）。
    MenuToggle(String, Option<String>),
    /// 展开/收起 runtime 的会话列表（+ / − 按钮）。
    ToggleRuntimeExpand(String),
    /// 鼠标悬停行（hover 显示 ⋯/+ 与行背景）。
    Hover(Option<(String, Option<String>)>),
    /// 在 runtime 下新建会话。
    NewSession(String),
    DeleteRuntime(String),
    ArchiveRuntime(String),
    DeleteSession(String, String),
    ArchiveSession(String, String),
    /// 选中会话（runtime_id, session_id）。
    SelectSession(String, String),
    /// composer 编辑动作（多行编辑器）。
    ComposerEdit(iced::widget::text_editor::Action),
    /// 发送。
    Send,
    /// 展开/收起工具卡片（参数 = 消息 seq：快照整体刷新后索引仍稳定）。
    ToggleTool(u64),
}

/// 渲染三区（侧边栏收起时只留对话 + 右侧）。
pub fn view(app: &App) -> Element<'_, Message> {
    if app.sidebar_collapsed {
        row![chat::view(app), details::view(app)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        row![sidebar::view(app), chat::view(app), details::view(app)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
