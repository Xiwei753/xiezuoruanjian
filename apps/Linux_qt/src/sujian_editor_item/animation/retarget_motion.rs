//! Issue #824 评论 5971089641 第 1 节：只负责「当前屏幕状态 → 最新目标」的正文运动。
//!
//! 本模块是正文吞吐运动（caret + Reveal/Conceal 边界）的**唯一状态模型**：
//!
//! - 只保留当前正在显示的运动，不保存历史输入队列；
//! - 新正文编辑到来时：
//!   1. 用本帧统一采到的当前屏幕 caret / Reveal/Conceal 边界 / 仍可见旧 glyph
//!      作为新起点（[`RetargetStart`]，由 `take_rebase_frames` 在同一个 `now`
//!      上采样）；
//!   2. 根据最新 old/new snapshot + display patches 重建目标
//!      （[`RetargetTarget`]，patch 事实由 `EditorEditResult.display_patches` 派生）；
//!   3. **替换**旧 target —— 不把旧 route 的剩余部分拼到新 route 前面。
//! - [`retarget`] 是明确入口：连续 Insert/Delete/Replace 都走它。
//!
//! `IngestLine / RowHandoff / LayoutHandoff` 只描述几何路径：
//! 段权重由 [`CaretTrackSegment::new`] 按几何路程写入，时间推进由整条 active
//! motion 统一管理（单一全局 progress + 单一 easing，见
//! [`crate::sujian_editor_item::animation::transaction::types::PreparedCursorVisualTrack`]）。
//!
//! 目标（Issue #824）：连续删除无论按多快，活动正文运动都不能随按键次数无限增加。
//! 每次 retarget 只重建一条 route 并替换旧 target；old glyph 的可见几何按本帧采样
//! 保留（无跳变），但绝不把旧 timing / 旧 stage / 旧剩余时长排进新动画。
//!
//! 连续退格时，旧 glyph 与整条 motion 共用同一个 frame_now / 同一段时长收敛到终态：
//! 每次按键只产生本笔自己的 Reveal/Conceal 加“仍可见旧 glyph 的收口”，
//! 不产生 `1、2、3……` 个待播放 DeleteConceal，也不存在历史动画队列。

use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::frame_state::SampledSliceFrame;
use crate::sujian_editor_item::animation::transaction::types::{
    CaretTrackSegment, CaretTrackSegmentKind, IngestSnapshotSide, IngestStageId,
};
use crate::sujian_editor_item::animation::transaction_builder::ingest_route::{
    build_delete_route, build_insert_route, collect_delete_rows, collect_insert_rows,
    ingest_route_shape, same_rect, IngestRouteShape,
};
use crate::sujian_editor_item::animation::VisualUnitTiming;
use crate::sujian_editor_item::edit_motion::{CursorRect, EditorAnimationKind};
use crate::sujian_editor_item::editor_animation_debug_log;

/// 本笔正文修改的 patch 类型（诊断 + 决策）。
///
/// Issue #824 评论 5971089641 第 2 节：只来自 display patches 的 inserted/deleted
/// 事实，不从 `operation_kind` 派生。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetargetPatchKind {
    Insert,
    Delete,
    Replace,
    CursorOnly,
}

impl RetargetPatchKind {
    pub(crate) fn from_kind(kind: EditorAnimationKind) -> Self {
        match kind {
            EditorAnimationKind::Insert => RetargetPatchKind::Insert,
            EditorAnimationKind::Delete => RetargetPatchKind::Delete,
            EditorAnimationKind::Replace => RetargetPatchKind::Replace,
            EditorAnimationKind::CursorOnly => RetargetPatchKind::CursorOnly,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            RetargetPatchKind::Insert => "Insert",
            RetargetPatchKind::Delete => "Delete",
            RetargetPatchKind::Replace => "Replace",
            RetargetPatchKind::CursorOnly => "CursorOnly",
        }
    }
}

/// 本帧从活动运动采到的当前屏幕起点。
///
/// 由 `take_rebase_frames` 在统一 `now` 上采样：`caret` 是当前屏幕上 caret 的真实
/// 文档坐标，`visual_line_id` / `line_top` / `line_bottom` 是同一帧的行身份与行几何。
/// 没有历史 motion 时，调用方用逻辑 old caret 充当起点（第一条 motion）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetargetStart {
    pub caret: CursorRect,
    pub visual_line_id: Option<usize>,
    pub line_top: f64,
    pub line_bottom: f64,
}

/// 最新目标（由最新 old/new snapshot + display patches 决定）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetargetTarget {
    pub caret: CursorRect,
    pub visual_line_id: Option<usize>,
}

/// [`retarget`] 的完整输入。所有诊断字段与几何输入一起传入，避免 retarget 里
/// 再去查外部状态。
pub(crate) struct RetargetRequest<'a> {
    /// 本帧采样到的当前屏幕起点。
    pub start: RetargetStart,
    /// 最新目标。
    pub target: RetargetTarget,
    /// 本笔编辑的吞吐 slice（InsertReveal/DeleteConceal），几何路径只从它们生成。
    pub slices: &'a [AnimatedSlice],
    /// 本笔 active motion 的 stage id。
    pub stage_id: IngestStageId,
    /// patch 类型（诊断 + Mixed 形状判定参考）。
    pub patch_kind: RetargetPatchKind,
    /// 被替换掉的旧 motion id（诊断）。新模型的 route 不带旧 stage。
    pub replaced_stage_id: Option<IngestStageId>,
    /// 本帧仍可见的新增 Reveal 数量（旧 glyph 采样 + 本笔新 slice）。
    pub active_reveal_count: usize,
    /// 本帧仍可见的 Conceal 数量。
    pub active_conceal_count: usize,
}

/// 正文 retarget 的唯一入口：当前屏幕采样 → 最新目标。
///
/// 连续 Insert/Delete/Replace（含 IME commit、粘贴）都走这里。
/// 返回**全新**的几何路径；旧 route 的剩余段一律丢弃，不参与拼接。
pub(crate) fn retarget(request: RetargetRequest<'_>) -> Vec<CaretTrackSegment> {
    let delete_rows = collect_delete_rows(request.slices);
    let insert_rows = collect_insert_rows(request.slices);
    let segments = if delete_rows.is_empty() && insert_rows.is_empty() {
        Vec::new()
    } else {
        match ingest_route_shape(request.slices) {
            IngestRouteShape::InsertOnly => {
                if insert_rows.is_empty() {
                    Vec::new()
                } else {
                    build_insert_route(
                        &insert_rows,
                        &request.start.caret,
                        &request.target.caret,
                        request.stage_id,
                    )
                }
            }
            IngestRouteShape::DeleteOnly => {
                if delete_rows.is_empty() {
                    Vec::new()
                } else {
                    build_delete_route(
                        &delete_rows,
                        &request.start.caret,
                        Some(&request.target.caret),
                        request.stage_id,
                    )
                }
            }
            // Issue #815 评论 5950887715: Mixed（IME commit 候选 Reveal +
            // 旧 preedit Conceal 同帧）不是退化路径：先吞旧（Old），再吐新（New）。
            IngestRouteShape::Mixed => {
                match (delete_rows.first().copied(), insert_rows.first().copied()) {
                    (Some(first_delete), Some(first_insert)) => {
                        // old 侧吞字（末尾换位段由下面统一接）。
                        let mut mixed = build_delete_route(
                            &delete_rows,
                            &request.start.caret,
                            None,
                            request.stage_id,
                        );
                        // old 吞完 → 切到 new 侧 candidate 起点。
                        let old_route_end = mixed
                            .last()
                            .map(|segment| segment.to)
                            .unwrap_or(request.start.caret);
                        let insert_start = first_insert.caret_rect_at(first_insert.left);
                        if !same_rect(&old_route_end, &insert_start) {
                            mixed.push(CaretTrackSegment::new(
                                CaretTrackSegmentKind::RowHandoff,
                                old_route_end,
                                insert_start,
                                Some(first_delete.line_ord),
                                Some(IngestSnapshotSide::Old),
                                first_delete.visual_line_id,
                                request.stage_id,
                            ));
                        }
                        // new 侧吐字。
                        mixed.extend(build_insert_route(
                            &insert_rows,
                            &insert_start,
                            &request.target.caret,
                            request.stage_id,
                        ));
                        mixed
                    }
                    _ => Vec::new(),
                }
            }
        }
    };

    record_retarget_diagnostic(&request, &segments);
    segments
}

/// 本帧仍可见的吞吐 slice 计数（Reveal, Conceal），供 retarget 诊断使用。
pub(crate) fn count_ingest_slices(slices: &[AnimatedSlice]) -> (usize, usize) {
    let mut reveal = 0usize;
    let mut conceal = 0usize;
    for slice in slices {
        match slice.kind {
            AnimatedSliceKind::InsertReveal => reveal += 1,
            AnimatedSliceKind::DeleteConceal => conceal += 1,
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {}
        }
    }
    (reveal, conceal)
}

/// 本帧仍可见旧 glyph（carried units）的吞吐计数（Reveal, Conceal）。
///
/// Issue #824 评论 5971089641 第 9 节：retarget 诊断要报“当前 active
/// Reveal/Conceal 数量”——本笔新 slice 加上还没收口完的旧 glyph 一起算。
pub(crate) fn count_carried_ingest_units(
    units: &[crate::sujian_editor_item::animation::frame_state::CarriedVisualUnit],
) -> (usize, usize) {
    let mut reveal = 0usize;
    let mut conceal = 0usize;
    for unit in units {
        match unit.slice.kind {
            AnimatedSliceKind::InsertReveal => reveal += 1,
            AnimatedSliceKind::DeleteConceal => conceal += 1,
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {}
        }
    }
    (reveal, conceal)
}

/// Issue #824 评论 5972388049：把仍在由 caret track 驱动的吞吐 slice 转成
/// 「从当前屏幕帧继续到终态」的 Timed 收口状态。
///
/// 两条路径共用本 helper，避免各写一套转换：
/// - `transaction_builder` 的 carried retarget：旧 glyph 带进新事务继续收口；
/// - `coordinator` 的 PointerClick caret detach：旧事务原地从 caret 驱动解绑，
///   文字既不掐到终态、也不从 0 重播。
///
/// 语义：
/// - 就地修正 slice 的遮罩锚点，使 `compute_frame(start_fraction)` 复现采样帧几何；
/// - `start_fraction` = 当前屏幕可见比例（不是 0，也不是终态）；
/// - `target_fraction` = Reveal 1 / Conceal 0；
/// - `started_at` 由调用方给：新事务用 `None`（等 Rendering 与整条 motion 同帧起跑），
///   mid-flight detach 用 `Some(now)`（从 detach 的同一帧继续）。
pub(crate) fn detach_caret_track_to_timed(
    slice: &mut AnimatedSlice,
    sampled: &SampledSliceFrame,
    duration_ms: u64,
    started_at: Option<Instant>,
) -> VisualUnitTiming {
    // 旧 `is_caret_line` 让 Timed 的遮罩锚点跟着**旧** caret 走，而 caret-driven
    // 的锚点由 driver 决定（Backspace 收向行左端 / Delete 键从被删区右端收向静止
    // caret）。脱离 caret 驱动后按采样帧的固定边重设收拢侧，保证反解出的 visible
    // 同时复现采样帧的位置与宽度。
    slice.is_caret_line = false;
    if slice.kind == AnimatedSliceKind::DeleteConceal {
        let frame = &sampled.dest_rect;
        // 固定边相对**本 slice 自己的矩形**判断：行级 mask 通常比 slice 宽，
        // 直接比 mask 会把「靠 slice 左端」误判成「靠行右端」。
        let slice_left = slice.from_document_rect.x;
        let slice_right = slice.from_document_rect.x + slice.from_document_rect.w;
        let left_gap = (frame.x - slice_left).abs();
        let right_gap = (frame.x + frame.w - slice_right).abs();
        slice.conceal_to_left_edge = left_gap <= right_gap;
    }
    let start_fraction = caret_track_sampled_visible_fraction(slice, sampled);
    VisualUnitTiming::Timed {
        started_at,
        duration_ms: duration_ms.max(1),
        start_fraction,
        target_fraction: match slice.kind {
            AnimatedSliceKind::DeleteConceal => 0.0,
            _ => 1.0,
        },
    }
}

/// caret-driven 采样帧的当前可见比例（0..1）。
///
/// `SampledSliceFrame.visible_fraction` 对 CaretTrack 单元固定是 0（它的边界来自
/// caret 几何，不是 0..1 进度）。脱离 caret 驱动的瞬间必须等于「当前屏幕」，所以要
/// 反解一个 `visible`，使 `compute_frame(visible)` 复现采样帧的绘制宽度。
/// `compute_frame` 的可见宽度对 `visible` 单调（从锚点向行级 extent 展开），
/// 因此用二分求逆；48 次二分 → 精度 ~1e-15，与采样帧在 1e-8 容差内一致。
fn caret_track_sampled_visible_fraction(slice: &AnimatedSlice, sampled: &SampledSliceFrame) -> f64 {
    match slice.kind {
        AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
            let target_w = sampled.dest_rect.w.max(0.0);
            let mut low = 0.0f64;
            let mut high = 1.0f64;
            for _ in 0..48 {
                let mid = 0.5 * (low + high);
                if slice.compute_frame(mid).w < target_w {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            0.5 * (low + high)
        }
        AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
            sampled.visible_fraction
        }
    }
}

/// Issue #824 评论 5971089641 第 9 节：正文 retarget 的正式诊断事件。
///
/// 字段：patch 类型、sampled current position、new target、replaced motion id、
/// 当前 active Reveal/Conceal 数量、重建出的段数。写 `writer_diagnostics`
/// 正式事件（非 env-gated debug log），诊断包里可直接看到每次 retarget 的事实。
fn record_retarget_diagnostic(request: &RetargetRequest<'_>, segments: &[CaretTrackSegment]) {
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert(
        "patch_kind".to_string(),
        serde_json::json!(request.patch_kind.label()),
    );
    fields.insert(
        "sampled_current_position".to_string(),
        serde_json::json!([request.start.caret.x, request.start.caret.top]),
    );
    fields.insert(
        "sampled_visual_line_id".to_string(),
        serde_json::json!(request.start.visual_line_id),
    );
    fields.insert(
        "sampled_line_geometry".to_string(),
        serde_json::json!([request.start.line_top, request.start.line_bottom]),
    );
    fields.insert(
        "new_target".to_string(),
        serde_json::json!([request.target.caret.x, request.target.caret.top]),
    );
    fields.insert(
        "target_visual_line_id".to_string(),
        serde_json::json!(request.target.visual_line_id),
    );
    fields.insert(
        "replaced_motion_id".to_string(),
        serde_json::json!(request.replaced_stage_id.map(|id| id.0)),
    );
    fields.insert(
        "active_reveal_count".to_string(),
        serde_json::json!(request.active_reveal_count),
    );
    fields.insert(
        "active_conceal_count".to_string(),
        serde_json::json!(request.active_conceal_count),
    );
    fields.insert(
        "segment_count".to_string(),
        serde_json::json!(segments.len()),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "editor.anim.retarget".to_string(),
        target: "editor.anim".to_string(),
        message: Some(format!(
            "Issue #824 评论 5971089641: 正文运动重定向（patch={}），替换旧运动 {:?}，\
             当前 visible reveal/conceal = {}/{}",
            request.patch_kind.label(),
            request.replaced_stage_id,
            request.active_reveal_count,
            request.active_conceal_count,
        )),
        fields,
    });
    editor_animation_debug_log(&format!(
        "anim_retarget: patch={} sampled=({:.1},{:.1}) target=({:.1},{:.1}) \
         replaced={:?} reveal/conceal={}/{} segments={}",
        request.patch_kind.label(),
        request.start.caret.x,
        request.start.caret.top,
        request.target.caret.x,
        request.target.caret.top,
        request.replaced_stage_id,
        request.active_reveal_count,
        request.active_conceal_count,
        segments.len(),
    ));
}
