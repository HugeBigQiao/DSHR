//! 0.1.7-rc.2 新增的非消息类事件族：`image/offload`、`workspace/changes`。
//!
//! 主要用途：图片卸载决策（哪些输入图片出现位置被永久省略）与「某 turn 有工作区变更摘要」
//! 两个标记事件的 data 类型。
//! 为什么需要：`image/offload` 是 `IMAGE_OFFLOAD_REQUIRED` 失败后的补偿记录，**属于模型可见
//! 投影**（官方标 `@messageProjection`），回放语义特殊；`workspace/changes` 的载荷只有 turn
//!（摘要本体留在 Host）——两者的载荷都「看起来不足」，必须解释为什么，故单列成族。
//! 上接：`session_event.rs` 的判别枚举与 `session_event/fallback.rs` 的分发；
//!       `dshr-state` 的 fold（图片状态 / 变更时间线）。
//! 下接：无（只依赖 serde）。
//!
//! 官方对应：两者都由插件包注册（官方 `spill` 之外的两处声明合并）——
//! - `image/offload`：`packages/compaction/compaction-image-offload/src/projection.ts`
//!   的 `SessionEventMap['image/offload']`，载荷 `{ targets: ImageOffloadTarget[] }`。
//! - `workspace/changes`：`packages/deliverables/workspace-changes/src/types.ts`
//!   的 `SessionEventMap['workspace/changes']`，载荷 `{ turn: number }`——
//!   变更摘要本体留在 Host（由 `workspaceChanges.summary()` 按事件 seq 提供），
//!   日志里只记「这个 turn 有变更摘要」。
use serde::{Deserialize, Serialize};

/// `image/offload` 的 data：选择若干输入图片出现位置，永久从后续模型请求中省略。
/// 官方：packages/compaction/compaction-image-offload/src/projection.ts
///     的 SessionEventMap['image/offload']（0.1.7-rc.2 新增）
/// 用在路由以 `IMAGE_OFFLOAD_REQUIRED` 拒绝请求后，插件记录一次卸载决策并重试该步。
/// 官方标 `@messageProjection`：该事件是模型可见投影的一部分，需要纯解释器回放。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageOffloadData {
    pub targets: Vec<ImageOffloadTarget>,
}

/// 一次卸载决策选中的确切输入图片出现位置。
/// 官方：packages/compaction/compaction-image-offload/src/projection.ts 的 ImageOffloadTarget
/// 用在 ImageOffloadData.targets。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOffloadTarget {
    /// 承载这些出现位置的当前消息产出事件（user/message 或 tool/result）的 seq。
    pub seq: u64,
    /// 该不可变消息内**从 0 开始的深度优先**图片下标；非空且严格递增，
    /// 计入嵌套 tool/result 内的图片与**已被省略**的图片。
    pub image_indexes: Vec<u64>,
}

/// `workspace/changes` 的 data：某次顶层 turn 的变更文件已被汇总。
/// 官方：packages/deliverables/workspace-changes/src/types.ts
///     的 SessionEventMap['workspace/changes']（0.1.7-rc.2 新增）
/// 用在交付物/工作区变更的时间线标记；同一 turn 的最新事件覆盖更早的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceChangesData {
    /// 该变更摘要描述的 turn。
    pub turn: u64,
}
