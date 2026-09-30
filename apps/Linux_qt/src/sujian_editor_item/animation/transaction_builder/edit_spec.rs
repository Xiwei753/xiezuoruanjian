//! Issue #808 评论 5918236360: `VisualEditSpec` / `CompositionCommitCrossfadeSpec`
//! 两个归一化输入结构从 `transaction_builder.rs` 拆出。
//!
//! 事务构造入口 [`super::build_prepared_transaction`] 仍是唯一创建
//! `PreparedTextVisualTransaction` 的地方；本模块只承载「归一化编辑事件」的数据形状，
//! 让 transaction_builder.rs 回到生产文件结构上限以内（god-file 800 行）。

use crate::sujian_editor_item::animation::rebase::RebaseCaretHandoff;
use crate::sujian_editor_item::animation::{RebaseFrame, TextVisualOperationKind};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use writer_core::editor::OffsetMap;

/// Issue #747 评论 5813540976: Composition commit 特殊 crossfade 规格（preedit→candidate 形变动画）。
/// 仅在 operation_kind == CompositionCommitOrCancel 且 is_commit 且 !visual_text_unchanged 时有值。
pub(crate) struct CompositionCommitCrossfadeSpec {
    pub(crate) preedit_byte_start: usize,
    pub(crate) preedit_byte_end: usize,
    pub(crate) candidate_byte_start: usize,
    pub(crate) candidate_byte_end: usize,
}

/// Issue #747 评论 5805324575 / 5813540976: 统一视觉事务构造的「归一化编辑事件」。
///
/// 所有编辑来源（普通 Insert/Delete、IME composition update/commit）先把原始编辑
/// 状态归一化成 `VisualEditSpec`，再交给 [`super::build_prepared_transaction`] 这唯一一处
/// 创建 `PreparedTextVisualTransaction`。`composition.rs` 只负责把 IME 状态归一化成
/// `VisualEditSpec` 并调用同一个事务构造器，不再维护第二套事务创建算法。
///
/// Issue #747 评论 5813540976: spec 只携带归一化输入（`offset_map`、cursor line info、
/// `text_animation_enabled` / `caret_animation_enabled`、`composition_commit_crossfade`），
/// 不再携带 `units` 与 `cursor_visual_track`——它们是 builder 的输出，
/// 由 [`super::build_prepared_transaction`] 内部统一构造。
pub(crate) struct VisualEditSpec {
    pub(crate) key: VisualTransactionKey,
    pub(crate) operation_kind: TextVisualOperationKind,
    pub(crate) old_snapshot: EditorLayoutSnapshot,
    pub(crate) new_snapshot: EditorLayoutSnapshot,
    pub(crate) inserted_ranges: Vec<(usize, usize)>,
    pub(crate) deleted_ranges: Vec<(usize, usize)>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) old_cursor_rect: Option<CursorRect>,
    pub(crate) new_cursor_rect: Option<CursorRect>,
    pub(crate) old_cursor_visual_line_id: Option<usize>,
    pub(crate) new_cursor_visual_line_id: Option<usize>,
    pub(crate) old_cursor_line_top: f64,
    pub(crate) old_cursor_line_bottom: f64,
    pub(crate) new_cursor_line_top: f64,
    pub(crate) new_cursor_line_bottom: f64,
    pub(crate) cursor_owner_epoch: u64,
    pub(crate) layout_basis_revision: LayoutRevision,
    pub(crate) rebase_frames: Vec<RebaseFrame>,
    pub(crate) caret_handoff: Option<RebaseCaretHandoff>,
    pub(crate) visual_affected_byte_range_old: Option<(usize, usize)>,
    pub(crate) visual_affected_byte_range_new: Option<(usize, usize)>,
    /// Issue #756 评论 5821042551: 文字 unit（InsertReveal/DeleteConceal/Reflow）的时长。
    /// Issue #808: 文字始终拥有独立 timeline，协同时也不再共享。
    pub(crate) text_duration_ms: u64,
    /// Issue #756 评论 5821042551: cursor visual track 的时长。
    /// Issue #808: 光标始终拥有独立 timeline，协同时也不再共享。
    pub(crate) caret_duration_ms: u64,
    /// Issue #756: 文字动画开关（ReflowMove/ReflowCrossFade + InsertReveal/DeleteConceal）。
    /// coordinated=true 或 typing_animation_enabled=true 时为 true。
    pub(crate) text_animation_enabled: bool,
    /// Issue #756: 光标动画开关（caret motion track）。
    /// coordinated=true 或 smooth_cursor_enabled=true 时为 true。
    pub(crate) caret_animation_enabled: bool,
    /// Issue #756: 协同动画显式模式。决定吞吐字（InsertReveal/DeleteConceal）的遮罩
    /// 锚点是否取自 caret 位置。Issue #808 后协同不再把文字与光标绑死：
    /// - coordinated=true：吞吐字遮罩从 caret 位置展开/收拢（视觉上从光标处吐出/被光标吞进），
    ///   但文字动画按自己的 timeline + easing 推进，不消费 caret frame。
    /// - coordinated=false：吞吐字用 typing timeline 自己推进，遮罩锚点取默认值。
    ///
    /// 三种语义彻底分开：文字动画、光标动画、协同动画（遮罩锚点选择）。
    pub(crate) coordinated_animation_enabled: bool,
    pub(crate) composition_commit_crossfade: Option<CompositionCommitCrossfadeSpec>,
}
