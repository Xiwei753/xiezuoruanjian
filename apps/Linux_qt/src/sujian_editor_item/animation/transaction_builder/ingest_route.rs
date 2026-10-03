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
//! Issue #815 评论 5950887715: IME commit 的 composition crossfade 同时产出
//! new 侧 `InsertReveal` 与 old 侧 `DeleteConceal`。此前这里直接放弃（返回空
//! route），理由是两套 canonical 的行号不能互相比大小——于是 IME 又退回 old→new
//! 一条斜线，正是前几轮刚从普通 Insert/Delete 里删掉的旧问题。
//!
//! 现在的解法不是放弃，而是给每段加 `IngestSnapshotSide`：old 侧段带 `Old`、
//! new 侧段带 `New`、最前置纯几何换位带 `None`。行序只在**同一 side 内**比较，
//! Mixed 路径按「先吞旧 preedit（Old），再吐新 candidate（New）」分段拼接，
//! 全程不产生跨 snapshot ordinal 比较。

use crate::sujian_editor_item::animated_slice::AnimatedSlice;
use crate::sujian_editor_item::animated_slice::{AnimatedSliceKind, IngestBoundaryDriver};
use crate::sujian_editor_item::animation::transaction::types::{
    CaretTrackSegment, CaretTrackSegmentKind, IngestSnapshotSide, IngestStageId,
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
    /// Issue #815 评论 5953049681 问题3: 这一行吞字用哪种驱动，必须**按行**决定。
    ///
    /// `CaretPosition`：真实 caret 会横扫这一行。
    /// `DeleteForwardBoundary`：真实 caret 不动，边界靠本段的 local
    /// `ingest_progress` 自行收拢。
    ///
    /// 之前是事务级 `any()`：只要有一片是前删，整条 old 侧 route 就缩成一个静止段。
    /// 但 composition crossfade 的 `conceal_to_left_edge` 是按**每个 old cluster**
    /// 相对 `new_cursor_rect.x` 单独算的，跨行时 x 会从行首重新开始，同一批
    /// preedit slice 完全可能一行是前删、另一行是退格。
    pub driver: IngestBoundaryDriver,
    /// 该行出现 driver 冲突时为 `true`（构造期已记 invariant diagnostic）。
    pub driver_conflict: bool,
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
            if existing.driver != slice.ingest_boundary_driver {
                // Issue #815 评论 5953049681 问题3: 同一行同时出现两种 driver 是
                // 数据异常（正常情况下同一行的 cluster 方向应当一致）。这里显式
                // 标记，绝不静默拿任意一侧的结论去驱动整行。
                existing.driver_conflict = true;
                record_ingest_row_driver_conflict(existing.line_ord);
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
            driver: slice.ingest_boundary_driver,
            driver_conflict: false,
        });
    }
    rows.sort_by_key(|row| row.line_ord);
    rows
}

/// Issue #815 评论 5953049681 问题3: 同一吞字行同时出现两种 driver 的 invariant
/// 诊断。日志只暴露原因（`cause`），真实要求仍是"前删与退格各自按行驱动"。
fn record_ingest_row_driver_conflict(line_ord: usize) {
    // Issue #815 评论 5954004872: 这是一条 **invariant** 违例，光打 debug log 不算
    // 说过——必须进正式 `writer_diagnostics`，否则事后没人能查得到"哪一行退化成
    // 只听一侧 driver"。保留第一个 driver 作为异常兜底（行为上退化成听一侧），
    // 但事件里必须留下事实。
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("line_ord".to_string(), serde_json::json!(line_ord));
    fields.insert(
        "drivers".to_string(),
        serde_json::json!(format!(
            "{:?}+{:?}",
            IngestBoundaryDriver::CaretPosition,
            IngestBoundaryDriver::DeleteForwardBoundary
        )),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Warn,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "editor.anim.ingest_row_driver_conflict".to_string(),
        target: "editor.anim".to_string(),
        message: Some(format!(
            "Issue #815 评论 5954004872: 吞字行 {line_ord} 同时出现 CaretPosition 与 DeleteForwardBoundary，按行驱动会退化成只听一侧"
        )),
        fields,
    });
    crate::sujian_editor_item::editor_animation_debug_log(&format!(
        "Issue #815 评论 5954004872: 吞字行 {line_ord} driver 冲突，已按第一个 driver 兜底并记 editor.anim.ingest_row_driver_conflict"
    ));
}

/// Issue #815 评论 5950887715: 只收 old 侧 `DeleteConceal` 的吞吐行。
///
/// 行序来自 **old snapshot**。Mixed（IME 同时 Reveal + Conceal）路径下
/// old / new 的 `VisualLine.id` 每次排版都从 0 重编、是两套互不相干的坐标系，
/// 绝不能互相比较，所以两侧必须分开收集。
pub(crate) fn collect_delete_rows(slices: &[AnimatedSlice]) -> Vec<IngestRow> {
    collect_ingest_rows(
        &slices
            .iter()
            .filter(|slice| slice.kind == AnimatedSliceKind::DeleteConceal)
            .cloned()
            .collect::<Vec<_>>(),
    )
}

/// Issue #815 评论 5950887715: 只收 new 侧 `InsertReveal` 的吞吐行。
///
/// 行序来自 **new snapshot**。
pub(crate) fn collect_insert_rows(slices: &[AnimatedSlice]) -> Vec<IngestRow> {
    collect_ingest_rows(
        &slices
            .iter()
            .filter(|slice| slice.kind == AnimatedSliceKind::InsertReveal)
            .cloned()
            .collect::<Vec<_>>(),
    )
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
    stage_id: IngestStageId,
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
            // 最前置的纯几何换位还没进入任何一侧的吞吐。
            ingest_side: None,
            visual_line_id: first.visual_line_id,
            ingest_stage_id: stage_id,
        });
    }
    for (index, row) in rows.iter().enumerate() {
        // 每条 IngestLine 的起点必须是**本行左端**。
        cursor = row.caret_rect_at(row.left);
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from: cursor,
            // Issue #815 评论 5954004872 问题2: IngestLine **只能在本行内移动**，
            // 终点固定是本行右端。
            //
            // 原来最后一行直接 `to = *new_caret`，这只在「最后一个可见字符和最终
            // caret 同一行」时成立。而 `build_insert_reveal_slices()` 会跳过纯空格 /
            // 制表 / 换行 / 控制字符——比如粘贴 `abc\n`：`abc` 在当前行产生最后一条
            // 可见 InsertReveal，`\n` 不产生任何 slice，而 `new_caret` 已经在下一行行首。
            // 于是原实现发出 `IngestLine(当前行): 当前行 left -> 下一行 new_caret`，
            // 把一条跨行的斜线/竖直换位标成了吞吐段，文字层会把这个 x 当成本行的吞吐
            // 边界——正是前几轮反复禁止的那件事。
            to: row.caret_rect_at(row.right),
            ingest_line_ord: Some(row.line_ord),
            ingest_side: Some(IngestSnapshotSide::New),
            visual_line_id: row.visual_line_id,
            ingest_stage_id: stage_id,
        });
        if index + 1 < rows.len() {
            let next = rows[index + 1];
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: row.caret_rect_at(row.right),
                to: next.caret_rect_at(next.left),
                // 刚扫完的行 → 该行保持终态，后面的行还没被碰到。
                ingest_line_ord: Some(row.line_ord),
                ingest_side: Some(IngestSnapshotSide::New),
                visual_line_id: row.visual_line_id,
                ingest_stage_id: stage_id,
            });
        }
    }
    // Issue #815 评论 5954588641 问题1: 这段 Tail handoff 发生在**所有** Insert 行
    // 已吐完之后，"刚扫完的行"是 `rows.last()`，不是 `rows.first()`。
    //
    // 原来标成 `first.line_ord` 时，多行 Insert（0 → 1 都吐完，末尾换行再换位到
    // line 2 的 caret）的 tail frame 会报告 `sampled_ingest_line_ord = 0`，
    // `ingest_phase_from_route_ord()` 于是把 line 1 判成还在 row 0「前面」
    // ⇒ `NotReached` ⇒ **刚吐完的 line 1 在最终 handoff 阶段重新隐藏**，
    // 到事务 retire 才又跳回最终静态内容，形成末尾闪回。
    let row_end = segments
        .last()
        .map(|segment| segment.to)
        .unwrap_or(ingest_start);
    if !same_rect(&row_end, new_caret) {
        let final_row = rows.last().copied().expect("rows 非空");
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::RowHandoff,
            from: row_end,
            to: *new_caret,
            ingest_line_ord: Some(final_row.line_ord),
            ingest_side: Some(IngestSnapshotSide::New),
            visual_line_id: final_row.visual_line_id,
            ingest_stage_id: stage_id,
        });
    }
    segments
}

/// 生成 Delete 的吞吐路径（吞字坐标来自 old snapshot，最后一段换位到 new caret）。
///
/// ```
/// 旧屏幕 caret ──IngestLine 吞本行──▶ RowHandoff 到上一行左端 ──IngestLine 吞本行──▶ …
///   ──▶ old deleted_range.start ──RowHandoff──▶ 新快照最终 caret
/// ```
/// Issue #815 评论 5954004872 问题1: 一行吞字的**段起点**。
///
/// `CaretPosition`（Backspace）真实 caret 自己横扫本行，从本行右端起步；
/// `DeleteForwardBoundary`（Delete 键）真实 caret 不动，边界从被删区左端起收，
/// 所以段起点就是本行左端。两处推断必须走同一个入口，不能各写各的。
fn row_ingest_start(row: &IngestRow) -> CursorRect {
    match row.driver {
        IngestBoundaryDriver::CaretPosition => row.caret_rect_at(row.right),
        IngestBoundaryDriver::DeleteForwardBoundary => row.caret_rect_at(row.left),
    }
}

/// Issue #815 评论 5954004872 问题1: 一行吞字的段终点。两种 driver 都收拢到本行左端。
fn row_ingest_end(row: &IngestRow) -> CursorRect {
    row.caret_rect_at(row.left)
}

pub(crate) fn build_delete_route(
    rows: &[IngestRow],
    screen_caret: &CursorRect,
    tail: Option<&CursorRect>,
    stage_id: IngestStageId,
) -> Vec<CaretTrackSegment> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut segments = Vec::new();
    // Issue #815 评论 5954004872 问题1: 退格的**起始行**是行序最大的那行
    // （`rows` 按 `line_ord` 升序），不是 `rows.first()`。
    //
    // 原实现先判 `first_row.driver == DeleteForwardBoundary` 就整条早退成一段静止
    // 段，把 old 侧其余所有行一起吞掉；而且 `first_row` 与注释里写的
    // 「第一行（Backspace 的起始行）」本来就是反的概念。现在没有早退：每一行都按
    // 自己的 driver 决定从哪起步，往上逐行吞。
    let start_row = rows.last().copied().expect("rows 非空");
    let ingest_start = row_ingest_start(&start_row);
    if !same_rect(screen_caret, &ingest_start) {
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::LayoutHandoff,
            from: *screen_caret,
            to: ingest_start,
            ingest_line_ord: None,
            // 纯几何换位，还没进入 old 侧吞吐。
            ingest_side: None,
            visual_line_id: start_row.visual_line_id,
            ingest_stage_id: stage_id,
        });
    }
    for (index, row) in rows.iter().enumerate().rev() {
        segments.push(CaretTrackSegment {
            kind: CaretTrackSegmentKind::IngestLine,
            from: row_ingest_start(row),
            to: row_ingest_end(row),
            ingest_line_ord: Some(row.line_ord),
            ingest_side: Some(IngestSnapshotSide::Old),
            visual_line_id: row.visual_line_id,
            ingest_stage_id: stage_id,
        });
        if index > 0 {
            let next_up = &rows[index - 1];
            // Issue #815 评论 5954004872 问题1: 行间换位的终点必须等于**下一段实际的
            // from**，也就是 `row_ingest_start(next_up)`。原来写死 `next_up.right`，
            // 而下一行的 `IngestLine` 在 `next_up` 是 DeleteForwardBoundary 时从
            // `next_up.left` 起步 —— 相邻两段会瞬移。
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: row_ingest_end(row),
                to: row_ingest_start(next_up),
                ingest_line_ord: Some(row.line_ord),
                ingest_side: Some(IngestSnapshotSide::Old),
                visual_line_id: row.visual_line_id,
                ingest_stage_id: stage_id,
            });
        }
    }
    // Issue #815 评论 5954004872 问题1: 末尾换位必须接**最后一条实际生成段的终点**，
    // 不再重算 `first_row.left`。
    let swallow_end = segments
        .last()
        .map(|segment| segment.to)
        .unwrap_or(*screen_caret);
    if let Some(tail_target) = tail {
        if !same_rect(&swallow_end, tail_target) {
            // Issue #815 评论 5954588641 问题2: Delete 是从**大行序往小行序**吞，
            // `start_row = rows.last()` 是**最开始吞的那一行**，不是最后吞完的。
            // 全部 Delete 行吞完之后，真正刚扫完的是 `rows.first()`。
            //
            // 原来标成 `start_row.line_ord` 时，Backspace 从 line 2 一路吞到 line 0
            // 再换位到 new caret 的 tail frame 会报告 sampled old ord = 2；对方向
            // `2 -> 0`，`ingest_phase_from_route_ord()` 把 line 1 / 0 判成
            // `NotReached` ⇒ **已经吞掉的低行序旧字在最终 handoff 阶段重新出现**。
            //
            // 注意：`swallow_end` 继续取 `segments.last().to` 是对的，只改 tail 的
            // phase identity，不退回重算几何。
            let final_row = rows.first().copied().expect("rows 非空");
            segments.push(CaretTrackSegment {
                kind: CaretTrackSegmentKind::RowHandoff,
                from: swallow_end,
                to: *tail_target,
                ingest_line_ord: Some(final_row.line_ord),
                ingest_side: Some(IngestSnapshotSide::Old),
                visual_line_id: final_row.visual_line_id,
                ingest_stage_id: stage_id,
            });
        }
    }
    segments
}

/// 拿不到 old/new caret 之一时返回空 —— 那种事务本来就建不出 track，
/// `build_cursor_visual_track` 会返回 `None` 并由调用点记 `editor.anim.transaction_skipped`。
///
/// Issue #819 评论 5968931455: 合成 `旧 carried route 剩余段 -> 当前新 route`，
/// 让 carried CaretTrack unit 消费自己原来的剩余 route，而不是被迫消费下一笔编辑的 route。
///
/// 旧剩余段来自 `spec.visual_state.caret_handoff.remaining_ingest_segments`，
/// 它们已经带旧 stage_id。新段用 `new_stage_id`。如果旧剩余段非空，保证相邻：
/// `old_remaining.last().to == new_route.first().from`，如果不等，插入一个
/// LayoutHandoff 换位段（ingest_side=None, stage_id=旧 stage_id）。
pub(crate) fn build_ingest_route(
    spec: &VisualEditSpec,
    slices: &[AnimatedSlice],
    new_stage_id: IngestStageId,
) -> Vec<CaretTrackSegment> {
    // Issue #815 评论 5950375533 问题3: 路径的**屏幕起点**必须优先用 handoff
    // （上一帧真正画出来的 caret 位置），拿不到才退回逻辑 `old_cursor_rect`。
    //
    // 原实现固定用 `spec.old_cursor_rect`，而 `build_cursor_visual_track` 拿到的
    // `handoff.sampled` 只写进顶层 `track.from`；只要 `segments` 非空，
    // `sampled_rect_at_progress()` 根本不读 `track.from`，读的是 `segments[0].from`
    // —— 于是快速连续输入时渲染又跳回逻辑 old caret。
    let screen_caret = spec
        .visual_state
        .caret_handoff
        .as_ref()
        .map(|handoff| &handoff.sampled)
        .or(spec.old_cursor_rect.as_ref());
    let delete_rows = collect_delete_rows(slices);
    let insert_rows = collect_insert_rows(slices);
    if delete_rows.is_empty() && insert_rows.is_empty() {
        // 即使没有新吞吐行，旧剩余段仍需保留，让 carried unit 继续收口。
        return spec
            .visual_state
            .caret_handoff
            .as_ref()
            .map(|h| h.remaining_ingest_segments.clone())
            .unwrap_or_default();
    }
    // 先用现有逻辑构造当前新编辑的 route，所有新段标记 new_stage_id。
    let new_route = match ingest_route_shape(slices) {
        IngestRouteShape::InsertOnly => screen_caret
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(screen_caret, new_caret)| {
                build_insert_route(&insert_rows, screen_caret, new_caret, new_stage_id)
            })
            .unwrap_or_default(),
        IngestRouteShape::DeleteOnly => screen_caret
            .zip(spec.new_cursor_rect.as_ref())
            .map(|(screen_caret, new_caret)| {
                build_delete_route(&delete_rows, screen_caret, Some(new_caret), new_stage_id)
            })
            .unwrap_or_default(),
        // Issue #815 评论 5950887715: Mixed（IME commit 候选 Reveal + 旧 preedit
        // Conceal 同帧）**不再是退化路径**。
        IngestRouteShape::Mixed => {
            let Some(new_caret) = spec.new_cursor_rect.as_ref() else {
                return Vec::new();
            };
            let (Some(first_delete), Some(first_insert)) =
                (delete_rows.first().copied(), insert_rows.first().copied())
            else {
                return Vec::new();
            };
            let Some(screen_caret) = screen_caret else {
                return Vec::new();
            };
            // old 侧吞字（不含末尾换位段，交给下面统一接）。
            let mut segments = build_delete_route(&delete_rows, screen_caret, None, new_stage_id);
            // old 吞完 → 切到 new 侧 candidate 起点。
            let old_route_end = segments.last().map(|seg| seg.to).unwrap_or(*screen_caret);
            let insert_start = first_insert.caret_rect_at(first_insert.left);
            if !same_rect(&old_route_end, &insert_start) {
                segments.push(CaretTrackSegment {
                    kind: CaretTrackSegmentKind::RowHandoff,
                    from: old_route_end,
                    to: insert_start,
                    ingest_line_ord: Some(first_delete.line_ord),
                    ingest_side: Some(IngestSnapshotSide::Old),
                    visual_line_id: first_delete.visual_line_id,
                    ingest_stage_id: new_stage_id,
                });
            }
            // new 侧吐字。
            segments.extend(build_insert_route(
                &insert_rows,
                &insert_start,
                new_caret,
                new_stage_id,
            ));
            segments
        }
    };
    // Issue #819 评论 5968931455: 合成旧 carried route 剩余段 + 新 route。
    // 旧剩余段来自 handoff，已经带旧 stage_id。
    let old_remaining = spec
        .visual_state
        .caret_handoff
        .as_ref()
        .map(|h| h.remaining_ingest_segments.clone())
        .unwrap_or_default();
    if old_remaining.is_empty() || new_route.is_empty() {
        // 没有旧剩余段或没有新段：直接返回新 route（或旧剩余段）。
        if old_remaining.is_empty() {
            return new_route;
        }
        return old_remaining;
    }
    // Issue #819 评论 5968931455: 新 route 的起点可能从 screen_caret（handoff.sampled）
    // 出发，而旧剩余段的终点是上一笔 route 的终点。二者不同时，直接拼会在中间产生
    // 一个回跳的换位段（caret 先退回 sampled 位置再向新目标走），破坏单调性——
    // 正是"先恢复完整再吞"的根因。
    //
    // 修正：把新 route 的起点**平移**到旧剩余段的终点，让 caret 从旧 route 停下
    // 的地方继续向新目标走，全程不回跳。只平移 from 端（第一段的 from），各段
    // 的 to 保持不变——后续段的 from 已由前一段的 to 决定，不需要再改。
    let mut combined = old_remaining.clone();
    let old_end = combined.last().map(|seg| seg.to);
    let mut new_route = new_route;
    if let (Some(old_end), Some(first_seg)) = (old_end, new_route.first_mut()) {
        if !same_rect(&old_end, &first_seg.from) {
            // 平移第一段起点到旧剩余段终点，消除回跳。
            first_seg.from = old_end;
        }
    }
    combined.extend(new_route);
    combined
}
