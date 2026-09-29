//! Files-page editor backend.
//!
//! With the `code-editor` feature the page uses the canvas-based
//! `iced-code-editor`; without it a lightweight `text_editor` fallback keeps
//! the project buildable in offline environments.
//!
//! 主要用途：给文件页提供**同一套 `FileEditor` API**（new/view/update/content/mark_saved/
//! is_modified/syntax_name/set_dark/lose_focus/set_wrap_enabled/set_folding_enabled/
//! set_line_numbers_enabled），两种后端（真编辑器 / 纯文本兜底）在编译期二选一。
//! 为什么需要：`iced-code-editor` 是路径依赖（离线/未下载该仓库时无法编译），且它把 iced 的
//! 默认 features 拉回来会改变渲染后端（见 DESIGN.md §9.16）；把两套实现藏在同一个类型后面，
//! `files.rs` 才能完全不关心 feature，也不必写 `#[cfg]`。它约束：两套 backend 必须暴露**完全
//! 相同的公开方法集**（兜底后端用 no-op 顶掉不支持的能力），否则切换 feature 会编译失败。
//! 上接：`files::{FilesState, Message::Editor}`（唯一消费者）。
//! 下接：`iced_code_editor::{CodeEditor, Message}` 或 `iced::widget::text_editor`。
//! 官方对应：`packages/client/ui-sidebar-documentpreview/src/client/code/CodeBody.tsx`
//! （代码查看/高亮）与 `text/TextBody.tsx`（纯文本兜底），二者在官方也是同一份壳下的两种正文。

#[cfg(feature = "code-editor")]
mod backend {
    //! 真编辑器后端（`code-editor` feature 开启时编译）。
    //!
    //! 为什么需要：canvas 版的 `iced-code-editor` 提供语法高亮/折叠/行号/LSP 面，是本页的
    //! 目标形态；本模块把它包成 `FileEditor`，顺便把「iced 主题 → 编辑器色板」的换算收在这里。
    use iced::{Element, Task};
    use iced_code_editor::{CodeEditor, Message as CodeMessage};

    /// Messages emitted by the code editor.
    ///
    /// 为什么需要：`files::Message::Editor` 要转发一个消息类型，而 `iced-code-editor` 自己的
    /// `Message` 是外部类型；用一个单变体包装层隔离它，将来换编辑器只需改这里（不改 files.rs）。
    #[derive(Debug, Clone)]
    pub enum Message {
        /// 编辑器内部动作（输入/选区/滚动/折叠等，语义由 iced-code-editor 定义）。
        Action(CodeMessage),
    }

    /// One open file editor.
    ///
    /// 为什么需要：编辑器实例 + 它自己的主题状态必须成对保存（切主题时二者一起更新）；
    /// `language` 冗余保存是为了让工具栏能显示语法名而不必再问编辑器。
    pub struct FileEditor {
        editor: CodeEditor,
        /// 保留给后续多 tab / 手动切换语言用；当前语言由 `syntax_name()` 展示。
        #[allow(dead_code)]
        language: String,
        dark: bool,
    }

    /// App 深浅开关 → iced 主题（再交给编辑器的色板换算）。
    fn iced_theme(dark: bool) -> iced::Theme {
        if dark {
            iced::Theme::Dark
        } else {
            iced::Theme::Light
        }
    }

    impl FileEditor {
        /// Create an editor for one file.
        ///
        /// 为什么需要：编辑器必须一次性配好字体/字号/主题（`iced-code-editor` 这些设置没有
        /// 全局默认）；等宽字体 + 14pt 与文件页其它等宽展示保持一致。
        /// 入参/出参：`content` 为文件全文、`language` 为语法键（见 `files::syntax_for`）、
        /// `dark` 为初始主题；返回可直接挂到视图上的编辑器。
        pub fn new(content: &str, language: &str, dark: bool) -> Self {
            let mut editor = CodeEditor::new(content, language);
            editor.set_font(iced::Font::MONOSPACE);
            editor.set_font_size(14.0, true);
            let theme = iced_theme(dark);
            editor.set_theme(iced_code_editor::theme::from_iced_theme(&theme));
            Self {
                editor,
                language: language.to_string(),
                dark,
            }
        }

        /// Render the editor.
        ///
        /// 入参/出参：`&self`（编辑器内部状态由它自己持有）；返回可直接嵌入页面的
        /// `Element<Message>`（动作已映射为本地 `Message::Action`）。
        pub fn view(&self) -> Element<'_, Message> {
            self.editor.view().map(Message::Action)
        }

        /// Forward an editor message.
        ///
        /// 为什么需要：真编辑器的输入处理必须回传给它自己（它持有文本/选区/折叠状态），
        /// 所以本层只做包装转发。
        /// 入参/出参：`message` 为包装后的编辑器消息；返回编辑器要求的后续 `Task`（如光标滚动）。
        pub fn update(&mut self, message: Message) -> Task<Message> {
            match message {
                Message::Action(action) => self.editor.update(&action).map(Message::Action),
            }
        }

        /// Current full text.
        ///
        /// 入参/出参：无；返回编辑器当前全文（保存时写盘的正是它）。
        pub fn content(&self) -> String {
            self.editor.content()
        }

        /// Mark the current buffer as saved.
        ///
        /// 为什么需要：未保存标记由编辑器自己维护，写盘成功后必须清掉，否则「● 未保存」
        /// 会一直亮着。入参/出参：无；无返回值。
        pub fn mark_saved(&mut self) {
            self.editor.mark_saved();
        }

        /// Whether the buffer has unsaved changes.
        ///
        /// 入参/出参：无；返回 true = 有未保存修改（工具栏显示「● 未保存」）。
        pub fn is_modified(&self) -> bool {
            self.editor.is_modified()
        }

        /// Resolved syntax display name.
        ///
        /// 入参/出参：无；返回编辑器解析出的语法名（可能比传入的键更具体，故不直接用 `language`）。
        pub fn syntax_name(&self) -> String {
            self.editor.syntax_name().to_string()
        }

        /// Update the editor theme to follow the app.
        ///
        /// 入参/出参：`dark` 为新主题；无返回值。只换色板，不动文本/选区状态。
        pub fn set_dark(&mut self, dark: bool) {
            self.dark = dark;
            let theme = iced_theme(dark);
            self.editor
                .set_theme(iced_code_editor::theme::from_iced_theme(&theme));
        }

        /// Change syntax after opening a file under a different extension.
        ///
        /// 为什么需要：当前单编辑器会在换文件时整体重建（不必手动换语法），但多 tab / 手动切换
        /// 语言时需要它——所以保留；入参/出参：`language` 为新语法键，无返回值。
        #[allow(dead_code)] // 保留：多 tab / 手动切换语言时会用到。
        pub fn set_language(&mut self, language: &str) {
            self.editor.set_syntax(language);
            self.language = language.to_string();
        }

        /// Transfer focus away from the editor.
        ///
        /// 为什么需要：换文件时旧编辑器若仍持有焦点，下一次按键会打到已经不显示的缓冲上；
        /// 打开新文件前先主动弃焦。入参/出参：无；无返回值。
        pub fn lose_focus(&mut self) {
            self.editor.lose_focus();
        }

        /// Toggle line wrapping.
        ///
        /// 为什么需要：三个显示开关的状态存在 `FilesState` 里，而设置只作用于当前编辑器实例；
        /// 两者必须在切换与新建时都同步。入参/出参：`enabled` 为目标状态，无返回值。
        pub fn set_wrap_enabled(&mut self, enabled: bool) {
            self.editor.set_wrap_enabled(enabled);
        }

        /// Toggle folding.
        ///
        /// 入参/出参：`enabled` 为目标状态，无返回值（见 `set_wrap_enabled` 的同步说明）。
        pub fn set_folding_enabled(&mut self, enabled: bool) {
            self.editor.set_folding_enabled(enabled);
        }

        /// Toggle the gutter line numbers.
        ///
        /// 入参/出参：`enabled` 为目标状态，无返回值（见 `set_wrap_enabled` 的同步说明）。
        pub fn set_line_numbers_enabled(&mut self, enabled: bool) {
            self.editor.set_line_numbers_enabled(enabled);
        }

        /// Raw language key.
        ///
        /// 入参/出参：无；返回创建时传入的语法键（`syntax_name()` 是编辑器解析后的展示名）。
        #[allow(dead_code)] // 保留：多 tab / 手动切换语言时会用到。
        pub fn language(&self) -> &str {
            &self.language
        }
    }
}

#[cfg(not(feature = "code-editor"))]
mod backend {
    //! 纯文本兜底后端（`code-editor` feature 关闭时编译；离线/未下载路径依赖时用）。
    //!
    //! 为什么需要：`iced-code-editor` 是路径依赖，缺它就不能编译；本模块用 iced 自带的
    //! `text_editor` 提供同套 API（无语法高亮/折叠/行号，相关方法为 no-op）。
    use iced::widget::text_editor;
    use iced::{Element, Task};

    /// Messages emitted by the fallback text editor.
    ///
    /// 为什么需要：与真后端保持同一形状（单变体包装 iced 的 `text_editor::Action`），
    /// 使 `files.rs` 的转发代码与 feature 无关。
    #[derive(Debug, Clone)]
    pub enum Message {
        /// 编辑器动作（输入/选区/滚动；`is_edit()` 用于判脏）。
        Action(text_editor::Action),
    }

    /// One open file editor (plain text fallback).
    ///
    /// 为什么需要：`text_editor::Content` 不自带脏标记，而工具栏的「● 未保存」需要它，
    /// 所以这里自己维护 `dirty`。`language` 只用于显示语法名。
    pub struct FileEditor {
        content: text_editor::Content,
        language: String,
        dirty: bool,
    }

    impl FileEditor {
        /// Create a plain text editor for one file.
        ///
        /// 入参/出参：`content` 为全文、`language` 为语法键（仅显示用）、`_dark` 被忽略
        /// （纯文本后端无主题色板）；返回编辑器。
        pub fn new(content: &str, language: &str, _dark: bool) -> Self {
            Self {
                content: text_editor::Content::with_text(content),
                language: language.to_string(),
                dirty: false,
            }
        }

        /// Render the editor.
        ///
        /// 入参/出参：`&self`；返回等宽字体的多行文本编辑器
        /// （`on_action` 直连本地 `Message::Action`）。
        pub fn view(&self) -> Element<'_, Message> {
            text_editor::TextEditor::new(&self.content)
                .on_action(Message::Action)
                .font(iced::Font::MONOSPACE)
                .size(14.0)
                .padding(8)
                .into()
        }

        /// Forward an editor action.
        ///
        /// 为什么需要：纯文本后端没有内置脏标记，必须在这里按 `is_edit()` 自行置位；
        /// 同时 `perform` 是唯一真正改动缓冲的地方。
        /// 入参/出参：`message` 为包装动作；返回 `Task::none()`（本后端无异步副作用）。
        pub fn update(&mut self, message: Message) -> Task<Message> {
            match message {
                Message::Action(action) => {
                    if action.is_edit() {
                        self.dirty = true;
                    }
                    self.content.perform(action);
                    Task::none()
                }
            }
        }

        /// Current full text.
        ///
        /// 入参/出参：无；返回缓冲全文（保存时写盘用）。
        pub fn content(&self) -> String {
            self.content.text()
        }

        /// Mark the current buffer as saved.
        ///
        /// 入参/出参：无；无返回值（清掉本地脏标记）。
        pub fn mark_saved(&mut self) {
            self.dirty = false;
        }

        /// Whether the buffer has unsaved changes.
        ///
        /// 入参/出参：无；返回 true = 有未保存修改。
        pub fn is_modified(&self) -> bool {
            self.dirty
        }

        /// Fallback has no syntax highlighting.
        ///
        /// 入参/出参：无；返回「语言键 (plain)」以明示当前没有高亮
        /// （工具栏照常显示，避免看起来像坏了）。
        pub fn syntax_name(&self) -> String {
            format!("{} (plain)", self.language)
        }

        /// No-op fallback theme hook.
        ///
        /// 为什么需要：与真后端保持同一公开方法集（见文件头约束）；纯文本没有色板可换，
        /// 因此是 no-op。入参/出参：`_dark` 被忽略；无返回值。
        pub fn set_dark(&mut self, _dark: bool) {}

        /// Store the language key for the toolbar.
        ///
        /// 入参/出参：`language` 为新语法键；无返回值（本后端只影响 `syntax_name()` 显示）。
        #[allow(dead_code)] // 保留：多 tab / 手动切换语言时会用到。
        pub fn set_language(&mut self, language: &str) {
            self.language = language.to_string();
        }

        /// No-op fallback focus hook.
        ///
        /// 与真后端同方法集：`text_editor` 的焦点由 iced 管理，无需显式弃焦。
        pub fn lose_focus(&mut self) {}

        /// No-op fallback wrap hook.
        ///
        /// 与真后端同方法集：兜底后端不支持换行开关（`text_editor` 默认行为即换行）。
        pub fn set_wrap_enabled(&mut self, _enabled: bool) {}

        /// No-op fallback folding hook.
        ///
        /// 与真后端同方法集：纯文本后端没有折叠能力。
        pub fn set_folding_enabled(&mut self, _enabled: bool) {}

        /// No-op fallback gutter hook.
        ///
        /// 与真后端同方法集：纯文本后端没有行号栏。
        pub fn set_line_numbers_enabled(&mut self, _enabled: bool) {}

        /// Raw language key.
        ///
        /// 入参/出参：无；返回创建/设置时保存的语法键。
        #[allow(dead_code)] // 保留：多 tab / 手动切换语言时会用到。
        pub fn language(&self) -> &str {
            &self.language
        }
    }
}

/// 当前 feature 选中的后端（两套实现同 API，`files.rs` 因此不需要任何 `#[cfg]`）。
pub use backend::{FileEditor, Message};
