//! UI 消息（iced 的 Message：用户操作 → update 的输入）。
//! 大枚举按页注释分组（决策 22：不拆子枚举，保持简单）。

use iced::widget::text_editor;

use dshr_state::UiEvent;

use crate::app::Page;
use crate::setting::ConfigPane;

/// 任务页"..."菜单的目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    Runtime(String),
    Session(String),
}

/// UI 消息。
#[derive(Debug, Clone)]
pub enum Message {
    // ---- 全局 ----
    /// 后台事件（Iced Subscription 事件流直收，不再 100ms 轮询）。
    Event(UiEvent),
    /// 顶部菜单切换页面。
    Navigate(Page),
    /// 退出（shutdown 全部 runtime）。
    Close,
    /// 关闭弹窗。
    DismissDialog,
    /// 空操作（下载中遮罩点击等，无副作用）。
    Noop,
    // ---- 任务页 ----
    /// 添加 runtime 弹窗。
    AddPressed,
    CancelAdd,
    NameChanged(String),
    PathChanged(String),
    ConfirmAdd,
    /// 指定 runtime 下开会话（参数 = runtime_id）。
    NewSession(String),
    /// 切换选中会话（参数 = session_id）。
    SelectSession(String),
    /// 输入框编辑（多行编辑器动作）。
    InputAction(text_editor::Action),
    /// 发送消息。
    SendPressed,
    /// 伸缩输入框（拉高/复原）。
    ToggleInputExpand,
    /// "..."菜单展开/收起。
    ToggleMenu(MenuTarget),
    /// 进入改名态（"..."菜单里点"改名"）。
    StartRename(MenuTarget),
    /// 改名输入变化（内联编辑）。
    RenameChanged(String),
    /// 确定改名（用 renaming 状态发命令）。
    ConfirmRename,
    /// 取消改名。
    CancelRename,
    /// 工作区弹窗：打开/输入/确定/取消。
    WorkspaceAdd,
    WorkspaceChanged(String),
    ConfirmWorkspace,
    CancelWorkspace,
    /// runtime 归档。
    ArchiveRuntime(String),
    /// runtime 彻底删除（连坐数据）。
    DeleteRuntime(String),
    /// 会话归档。
    ArchiveSession(String),
    /// 会话彻底删除（连坐数据）。
    DeleteSession(String),
    /// 文件树：进入子目录（path 相对工作区）。
    FileOpen(String),
    /// 文件树：返回上级。
    FileUp,
    // ---- 监控页 ----
    // ---- 配置页 ----
    /// 下载/更新 dsh 无头运行时（决策 24：本期禁用，不触发；开放后配置页按钮接回）。
    #[allow(dead_code)]
    FetchDsh,
    /// 关闭下载进度弹窗。
    DismissFetch,
    /// 编辑缓冲区变化（text_editor 动作）。
    ConfigEdit(ConfigPane, text_editor::Action),
    /// 保存当前缓冲区到文件。
    ConfigSave(ConfigPane),
    /// 恢复默认模板（重写文件 + 刷新缓冲区）。
    ConfigReset(ConfigPane),
    // ---- 外观（配置页「外观」区块）----
    /// 选择主题（即时预览，保存后落盘）。
    ThemeSelect(String),
    /// 字号输入变化。
    FontSizeChanged(String),
    /// 保存外观（主题 + 字号 → config.json 的 ui 字段）。
    AppearanceSave,
    // ---- dsh 下载/镜像源（配置页「dsh 运行时」区块）----
    /// 镜像源输入变化（npm_registry；空 = 官方 registry）。
    RegistryChanged(String),
    /// 保存镜像源（写 config.json 的 npm_registry 字段）。
    RegistrySave,
}
