//! 自定义控件（覆盖式下拉菜单 Popover）。
//!
//! 主要用途：本 crate 自研 iced `Widget` 的模块根，目前只收 `popover`。
//! 为什么需要：自研控件是「与 iced 内部 API 打交道」的代码（`advanced::{Widget, Overlay, Tree}`），
//! 与页面布局代码性质完全不同，单独成模块才能在 iced 升级时一处处排查（见 DESIGN.md §10 的
//! iced 0.14 坑清单）。它约束：自定义控件放这里，页面不得直接实现 `Widget`。
//! 上接：`task::sidebar`（唯一使用者，经 `crate::widgets::popover::Popover`）。
//! 下接：`widgets::popover`。
//! 官方对应：`packages/client/ui-primitives/src/index.ts`（官方把可复用控件集中在一个包里，
//! 但它是 TS/React 组件库，与 iced widget 不同构）。

pub mod popover;
