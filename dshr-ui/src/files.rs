//! Files 页：左侧工作区文件树 + 右侧代码/文本编辑器。
//!
//! 主要用途：文件页的全部状态与交互——目录树懒加载（展开才列子目录）、打开文件、编辑、
//! 保存（工具栏按钮）、以及换行/折叠/行号三个编辑器开关；同时负责按扩展名推断语法高亮语言。
//! 为什么需要：文件页是唯一「有真实 IO 副作用」的页面（列目录/读文件/写文件），它必须自己持有
//! 缓存（`children` 按目录缓存、`expanded` 展开集）并把 IO 结果用消息回流（`LoadedDir`/
//! `LoadedFile`/`Saved`），这样 `update` 才能保持同步、不阻塞渲染；把这些与 UI 状态一起隔离在
//! 本文件，`app.rs` 只需转发消息。它约束：所有路径都是**工作区相对路径**（根校验在
//! `dshr_state::workspace` 里做，UI 不自己拼绝对路径）。
//! 上接：`app::{App, update}`（`App.files: FilesState`）。
//! 下接：`dshr_state::workspace`（`list_dir`/`read_text_file`/`write_text_file`）、
//! `files::editor::FileEditor`、`theme`。
//! 官方对应：`packages/client/ui-sidebar-files/src/client/FilesBody.tsx` 的 `FilesBody`
//! （工作区树 + 展开/选中 + 懒加载）；编辑器呈现对应
//! `packages/client/ui-sidebar-documentpreview/src/client/code/CodeBody.tsx` 与 `text/TextBody.tsx`。
//!
//! 文件风格 Rust 2018+：本文件为模块根，子模块在 files/。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use dshr_state::workspace::{self, FileEntry};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length, Task};

use crate::app::App;
use crate::theme;

pub mod editor;

pub use editor::FileEditor;

/// Files-page messages.
///
/// 为什么需要：文件页的 IO 是异步的（`Task::perform` + `spawn_blocking`），请求与结果必须分成
/// 不同变体——`Refresh`/`ToggleDir`/`SelectFile`/`Save` 是「请求」（发任务），
/// `LoadedDir`/`LoadedFile`/`Saved` 是「结果」（带 `Result` 回流）。所有结果都携带 `path`，
/// 因为期间用户可能已经切到另一个文件/目录，落状态前必须用它做归属判断。
#[derive(Debug, Clone)]
pub enum Message {
    /// Reload the root directory.
    Refresh,
    /// Root/directory entries arrived.
    LoadedDir {
        path: String,
        result: Result<Vec<FileEntry>, String>,
    },
    /// Expand or collapse a directory.
    ToggleDir(String),
    /// Open a file.
    SelectFile(String),
    /// File contents arrived.
    LoadedFile {
        path: String,
        result: Result<String, String>,
    },
    /// Forwarded editor message.
    Editor(editor::Message),
    /// Save the current editor buffer.
    Save,
    /// Save result.
    Saved {
        path: String,
        result: Result<(), String>,
    },
    /// Toggle soft wrapping.
    ToggleWrap,
    /// Toggle code folding.
    ToggleFolding,
    /// Toggle the gutter line numbers.
    ToggleLineNumbers,
}

/// 列目录（阻塞 IO → 异步任务）。
///
/// 为什么需要：`workspace::list_dir` 是同步文件系统调用，直接在 `update` 里跑会卡住整个 UI
/// 线程（大目录/慢盘明显）；`spawn_blocking` 把它挪到阻塞线程池。
/// 入参/出参：`path` 为工作区相对目录（空串 = 根）；返回该目录的 `FileEntry` 列表，
/// 或错误文本（join 失败或 workspace 拒绝该路径）——错误不 panic，交由 `status` 显示。
async fn list_dir_task(path: String) -> Result<Vec<FileEntry>, String> {
    tokio::task::spawn_blocking(move || workspace::list_dir(&path))
        .await
        .map_err(|error| format!("list task join failed: {error}"))?
}

/// 读文本文件（阻塞 IO → 异步任务）。
///
/// 为什么需要：同 `list_dir_task`——读大文件必须离开 UI 线程，否则打开文件会整窗卡住。
/// 入参/出参：`path` 为工作区相对路径；返回文件全文，或错误文本（越界路径/非 UTF-8/不存在）。
async fn read_file_task(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || workspace::read_text_file(&path))
        .await
        .map_err(|error| format!("read task join failed: {error}"))?
}

/// 写文本文件（阻塞 IO → 异步任务）。
///
/// 为什么需要：保存同样要离开 UI 线程；失败必须作为结果回流而不是 panic，否则一次写失败
/// （只读文件/磁盘满）会直接崩掉整个窗口。
/// 入参/出参：`path` 为工作区相对路径、`content` 为编辑器全文；返回 `Ok(())` 或错误文本
/// （`workspace` 的路径校验失败也会走这里）。
async fn write_file_task(path: String, content: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::write_text_file(&path, &content))
        .await
        .map_err(|error| format!("write task join failed: {error}"))?
}

/// Files-page state.
///
/// 主要用途：承载文件页全部状态（目录缓存/展开集/当前文件/编辑器实例/三个显示开关/状态行）
/// 与消息处理（`handle`）。
/// 为什么需要：文件树是「懒加载 + 缓存」结构——`children` 保存已列出的目录内容（键为目录路径），
/// `expanded` 决定哪些目录要递归展开；没有缓存每次重绘都要重列目录。`editor` 是当前文件的
/// 唯一编辑器（单文件单编辑器，多 tab 未做），三个开关（wrap/folding/line_numbers）必须留在
/// 状态里，因为编辑器实例会在换文件时重建，重建后要按当前开关恢复。
pub struct FilesState {
    /// Expanded directory paths.
    pub expanded: HashSet<String>,
    /// Directory children cache keyed by directory path.
    pub children: HashMap<String, Vec<FileEntry>>,
    /// Currently open file path.
    pub current_path: Option<String>,
    /// Current editor, when a file is open.
    pub editor: Option<FileEditor>,
    /// Status line.
    pub status: String,
    /// Whether a directory/file load is in flight.
    pub loading: bool,
    /// Whether the app is in dark mode.
    pub dark: bool,
    /// Whether soft wrapping is enabled.
    pub wrap: bool,
    /// Whether code folding is enabled.
    pub folding: bool,
    /// Whether gutter line numbers are shown.
    pub line_numbers: bool,
}

impl FilesState {
    /// Create empty Files-page state.
    ///
    /// 为什么需要：开页面时不能自动扫盘（大工作区会卡首帧），所以初始只有一句「点刷新」提示；
    /// 三个显示开关默认全开（换行/折叠/行号）以匹配常见编辑器预期。
    /// 入参/出参：`dark` 为初始主题（新建的编辑器要跟着走）；返回空状态。
    pub fn new(dark: bool) -> Self {
        Self {
            expanded: HashSet::new(),
            children: HashMap::new(),
            current_path: None,
            editor: None,
            status: "Refresh to load the workspace tree".to_string(),
            loading: false,
            dark,
            wrap: true,
            folding: true,
            line_numbers: true,
        }
    }

    /// Update the editor theme when the app theme changes.
    ///
    /// 为什么需要：编辑器实例自带主题（语法色板来自 iced-code-editor），不会跟随 App 的
    /// `dark` 自动变；切成浅色时必须显式推给它，否则浅底窗口里会出现深色代码区。
    /// 入参/出参：`dark` 为新主题；无返回值（无打开文件时只记状态，等下次建编辑器时用）。
    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        if let Some(editor) = &mut self.editor {
            editor.set_dark(dark);
        }
    }

    /// Handle one Files-page message.
    ///
    /// 为什么需要：这是文件页唯一的状态入口——请求类变体发出异步任务并置 `loading`
    /// （同时立刻写一句状态行，让慢盘也有反馈），结果类变体落缓存/落编辑器/写错误文本。
    /// 入参/出参：`message` 为文件页消息；返回 iced `Task`（IO 类变体返回真正任务，其余
    /// `Task::none()`）。
    /// 错误条件：无返回值层面的错误——`Result::Err` 一律写进 `status`（清空编辑器，
    /// 避免「显示上一个文件内容却标着新文件名」）。
    pub fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => {
                self.loading = true;
                self.status = "loading workspace tree...".to_string();
                Task::perform(list_dir_task(String::new()), |result| Message::LoadedDir {
                    path: String::new(),
                    result,
                })
            }
            Message::LoadedDir { path, result } => {
                self.loading = false;
                match result {
                    Ok(entries) => {
                        self.status = format!(
                            "{} entries in {}",
                            entries.len(),
                            if path.is_empty() { "workspace" } else { &path }
                        );
                        self.children.insert(path, entries);
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::ToggleDir(path) => {
                if self.expanded.remove(&path) {
                    return Task::none();
                }
                self.expanded.insert(path.clone());
                if self.children.contains_key(&path) {
                    return Task::none();
                }
                self.loading = true;
                self.status = format!("loading {path}...");
                let task_path = path.clone();
                Task::perform(list_dir_task(path), move |result| Message::LoadedDir {
                    path: task_path,
                    result,
                })
            }
            Message::SelectFile(path) => {
                self.current_path = Some(path.clone());
                if let Some(editor) = &mut self.editor {
                    editor.lose_focus();
                }
                self.loading = true;
                self.status = format!("loading {path}...");
                let task_path = path.clone();
                Task::perform(read_file_task(path), move |result| Message::LoadedFile {
                    path: task_path,
                    result,
                })
            }
            Message::LoadedFile { path, result } => {
                self.loading = false;
                match result {
                    Ok(content) => {
                        let language = syntax_for(&path);
                        let mut editor = FileEditor::new(&content, language, self.dark);
                        editor.set_wrap_enabled(self.wrap);
                        editor.set_folding_enabled(self.folding);
                        editor.set_line_numbers_enabled(self.line_numbers);
                        self.editor = Some(editor);
                        self.status = format!("opened {path}");
                    }
                    Err(error) => {
                        self.editor = None;
                        self.status = error;
                    }
                }
                Task::none()
            }
            Message::Editor(message) => match &mut self.editor {
                Some(editor) => editor.update(message).map(Message::Editor),
                None => Task::none(),
            },
            Message::Save => {
                let Some(path) = self.current_path.clone() else {
                    self.status = "no file selected".to_string();
                    return Task::none();
                };
                let Some(editor) = self.editor.as_ref() else {
                    self.status = "no editor open".to_string();
                    return Task::none();
                };
                let content = editor.content();
                self.status = format!("saving {path}...");
                let task_path = path.clone();
                Task::perform(write_file_task(path, content), move |result| {
                    Message::Saved {
                        path: task_path,
                        result,
                    }
                })
            }
            Message::Saved { path, result } => {
                match result {
                    Ok(()) => {
                        if self.current_path.as_deref() == Some(path.as_str()) {
                            if let Some(editor) = &mut self.editor {
                                editor.mark_saved();
                            }
                        }
                        self.status = format!("saved {path}");
                    }
                    Err(error) => self.status = error,
                }
                Task::none()
            }
            Message::ToggleWrap => {
                self.wrap = !self.wrap;
                if let Some(editor) = &mut self.editor {
                    editor.set_wrap_enabled(self.wrap);
                }
                Task::none()
            }
            Message::ToggleFolding => {
                self.folding = !self.folding;
                if let Some(editor) = &mut self.editor {
                    editor.set_folding_enabled(self.folding);
                }
                Task::none()
            }
            Message::ToggleLineNumbers => {
                self.line_numbers = !self.line_numbers;
                if let Some(editor) = &mut self.editor {
                    editor.set_line_numbers_enabled(self.line_numbers);
                }
                Task::none()
            }
        }
    }
}

/// 扩展名 → iced-code-editor 的语法键（未知/无扩展名回落 `text`）。
///
/// 为什么需要：语法键是编辑器 API 的字符串契约（`set_syntax`/`CodeEditor::new`），必须有唯一
/// 映射点；`tsx` 归 typescript、`mjs`/`cjs` 归 javascript 是为了让常见变体都有高亮。
/// 入参/出参：`path` 为文件路径（只用扩展名，大小写不敏感）；返回静态语法键。
fn syntax_for(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "ps1" => "powershell",
        _ => "text",
    }
}

/// 递归拼一棵目录树（按 `expanded` 决定是否下钻，缩进按 `depth`）。
///
/// 为什么需要：目录树是「缓存 + 展开集」驱动的递归结构，且只有已展开且已加载的目录才有子列；
/// 用递归函数把它变成列，`view` 就不必关心树的深度。
/// 入参/出参：`files` 提供缓存与选中态、`path` 为当前目录（空串 = 根）、`depth` 为缩进层级、
/// `app` 提供调色板/字号；返回该层子树的 `Column`（目录未被列出时为空列，不是错误）。
fn tree_column<'a>(
    files: &'a FilesState,
    path: &str,
    depth: usize,
    app: &'a App,
) -> iced::widget::Column<'a, Message> {
    let palette = app.palette();
    let mut column = iced::widget::Column::new().spacing(1);
    let Some(entries) = files.children.get(path) else {
        return column;
    };
    for entry in entries {
        let indent = Space::new().width(Length::Fixed(depth as f32 * 14.0));
        if entry.is_dir {
            let expanded = files.expanded.contains(&entry.path);
            let arrow = if expanded { "\u{25be}" } else { "\u{25b8}" };
            let label = row![
                indent,
                text(arrow).size(app.fs(12)).color(palette.label_caption),
                text(&entry.name)
                    .size(app.fs(13))
                    .color(palette.label_primary),
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center);
            column = column.push(
                button(label)
                    .on_press(Message::ToggleDir(entry.path.clone()))
                    .style(theme::ghost_button(palette))
                    .padding([3, 6])
                    .width(Length::Fill),
            );
            if expanded {
                column = column.push(tree_column(files, &entry.path, depth + 1, app));
            }
        } else {
            let selected = files.current_path.as_deref() == Some(entry.path.as_str());
            let label = row![
                indent,
                text(" ").size(app.fs(12)),
                text(&entry.name)
                    .size(app.fs(13))
                    .color(palette.label_primary),
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center);
            column = column.push(
                button(label)
                    .on_press(Message::SelectFile(entry.path.clone()))
                    .style(theme::nav_button(palette, selected))
                    .padding([3, 6])
                    .width(Length::Fill),
            );
        }
    }
    column
}

/// Render the Files page.
///
/// 主要用途：画左树右编辑器两栏 + 顶部工具栏，并把树的展开/选中与编辑器渲染接到状态上。
/// 为什么需要：文件页的两栏比例（树 1 : 编辑器 3）与工具栏（刷新/保存/三个开关 + 当前文件
/// 信息行）只在这里表达；未打开文件时给一句引导文案而不是空白。
/// 入参/出参：`app` 提供 `files` 状态与调色板/字号；返回文件页 `Element<Message>`。
pub fn view(app: &App) -> Element<'_, Message> {
    let palette = app.palette();
    let files = &app.files;

    let tree = container(
        scrollable(tree_column(files, "", 0, app))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .padding(8)
    .style(theme::surface(palette, palette.sidebar_fill, 0.0));

    let dirty = files.editor.as_ref().is_some_and(FileEditor::is_modified);
    let syntax = files
        .editor
        .as_ref()
        .map(FileEditor::syntax_name)
        .unwrap_or_default();

    let mut info = row![
        text(
            files
                .current_path
                .as_deref()
                .unwrap_or("\u{672a}\u{9009}\u{62e9}\u{6587}\u{4ef6}")
        )
        .size(app.fs(12))
        .color(palette.label_secondary)
    ]
    .spacing(6)
    .align_y(iced::alignment::Vertical::Center);

    if !syntax.is_empty() {
        info = info.push(
            text(format!("\u{00b7} {syntax}"))
                .size(app.fs(12))
                .color(palette.label_caption),
        );
    }
    if dirty {
        info = info.push(
            text("\u{25cf} \u{672a}\u{4fdd}\u{5b58}")
                .size(app.fs(12))
                .color(palette.warn),
        );
    }

    let toolbar = row![
        button(text("\u{5237}\u{65b0}").size(app.fs(12)))
            .on_press(Message::Refresh)
            .style(theme::ghost_button(palette))
            .padding([4, 10]),
        button(text("\u{4fdd}\u{5b58}").size(app.fs(12)))
            .on_press(Message::Save)
            .style(theme::primary_button(palette))
            .padding([4, 10]),
        // 三个开关按钮：开启态用 nav_button 的 active 底色，一眼能看出开/关。
        button(text("\u{6362}\u{884c}").size(app.fs(12)))
            .on_press(Message::ToggleWrap)
            .style(theme::nav_button(palette, files.wrap))
            .padding([4, 10]),
        button(text("\u{6298}\u{53e0}").size(app.fs(12)))
            .on_press(Message::ToggleFolding)
            .style(theme::nav_button(palette, files.folding))
            .padding([4, 10]),
        button(text("\u{884c}\u{53f7}").size(app.fs(12)))
            .on_press(Message::ToggleLineNumbers)
            .style(theme::nav_button(palette, files.line_numbers))
            .padding([4, 10]),
        info,
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Center);

    let body: Element<'_, Message> = if let Some(editor) = &files.editor {
        editor.view().map(Message::Editor)
    } else {
        container(
            text("\u{9009}\u{62e9}\u{5de6}\u{4fa7}\u{6587}\u{4ef6}\u{67e5}\u{770b}\u{6216}\u{7f16}\u{8f91}")
                .size(app.fs(13))
                .color(palette.label_caption),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    let editor_pane = container(column![toolbar, body].spacing(8))
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .padding(8);

    row![tree, editor_pane]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
