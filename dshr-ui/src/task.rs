//! 任务页：三区布局（左侧边栏 / 中间对话 / 右侧预留）。
//! 对标官方三栏壳（AppFrame: sidebar | conversation | details）；侧边栏可收起（底部图标栏）。
//!
//! 主要用途：定义任务页的消息枚举（侧边栏树动作 + 对话区动作），并只负责把三个子区横向拼起来。
//! 为什么需要：它是任务页的模块根——`Message` 必须有一个共同的类型让子区互相转发（侧边栏的
//! 菜单项与对话区的发送都经 `app::Message::Task(..)` 进来），而拼装逻辑单独成文件才能让
//! `sidebar`/`chat`/`details` 各自只关心自己的区域。它约束：子区之间不直接调用，一律经
//! `task::Message` + `App::handle_task` 通信。
//! 上接：`app::{App::view, App::handle_task}`（`Message` 经 `Message::Task` 嵌套）。
//! 下接：`task::{sidebar, chat, details}`（三个子区的 `view`）、`theme`。
//! 官方对应：`packages/client/ui-layout/src/client/AppFrame.tsx` 的 `AppFrame`
//! （`sidebar | center | rightbar` 三栏网格；侧边栏收起对应其 `sidebarCollapsed` 列宽解算）。
use iced::widget::row;
use iced::{Element, Length};

use crate::app::App;

pub mod chat;
pub mod details;
pub mod sidebar;

/// 任务页消息（侧边栏树 + 对话区）。
///
/// 为什么需要：这些动作全部由侧边栏/对话区的控件产生，但只有 `App` 能做真实决策（发命令、
/// 改全局 `menu`/`hover`/`expanded_tools`）；把「产生消息」与「处理消息」分开，两个子区就能
/// 保持无状态。多数变体携带 `runtime_id`/`session_id`，因为菜单与选中态都是按行定位的。
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
    ///
    /// 当前单会话阶段等价于「重置当前会话」（App 映射到 `BridgeCmd::ResetSession`）；
    /// 多会话目录落地后才会真的新增一行。
    NewSession(String),
    /// 删除 runtime：当前映射为停止 runtime（进程 shutdown）。
    DeleteRuntime(String),
    /// 归档 runtime：当前与删除同义（保留数据可查待 store 接线）。
    ArchiveRuntime(String),
    /// 删除会话（runtime_id, session_id）：当前映射为重置当前会话。
    DeleteSession(String, String),
    /// 归档会话：当前与删除同义。
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
///
/// 为什么需要：三区横向拼装写成 `row![a, b, c]` 单函数即可，但每个子区都按 `Length::FillPortion`
/// 参与分配（1 : 3 : 1），说明它们是同一行内的按比例分配，而非独立面板。
/// 入参/出参：`app` 提供 `sidebar_collapsed` 与各子区所需状态；返回任务页 `Element<Message>`。
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
