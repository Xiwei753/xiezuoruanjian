//! Issue #815 评论 5949097065 问题3: 由**同侧** slice 几何生成 caret 吞吐路径。
//!
//! #815 第 1–5 轮把 cursor track 当成"old rect → new rect 一条直线"，跨软换行时
//! 那条直线是从右上飞到左下的对角线：文字拿到的是对角线上的某个 x，而不是
//! "这一行上 caret 走到了哪里"。第 1 轮甚至直接把 `old_cursor_rect.x`
//! （**旧** canonical 坐标）塞进 new snapshot 的 `caret_anchor_x`，导致
//! "输入一个字恰好自动换行"这种最常见的情况从头到尾一个字都不显示。
//!
//! 本模块在 slice 建完、`assign_shared_line_masks` 之后运行：那时每一行的真实
//! 吞吐范围（`line_mask_left` / `line_mask_right`）和行几何
//! （`ingest_line_top` / `ingest_line_bottom`）都已由**本侧** canonical 写好，
//! 路径只能从这些同侧数据生成，绝不跨 layout 混用坐标。
//!
//! 已知边界：IME commit 的 composition crossfade 同时产出 new 侧 InsertReveal 与
//! old 侧 DeleteConceal，两者的行序分属两套 canonical。把它们放进同一条路径就必须
//! 拿两套 revision 的行号互相比较——这正是维护者明令禁止的。因此 composition 这一路
//! 不建路径，逐帧几何继续走 `from → to` fallback，行为与本轮改动前完全一致。

use crate::sujian_editor_item::animated_slice::AnimatedSlice;
use crate::sujian_editor_item::animated_slice::{AnimatedSliceKind, IngestBoundaryDriver};
use crate::sujian_editor_item::animation::transaction::types::{
    CaretTrackSegment, CaretTrackSegmentKind,
};
use crate::sujian_editor_item::animation::transaction_builder::VisualEditSpec;
use crate::sujian_editor_item::edit_motion::CursorRect;

/// 一行在**本侧 canonical** 里的真实吞吐范围与行几何。
///
/// 数据来源是同侧 slice 的 `line_mask_left/right` 与
/// `ingest_line_top/bottom`，不是另一侧 layout 的任何坐标。
#[derive(Clone, Copy, Debug)]
pub(crate) struct IngestRow {
    pub line_ord: usize,
    pub visual_line_id: Option<usize>,
    pub left: f64,
    pub right: f64,
    pub line_top: f64,
    pub line_bottom: f64,
}

impl IngestRow {
    fn caret_rect_at(&self, x: f64) -> CursorRect {
        CursorRect {
            x,
            top: self.line_top,
            bottom: self.line_bottom,
            baseline_y: self.line_bottom,
        }
    }
}

/// 这一批协同吞吐单元的构成。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IngestRouteShape {
    /// 只有 InsertReveal —— new snapshot 的吐字路径。
    InsertOnly,
    /// 只有 DeleteConceal —— old snapshot 的吞字路径。
    DeleteOnly,
    /// InsertReveal 与 DeleteConceal 都有（IME commit composition crossfade）。
    /// 行序分属两套 canonical，不建路径。
    Mixed,
}

/// 从吞吐 slice 里收集吞吐路径经过的每一行。
///
/// 本函数在 **wrap 之前**调用（`build_prepared_transaction` 的顺序是
/// slices → `assign_shared_line_masks` → 建路径 → `build_cursor_visual_track` →
/// wrap units），所以只能看 slice 本身，不能看 `VisualUnitTiming`。
/// 非协同文字动画在 `build_cursor_visual_track` 之后才决定要不要建 track，
/// 协作建不建路径对它们没有影响。
///
/// 只收 InsertReveal / DeleteConceal —— Reflow 不参与吞吐。
/// 同一行多个 cluster 已由 `assign_shared_line_masks` 合并成同一个
/// `line_mask_left/right`，这里按 `ingest_line_ord` 去重并合并范围。
pub(crate) fn collect_ingest_rows(slices: &[AnimatedSlice]) -> Vec<IngestRow> {
    let mut rows: Vec<IngestRow> = Vec::new();
    for slice in slices {
        if !matches!(
            slice.kind,
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
        ) {
            continue;
        }
        let Some(line_ord) = slice.ingest_line_ord else {
            continue;
        };
        let (Some(top), Some(bottom)) = (slice.ingest_line_top, slice.ingest_line_bottom) else {
            continue;
        };
        if let Some(existing) = rows.iter_mut().find(|row| row.line_ord == line_ord) {
            existing.left = existing.left.min(slice.line_mask_left);
            existing.right = existing.right.max(slice.line_mask_right);
            existing.line_top = existing.line_top.min(top);
            existing.line_bottom = existing.line_bottom.max(bottom);
            if existing.visual_line_id.is_none() {
                existing.visual_line_id = slice.visual_line_id;
            }
            continue;
        }
        rows.push(IngestRow {
            line_ord,
            visual_line_id: slice.visual_line_id,
            left: slice.line_mask_left,
            right: slice.line_mask_right,
            line_top: top,
            line_bottom: bottom,
        });
    }
    rows.sort_by_key(|row| row.line_ord);
    rows
}

/// 判定这一批吞吐 slice 的形状。
pub(crate) fn ingest_route_shape(slices: &[AnimatedSlice]) -> IngestRouteShape {
    let mut reveal = false;
    let mut conceal = false;
    for slice in slices {
        match slice.kind {
            AnimatedSliceKind::InsertReveal => reveal = true,
            AnimatedSliceKind::DeleteConceal => conceal = true,
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {}
        }
    }
    match (reveal, conceal) {
        (true, false) => IngestRouteShape::InsertOnly,
        (false, true) => IngestRouteShape::DeleteOnly,
        _ => IngestRouteShape::Mixed,
    }
}

/// 生成 Insert 的吞吐路径（全部坐标来自 new snapshot）。
///
/// ```
/// 当前屏幕 caret ──LayoutHandoff──▶ 新起始行左端
///   ──IngestLine 扫本行──▶ RowHandoff 到下一行 ──IngestLine 扫本行──▶ … ──▶ 新 caret
/// ```
///
/// 开头的 `LayoutHandoff` 是跨 layout 换位（旧坐标 → 新坐标），只移动 caret，
/// 文字一个字都不吞吐；维护者点名的"输入一个字恰好自动换行"就靠它不被裁成对角线。
pub(crate) fn build_insert_route(
    rows: &[IngestRow],
    screen_caret: &CursorRect,
    new_caret: &CursorRect,
) -> Vec<CaretTrackSegment> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut segments = Vec::new();
    let first = rows[0];
    let ingest_start = first.caret_rect_at(first.left);
    segments.push(CaretTrackSegment {
        kind: CaretTrackSegmentKind::LayoutHandoff,
        from: *screen_caret,
        to: ingest_start,
        ingest_line_ord: None,
        visual_line_id: first.visual_line_id,
    });
    let mut cursor = ingest_start;
    for (index, row) in rows.iter().enumerate() {
        let is_last = index + 1 == rows.len();
        // 最后一行的终点直接用 **new snapshot 的新 caret**：插入文本的终点就是 caret。
        let to = if is_last {
            *new_caret
        } else {
            row.caret_rect_at(row.right)
        };
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from: cursor,
            to,
            ingest_line_ord: Some(row.line_ord),
            visual_line_id: row.visual_line_id,
        });
        cursor = to;
    }
    segments
}

/// 生成 Delete 的吞吐路径（全部坐标来自 old snapshot，最后一段换位到 new caret）。
///
/// ```
/// 旧 caret ──IngestLine 吞本行──▶ RowHandoff 到上一行 ──IngestLine 吞本行──▶ …
///   ──▶ old deleted_range.start ──RowHandoff──▶ 新快照最终 caret
/// ```
pub(crate) fn build_delete_route(
    slices: &[AnimatedSlice],
    rows: &[IngestRow],
    old_caret: &CursorRect,
    new_caret: &CursorRect,
) -> Vec<CaretTrackSegment> {
    if rows.is_empty() {
        return Vec::new();
    }
    if is_forward_delete(slices) {
        // 维护者明确要求：前删的真实 caret 本来就不动，不许伪装成"caret 横向移动"。
        // 前删的吞字边界由 `IngestBoundaryDriver::DeleteForwardBoundary` 按本笔 track
        // 的 progress 自己收拢，这里只给光标一段换位路径。
        return vec![CaretTrackSegment {
            kind: CaretTrackSegmentKind::LayoutHandoff,
            from: *old_caret,
            to: *new_caret,
            ingest_line_ord: None,
            visual_line_id: rows.first().and_then(|row| row.visual_line_id),
        }];
    }
    // 退格：路径从**旧 caret 本身**起步（旧快照坐标，不需要换位），
    // 按行序从大到小逐行吞到 deleted_range.start 所在行。
    let mut segments = Vec::new();
    let mut cursor = *old_caret;
    for (index, row) in rows.iter().enumerate().rev() {
        let from = if index == rows.len() - 1 {
            // 起始行：吞字从旧 caret 处开始（被删文字在 caret 左侧）。
            cursor
        } else {
            row.caret_rect_at(row.right)
        };
        let to = row.caret_rect_at(row.left);
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from,
            to,
            ingest_line_ord: Some(row.line_ord),
            visual_line_id: row.visual_line_id,
        });
        if index > 0 {
            // 行与行之间的换位：只移动 caret，上一行保持终态。
            let next_up = &rows[index - 1];
            let step_to = next_up.caret_rect_at(next_up.right);
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: to,
                to: step_to,
                ingest_line_ord: Some(row.line_ord),
                visual_line_id: row.visual_line_id,
            });
        }
        cursor = to;
    }
    let last_ord = rows.first().map(|row| row.line_ord);
    segments.push(CaretTrackSegment {
        kind: CaretTrackSegmentKind::RowHandoff,
        from: cursor,
        to: *new_caret,
        ingest_line_ord: last_ord,
        visual_line_id: rows.first().and_then(|row| row.visual_line_id),
    });
    segments
}

/// 这一批吞吐字是不是"前删"（真实 caret 固定，吞字边界自己收拢）。
fn is_forward_delete(slices: &[AnimatedSlice]) -> bool {
    slices.iter().any(|slice| {
        matches!(slice.kind, AnimatedSliceKind::DeleteConceal)
            && slice.ingest_boundary_driver == IngestBoundaryDriver::DeleteForwardBoundary
    })
}

/// 拿不到 old/new caret 之一时返回空 —— 那种事务本来就建不出 track，
/// `build_cursor_visual_track` 会返回 `None` 并由调用点记 `editor.anim.transaction_skipped`。
pub(crate) fn build_ingest_route(
    spec: &VisualEditSpec,
    slices: &[AnimatedSlice],
) -> Vec<CaretTrackSegment> {
    let rows = collect_ingest_rows(slices);
    if rows.is_empty() {
        return Vec::new();
    }
    match ingest_route_shape(slices) {
        IngestRouteShape::InsertOnly => spec
            .old_cursor_rect
            .as_ref()
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(old_caret, new_caret)| build_insert_route(&rows, old_caret, new_caret))
            .unwrap_or_default(),
        IngestRouteShape::DeleteOnly => spec
            .old_cursor_rect
            .as_ref()
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(old_caret, new_caret)| build_delete_route(slices, &rows, old_caret, new_caret))
            .unwrap_or_default(),
        // Mixed（IME commit composition crossfade）：行序分属两套 canonical，不建路径。
        IngestRouteShape::Mixed => Vec::new(),
    }
}
