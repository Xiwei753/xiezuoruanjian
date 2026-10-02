//! Issue #808 评论 5918236360: `VisualEditSpec` / `CompositionCommitCrossfadeSpec`
//! 两个归一化输入结构从 `transaction_builder.rs` 拆出。
//!
//! 事务构造入口 [`super::build_prepared_transaction`] 仍是唯一创建
//! `PreparedTextVisualTransaction` 的地方；本模块只承载「归一化编辑事件」的数据形状，
//! 让 transaction_builder.rs 回到生产文件结构上限以内（god-file 800 行）。
use crate::sujian_editor_item::AnimationSkipFields;

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
    /// Issue #815 评论 5947443780: 协同时只有 ReflowMove/ReflowCrossFade 用这个值走
    /// 独立 `Timed`；协同 InsertReveal/DeleteConceal 是 `CaretTrack`，逐帧边界来自
    /// cursor track 的当前帧，不用这个时长。
    pub(crate) text_duration_ms: u64,
    /// Issue #756 评论 5821042551: cursor visual track 的时长。
    /// Issue #815 评论 5947443780: 协同动画里这条 track 是唯一运动事实源，
    /// 文字吞吐层与光标层消费同一次采样。
    pub(crate) caret_duration_ms: u64,
    /// Issue #756: 文字动画开关（ReflowMove/ReflowCrossFade + InsertReveal/DeleteConceal）。
    /// coordinated=true 或 typing_animation_enabled=true 时为 true。
    pub(crate) text_animation_enabled: bool,
    /// Issue #756: 光标动画开关（caret motion track）。
    /// coordinated=true 或 smooth_cursor_enabled=true 时为 true。
    pub(crate) caret_animation_enabled: bool,
    /// Issue #756: 协同动画显式模式。
    ///
    /// Issue #815 评论 5947443780: 协同模式 = 一条 caret 运动轨迹 +
    /// 文字以该轨迹当前帧为吞吐边界 + Reflow 可独立。
    /// - coordinated=true：InsertReveal/DeleteConceal 一律是 `CaretTrack`，没有自己的
    ///   progress；逐帧吞吐边界直接取本事务 cursor track 当前帧的 caret.x。吞字另有
    ///   `DeleteForwardBoundary`（前删时真实 caret 不动，边界自己朝它收拢）。
    /// - coordinated=false：InsertReveal/DeleteConceal 退回独立 `Timed`，按
    ///   typing_animation_enabled 自己推进。
    /// - ReflowMove/ReflowCrossFade 始终独立 `Timed`，两种模式下都不被接管。
    pub(crate) coordinated_animation_enabled: bool,
    pub(crate) composition_commit_crossfade: Option<CompositionCommitCrossfadeSpec>,
}

/// Issue #815 评论 6042062633 修改 8: 把一笔 `VisualEditSpec` 的动画开关/光标几何/
/// 单元种类翻译成 `editor.anim.transaction_skipped` 事件字段。
///
/// 放在 spec 模块而不是 builder 里：事件描述的全部内容都来自 spec 本身，
/// builder 只需要给出 `cause` 和它自己才知道的两个事实
/// （cursor track 是否存在、插入区间）。
pub(crate) fn skip_fields<'a>(
    cause: &'a str,
    spec: &VisualEditSpec,
    unit_kinds: &'a str,
    cursor_track_present: bool,
    inserted_range: Option<(usize, usize)>,
) -> AnimationSkipFields<'a> {
    AnimationSkipFields {
        cause,
        operation_kind:
            crate::sujian_editor_item::animation::transaction_builder::operation_kind_label(
                spec.operation_kind,
            ),
        typing_animation_enabled: spec.text_animation_enabled
            && !spec.coordinated_animation_enabled,
        smooth_cursor_enabled: spec.caret_animation_enabled && !spec.coordinated_animation_enabled,
        coordinated_animation_enabled: spec.coordinated_animation_enabled,
        old_caret_present: spec.old_cursor_rect.is_some(),
        new_caret_present: spec.new_cursor_rect.is_some(),
        inserted_range,
        unit_kinds,
        cursor_track_present,
        is_scrolling: false,
        is_loading: false,
        is_applying_format: false,
        transaction_id: Some(spec.key.transaction_id),
        generation: spec.key.generation,
    }
}
