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

/// 两个 caret 位置的几何是否完全相同。
///
/// Issue #815 评论 5950375533 问题4: 所有 segment 均分总时长，所以一个 0 长度的
/// segment 会白白吃掉一半动画——普通同行输入前一半时间一个字都不吐，
/// 体感就是"打字慢半拍"。几何相同就不该生成这个 segment。
fn same_rect(a: &CursorRect, b: &CursorRect) -> bool {
    a.x == b.x && a.top == b.top && a.bottom == b.bottom
}

/// 生成 Insert 的吞吐路径（全部坐标来自 new snapshot）。
///
/// ```
/// 当前屏幕 caret ──LayoutHandoff（仅在跨 layout 时）──▶ 新起始行左端
///   ──IngestLine 扫本行──▶ RowHandoff 到下一行左端 ──IngestLine 扫本行──▶ …
/// ```
///
/// Issue #815 评论 5950375533 问题1: 原实现扫完第一行后，让第二行的 `IngestLine`
/// 直接从上一行右端连到本行右端/新 caret，把一条**行间斜线**标成了
/// `IngestLine(本行)` —— 正是第 6 轮明令禁止的"行间斜线 x 冒充本行吞吐边界"。
/// 现在每个非最后一行扫完后都显式 push 一个 `RowHandoff`，下一条 `IngestLine`
/// 的起点固定取**本行左端**，绝不继承上一行右端。
///
/// Issue #815 评论 5950375533 问题4: `screen_caret` 与吞吐起点几何相同时
/// 不生成 0 长度的 `LayoutHandoff`，直接从 `IngestLine` 开始。
pub(crate) fn build_insert_route(
    rows: &[IngestRow],
    screen_caret: &CursorRect,
    new_caret: &CursorRect,
) -> Vec<CaretTrackSegment> {
    let Some(first) = rows.first().copied() else {
        return Vec::new();
    };
    let ingest_start = first.caret_rect_at(first.left);
    let mut segments = Vec::new();
    let mut cursor = *screen_caret;
    if !same_rect(&cursor, &ingest_start) {
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::LayoutHandoff,
            from: cursor,
            to: ingest_start,
            ingest_line_ord: None,
            visual_line_id: first.visual_line_id,
        });
    }
    for (index, row) in rows.iter().enumerate() {
        let is_last = index + 1 == rows.len();
        // 每条 IngestLine 的起点必须是**本行左端**。
        cursor = row.caret_rect_at(row.left);
        let to = if is_last {
            // 最后一行的终点直接用 **new snapshot 的新 caret**：插入文本的终点就是 caret。
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
        if !is_last {
            let next = rows[index + 1];
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: to,
                to: next.caret_rect_at(next.left),
                // 刚扫完的行 → 该行保持终态，后面的行还没被碰到。
                ingest_line_ord: Some(row.line_ord),
                visual_line_id: row.visual_line_id,
            });
        }
    }
    segments
}

/// 生成 Delete 的吞吐路径（吞字坐标来自 old snapshot，最后一段换位到 new caret）。
///
/// ```
/// 旧屏幕 caret ──IngestLine 吞本行──▶ RowHandoff 到上一行左端 ──IngestLine 吞本行──▶ …
///   ──▶ old deleted_range.start ──RowHandoff──▶ 新快照最终 caret
/// ```
pub(crate) fn build_delete_route(
    slices: &[AnimatedSlice],
    rows: &[IngestRow],
    screen_caret: &CursorRect,
    new_caret: &CursorRect,
) -> Vec<CaretTrackSegment> {
    let Some(first_row) = rows.first().copied() else {
        return Vec::new();
    };
    if is_forward_delete(slices) {
        // Issue #815 评论 5950375533 问题2: 前删的真实 caret 本来就不动。
        //
        // 原实现把它做成 `LayoutHandoff(old → new)`，而 `LayoutHandoff` 的正式语义是
        // `is_ingest_segment=false + ingest_line_ord=None ⇒ RouteBeforeStart ⇒
        // 所有吞吐 slice 保持初态`。于是虽然 `DeleteForwardBoundary` 和 track
        // progress 都还在，文字层根本进不到边界收拢逻辑，旧字整段动画期间保持完整，
        // track 结束后才突然被 retire 收掉。
        //
        // 现在给它一条**静止的吞吐段**：`from == to == 屏幕 caret`，
        // `ingest_line_ord = 当前删除行` ⇒ `is_ingest_segment=true`，
        // `DeleteForwardBoundary` 才能消费 track progress 把边界从被删区右端
        // 收拢到 caret。
        return vec![CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from: *screen_caret,
            to: *screen_caret,
            ingest_line_ord: Some(first_row.line_ord),
            visual_line_id: first_row.visual_line_id,
        }];
    }
    // 退格：路径从**当前屏幕 caret** 起步，按行序从大到小逐行吞到
    // deleted_range.start 所在行。
    //
    // Issue #815 评论 5950677031 问题1: 原来这里直接 `for ... in rows.iter().enumerate().rev()`，
    // 第一条 `IngestLine` 的起点固定写成 `row.caret_rect_at(row.right)`，
    // `screen_caret` 参数完全没被消费。于是 `build_ingest_route()` 虽然已经正确优先选了
    // `caret_handoff.sampled`，传进来以后又被丢掉：
    // `handoff.sampled -> build_delete_route(screen_caret=真实位置) -> 第一段仍从逻辑
    // 删除区 row.right 起步` —— 快速连续退格仍然会从上一帧真实 caret 瞬移回去。
    //
    // 现在：真正的 old-side 吞吐起点是**最后一条吞字行**（Backspace 起始行）的
    // `row.right`。`screen_caret` 与它不同就先插一段纯几何换位 `LayoutHandoff`
    // （`ingest_line_ord = None` / `is_ingest_segment = false`，不吞字）；
    // 相同就直接从第一条 `IngestLine` 开始，不白占时长。
    let start_row = rows.last().copied().unwrap_or(first_row);
    let ingest_start = start_row.caret_rect_at(start_row.right);
    let mut segments = Vec::new();
    if !same_rect(screen_caret, &ingest_start) {
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::LayoutHandoff,
            from: *screen_caret,
            to: ingest_start,
            ingest_line_ord: None,
            visual_line_id: start_row.visual_line_id,
        });
    }
    for (index, row) in rows.iter().enumerate().rev() {
        // 起点固定取**本行右端**，绝不继承上一行吞完后的位置。
        let from = row.caret_rect_at(row.right);
        let to = row.caret_rect_at(row.left);
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from,
            to,
            ingest_line_ord: Some(row.line_ord),
            visual_line_id: row.visual_line_id,
        });
        if index > 0 {
            // 行与行之间的换位：只移动 caret，刚吞完的行保持终态。
            let next_up = &rows[index - 1];
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: to,
                // Issue #815 评论 5950677031 问题2: 终点必须是上一行**右端**。
                // 原来写成 `next_up.left`，而紧接着的 `IngestLine(next_up)` 从
                // `next_up.right` 起步 —— 两段在边界处不连续，采样切过去那一帧会
                // 从上一行左端瞬移到右端。正确的退格路线是
                // `当前行 right -> left (IngestLine)`、
                // `当前行 left -> 上一行 right (RowHandoff)`、
                // `上一行 right -> left (IngestLine)`。
                to: next_up.caret_rect_at(next_up.right),
                ingest_line_ord: Some(row.line_ord),
                visual_line_id: row.visual_line_id,
            });
        }
    }
    // Issue #815 评论 5950375533 问题4: old 侧吞字终点与 new caret 完全相同时
    // 不生成这一段，否则普通单字符退格会白白把一半时长花在 0 位移上。
    let swallow_end = first_row.caret_rect_at(first_row.left);
    if !same_rect(&swallow_end, new_caret) {
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::RowHandoff,
            from: swallow_end,
            to: *new_caret,
            ingest_line_ord: Some(first_row.line_ord),
            visual_line_id: first_row.visual_line_id,
        });
    }
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
    // Issue #815 评论 5950375533 问题3: 路径的**屏幕起点**必须优先用 handoff
    // （上一帧真正画出来的 caret 位置），拿不到才退回逻辑 `old_cursor_rect`。
    //
    // 原实现固定用 `spec.old_cursor_rect`，而 `build_cursor_visual_track` 拿到的
    // `handoff.sampled` 只写进顶层 `track.from`；只要 `segments` 非空，
    // `sampled_rect_at_progress()` 根本不读 `track.from`，读的是 `segments[0].from`
    // —— 于是快速连续输入时渲染又跳回逻辑 old caret。
    let screen_caret = spec
        .caret_handoff
        .as_ref()
        .map(|handoff| &handoff.sampled)
        .or(spec.old_cursor_rect.as_ref());
    match ingest_route_shape(slices) {
        IngestRouteShape::InsertOnly => screen_caret
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(screen_caret, new_caret)| build_insert_route(&rows, screen_caret, new_caret))
            .unwrap_or_default(),
        IngestRouteShape::DeleteOnly => screen_caret
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(screen_caret, new_caret)| {
                build_delete_route(slices, &rows, screen_caret, new_caret)
            })
            .unwrap_or_default(),
        // Mixed（IME commit composition crossfade）：行序分属两套 canonical，不建路径。
        IngestRouteShape::Mixed => Vec::new(),
    }
}
