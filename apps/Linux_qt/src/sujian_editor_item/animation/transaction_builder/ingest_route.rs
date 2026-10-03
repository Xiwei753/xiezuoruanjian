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
    pub(crate) fn caret_rect_at(&self, x: f64) -> CursorRect {
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
pub(crate) fn same_rect(a: &CursorRect, b: &CursorRect) -> bool {
    a.x == b.x && a.top == b.top && a.bottom == b.bottom
}

/// 生成 Insert 的吞吐路径（全部坐标来自 new snapshot）。
///
/// ```text
/// 当前屏幕 caret ──LayoutHandoff（仅在跨 layout 时）──▶ 新起始行左端
///   ──IngestLine 扫本行──▶ RowHandoff 到下一行左端 ──IngestLine 扫本行──▶ …
/// ```text
///
/// Issue #815 评论 5950375533 问题1: 原实现扫完第一行后，让第二行的 `IngestLine`
/// 直接从上一行右端连到本行右端/新 caret，把一条**行间斜线**标成了
/// `IngestLine(本行)` —— 正是第 6 轮明令禁止的"行间斜线 x 冒充本行吞吐边界"。
/// 现在每个非最后一行扫完后都显式 push 一个 `RowHandoff`，下一条 `IngestLine`
/// 的起点固定取**本行左端**，绝不继承上一行右端。
///
/// Issue #815 评论 5950375533 问题4: `screen_caret` 与吞吐起点几何相同时
/// 不生成 0 长度的 `LayoutHandoff`，直接从 `IngestLine` 开始。
///
/// Issue #824 评论 5971089641 第 3 节：本函数只生成**几何路径**；段权重由
/// [`CaretTrackSegment::new`] 按路程长度写入，时间推进由整条 active motion
/// 统一管理，不在每个段上分配时长。
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
        segments.push(CaretTrackSegment::new(
            CaretTrackSegmentKind::LayoutHandoff,
            cursor,
            ingest_start,
            None,
            // 最前置的纯几何换位还没进入任何一侧的吞吐。
            None,
            first.visual_line_id,
            stage_id,
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        // 每条 IngestLine 的起点必须是**本行左端**。
        cursor = row.caret_rect_at(row.left);
        segments.push(CaretTrackSegment::new(
            CaretTrackSegmentKind::IngestLine,
            cursor,
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
            row.caret_rect_at(row.right),
            Some(row.line_ord),
            Some(IngestSnapshotSide::New),
            row.visual_line_id,
            stage_id,
        ));
        if index + 1 < rows.len() {
            let next = rows[index + 1];
            segments.push(CaretTrackSegment::new(
                CaretTrackSegmentKind::RowHandoff,
                row.caret_rect_at(row.right),
                next.caret_rect_at(next.left),
                // 刚扫完的行 → 该行保持终态，后面的行还没被碰到。
                Some(row.line_ord),
                Some(IngestSnapshotSide::New),
                row.visual_line_id,
                stage_id,
            ));
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
        segments.push(CaretTrackSegment::new(
            CaretTrackSegmentKind::RowHandoff,
            row_end,
            *new_caret,
            Some(final_row.line_ord),
            Some(IngestSnapshotSide::New),
            final_row.visual_line_id,
            stage_id,
        ));
    }
    segments
}

/// 生成 Delete 的吞吐路径（吞字坐标来自 old snapshot，最后一段换位到 new caret）。
///
/// ```text
/// 旧屏幕 caret ──IngestLine 吞本行──▶ RowHandoff 到上一行左端 ──IngestLine 吞本行──▶ …
///   ──▶ old deleted_range.start ──RowHandoff──▶ 新快照最终 caret
/// ```text
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
        segments.push(CaretTrackSegment::new(
            CaretTrackSegmentKind::LayoutHandoff,
            *screen_caret,
            ingest_start,
            None,
            // 纯几何换位，还没进入 old 侧吞吐。
            None,
            start_row.visual_line_id,
            stage_id,
        ));
    }
    for (index, row) in rows.iter().enumerate().rev() {
        // Issue #824 评论 5971089641 第 3/4 节：段权重只表达**路程**。
        // `DeleteForwardBoundary`（前删）的真实 caret 不动，段的 from→to 是静止的，
        // 但它的吞吐边界要扫过整行——那段路程就是本行的吞吐宽度。
        let ingest_segment = CaretTrackSegment::new(
            CaretTrackSegmentKind::IngestLine,
            row_ingest_start(row),
            row_ingest_end(row),
            Some(row.line_ord),
            Some(IngestSnapshotSide::Old),
            row.visual_line_id,
            stage_id,
        );
        let ingest_segment = match row.driver {
            IngestBoundaryDriver::DeleteForwardBoundary => {
                ingest_segment.with_distance_weight((row.right - row.left).abs())
            }
            IngestBoundaryDriver::CaretPosition => ingest_segment,
        };
        segments.push(ingest_segment);
        if index > 0 {
            let next_up = &rows[index - 1];
            // Issue #815 评论 5954004872 问题1: 行间换位的终点必须等于**下一段实际的
            // from**，也就是 `row_ingest_start(next_up)`。原来写死 `next_up.right`，
            // 而下一行的 `IngestLine` 在 `next_up` 是 DeleteForwardBoundary 时从
            // `next_up.left` 起步 —— 相邻两段会瞬移。
            segments.push(CaretTrackSegment::new(
                CaretTrackSegmentKind::RowHandoff,
                row_ingest_end(row),
                row_ingest_start(next_up),
                Some(row.line_ord),
                Some(IngestSnapshotSide::Old),
                row.visual_line_id,
                stage_id,
            ));
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
            segments.push(CaretTrackSegment::new(
                CaretTrackSegmentKind::RowHandoff,
                swallow_end,
                *tail_target,
                Some(final_row.line_ord),
                Some(IngestSnapshotSide::Old),
                final_row.visual_line_id,
                stage_id,
            ));
        }
    }
    segments
}

// Issue #824 评论 5971089641 第 3 节：本模块只生成几何路径。
//
// 原来这里还有 `build_ingest_route()`：把 `caret_handoff.remaining_ingest_segments`
// 的旧 route 剩余段拼在 `new_route` 前面（`old_remaining + new_route`），再按
// `caret_duration_ms / new_route.len()` 给每个段平均分时间。那是“历史输入队列 +
// 每段一条独立动画”的串行模型，正是 #824 要删除的积债：
// - carried CaretTrack unit 被迫消费下一笔编辑的 route；
// - 连续删除时活动正文运动随按键次数无限增长。
//
// 现在 route 的装配（起点 → 最新目标、`new_stage_id`、路程权重）统一由
// `crate::sujian_editor_item::animation::retarget_motion::retarget` 负责：
// 起点来自 retarget 时采到的当前屏幕 caret/boundary，不拼任何历史段；
// 时间推进由整条 active motion 统一管理。
