use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

use super::timeline::{TransactionTimeline, VisualUnitTiming};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

impl TextVisualTransactionState {
    /// Issue #727 评论 5757225958 问题3: 事务是否允许收集 clip_rects。
    ///
    /// 只有 Prepared / Rendering / Paused 状态的事务才允许静态层隐藏
    ///（Pending 无论 texture_prepared 是什么都不能隐藏正文，否则资源还没
    /// 准备好就会出现空洞；Completed/Cancelled 已移除）。
    /// 把状态检查集中到这个方法，让 build_render_plan_full 的 clip_rects
    /// 收集逻辑不再内联 `TextVisualTransactionState::Rendering` 等枚举值，
    /// 便于结构守卫测试断言"完成帧事务被 keys_to_complete 排除"。
    pub(crate) fn is_clip_eligible(self) -> bool {
        matches!(
            self,
            TextVisualTransactionState::Prepared
                | TextVisualTransactionState::Rendering
                | TextVisualTransactionState::Paused
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextVisualOperationKind {
    Insert,
    Delete,
    /// Issue #824 评论 5971089641 第 2 节：display patches 同时含 inserted 与
    /// deleted（IME 选区替换、粘贴覆盖选区）。它和 Insert/Delete 走同一个
    /// patch → retarget 入口，只是诊断标签不同。
    Replace,
    CompositionUpdate,
    CompositionCommitOrCancel,
}

/// Issue #690 评论 5675007226 步骤 3: 单个视觉单元。
///
/// Issue #815 评论 6042062633 修改 2: 计时驱动分成两种（见 [`VisualUnitTiming`]）。
/// - 非协同 InsertReveal/DeleteConceal、以及全部 ReflowMove/ReflowCrossFade：
///   `VisualUnitTiming::Timed`，用文字自己的 `ease_out_quad` + `text_duration_ms`。
/// - 协同 InsertReveal/DeleteConceal：`VisualUnitTiming::CaretTrack`，**没有自己的
///   时间线**。逐帧吞吐边界直接来自同一笔事务 cursor track 的当前帧
///   （`AnimatedSlice::compute_frame_by_caret_ingest`），文字层与光标层消费同一次
///   采样。
///
/// 协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立。
/// 不再是"两条互不相干的时间线，只在起点位置看起来碰巧挨着"。
///
/// 快速连续输入时，旧事务被 cancel 并 rebase：
///
/// - `Timed` unit 通过 `rebase_from_frame` 把当前 `visible_fraction` 写入
///   `start_fraction`，从"已经吐/吞到一半"的位置继续。
/// - `CaretTrack` unit 的连续性由 `RebaseCaretHandoff` 承担：先采样旧 track 的
///   当前 caret，新 track 从这个当前 caret 连到新的目标 caret。
///
/// 只有被新编辑实际覆盖的 unit 才结束/替换。
#[derive(Clone, Debug)]
pub(crate) struct PreparedVisualUnit {
    pub slice: AnimatedSlice,
    pub timing: VisualUnitTiming,
    /// Issue #819 评论 5968931455: 本 unit 所属的 visual stage id。
    ///
    /// - `None`：不参与 stage_id 过滤（向后兼容，测试构造 / Reflow unit）。
    /// - `Some(id)`：`sample_unit_slice_frame` 比较本 unit 的 stage_id 与
    ///   当前 segment 的 stage_id，不匹配时保持初态/终态，避免跨事务 carried unit
    ///   消费下一笔编辑的 route。
    pub stage_id: Option<IngestStageId>,
}

impl PreparedVisualUnit {
    /// 把一个 `AnimatedSlice` 包成拥有独立生命期的视觉单元。
    ///
    /// `duration_ms` 取自事务（与 `TransactionTimeline::duration_ms` 一致）。
    /// Issue #815: 本入口只用于 ReflowMove/ReflowCrossFade，始终独立 Timed timing。
    /// Issue #819 评论 5968931455: `stage_id` 默认 `None`（Reflow 不参与吞吐，
    /// 不需要 stage_id 过滤）。调用方按需用 `with_stage_id` 设置。
    pub fn wrap(slice: AnimatedSlice, duration_ms: u64) -> Self {
        let timing = VisualUnitTiming::default_for_kind(slice.kind, duration_ms);
        Self {
            slice,
            timing,
            stage_id: None,
        }
    }

    /// Issue #756 / Issue #815 评论 6042062633 修改 2: 按 `coordinated` 决定
    /// InsertReveal/DeleteConceal 的计时驱动。
    ///
    /// Issue #815 评论 6042062633 修改 2：`coordinated=true` 时返回
    /// [`VisualUnitTiming::CaretTrack`]——吞吐字不再拥有自己的
    /// `ease_out_quad + text_duration_ms` progress，逐帧边界直接来自本事务
    /// cursor track 的当前帧。`coordinated=false` 时仍是独立 `Timed`。
    ///
    /// ReflowMove / ReflowCrossFade 走 [`PreparedVisualUnit::wrap`]，始终独立 `Timed`。
    ///
    /// `is_caret_line` / `caret_anchor_x` 仍表达遮罩锚点与方向（构造阶段写入），
    /// 但锚点只是几何参考——真正决定每帧边界的是当前 caret.x。
    ///
    /// 协同=一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立。
    /// Issue #819 评论 5968931455: `stage_id` 默认 `None`，调用方按需设置。
    pub fn wrap_with_coordinated(
        slice: AnimatedSlice,
        duration_ms: u64,
        coordinated: bool,
    ) -> Self {
        let timing = VisualUnitTiming::default_for_kind_with_coordinated(
            slice.kind,
            duration_ms,
            coordinated,
        );
        Self {
            slice,
            timing,
            stage_id: None,
        }
    }

    /// Issue #819 评论 5968931455: 设置本 unit 的 visual stage id。
    ///
    /// 协同 InsertReveal/DeleteConceal 在 `build_prepared_transaction` 中
    /// 调本方法标记自己属于哪个 stage。carried unit 标记旧事务的 stage_id，
    /// 新事务自己创建的 unit 标记新事务的 stage_id。
    pub fn with_stage_id(mut self, stage_id: IngestStageId) -> Self {
        self.stage_id = Some(stage_id);
        self
    }

    /// 从自己的时间线计算当前 progress（0..1）。
    /// Issue #815: `Timed` 从自己的时间线算；`CaretTrack` 没有自己的时间线，
    /// 未 retired 时返回 0，retired 后返回 1。
    pub fn progress(&self, now: Instant) -> f64 {
        self.timing.progress(now)
    }

    /// 判断单元是否已到达终态（不应再交棒）。
    /// `Timed`：`progress >= 1.0` 表示已播完。`CaretTrack`：只在 retired 后为 true。
    #[cfg(test)]
    pub fn is_finished(&self, now: Instant) -> bool {
        self.timing.progress(now) >= 1.0
    }

    /// 单元在 `now` 时刻的**独立**可见比例（0..1）。
    ///
    /// Issue #815: 只有 `Timed` unit 允许消费这个值去算逐帧 clip。`CaretTrack` unit
    /// 的边界是 caret.x 本身，必须走 `AnimatedSlice::compute_frame_by_caret_ingest`。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        self.timing.current_visible_fraction(now)
    }

    /// 按旧单元的当前帧续播本单元。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 可见比例成为新单元的起点。
    /// Issue #690 评论 5683759796: started_at 留 None，等进入 Rendering 再用
    /// sample.frame_now 启动，不再用 frame.sampled_at 提前计时。
    /// Issue #815: `CaretTrack` unit 直接 return——它的连续性由 cursor track 的
    /// handoff 承担（先采样旧 track 当前帧，新 track 从这个当前 caret 连到新目标）。
    pub fn rebase_from_frame(&mut self, frame: &RebaseFrame) {
        self.slice
            .rebase_from(frame.x, frame.y, frame.opacity, frame.visible_fraction);
        // Issue #727: 对 ReflowMove/ReflowCrossFade，slice.rebase_from 已修改 from_document_rect
        // 为屏幕位置（current_x），timing 的 start_fraction 应重置为 0——from 已被改写为
        // 屏幕位置，不需要再通过 start_fraction 表达已演进状态，否则进度会被重复应用。
        // Issue #785: 对 InsertReveal/DeleteConceal，slice.rebase_from 只修改 start_fraction
        //（不修改几何），timing 的 start_fraction 需要保留 visible_fraction 作为交棒载体，
        // 文字从当前可见比例继续，不从 0 重播。
        let timing_start_fraction = match self.slice.kind {
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => 0.0,
            _ => frame.visible_fraction,
        };
        // Issue #815: CaretTrack unit 没有独立时间线，start_fraction 不参与逐帧 clip；
        // 它的连续性由 RebaseCaretHandoff 承担（新 track 从旧 track 当前帧连到新目标）。
        if self.timing.is_caret_track() {
            return;
        }
        self.timing
            .rebase_from_frame(timing_start_fraction, frame.remaining_duration_ms);
    }
}

/// 旧事务某个视觉单元在当前时刻的视觉帧，交棒给新事务继续播放。
///
/// Issue #690 评论 5675007226 步骤 3: 除了位置与透明度，必须带当前 `visible_fraction`
/// 和单元自己的时间线。只传 `(x, y, opacity)` 时 Reveal/Conceal 会按新事务 progress
/// 重新 0→1 / 1→0，快速连打时上一笔的字被反复重启。
///
/// Issue #690 评论 5679744253 问题 1: 原来同时携带 `started_at`（旧起点）和 `duration_ms`
/// （旧总时长），下一帧 `current_visible_fraction()` 用旧时间线算 progress，再从
/// `start_fraction` 到 target 做 easing，进度被重复应用，快速连打时制造跳变。
/// 现在改为携带 `sampled_at`（采集帧时间点）和 `remaining_duration_ms`（旧 unit 剩余
/// 播放时长），retarget 时从当前帧重新起一段：`duration_ms = remaining_duration_ms`，
/// 不再沿用旧起始时间。
///
/// Issue #690 评论 5683759796: `sampled_at` 不再直接作为新 unit 的 `started_at`——
/// `rebase_from_frame` 现在把 `started_at` 留 `None`，等进入 Rendering 才用
/// `sample.frame_now` 启动。`sampled_at` 字段保留做诊断/连续性断言
/// （`issue690_collect_rebase_frames_uses_per_unit_progress` 断言 `frame.sampled_at == now`，
/// `collect_rebase_frames` 仍设置 `sampled_at: now`）。
#[derive(Clone, Debug)]
pub(crate) struct RebaseFrame {
    pub byte_start: usize,
    pub byte_end: usize,
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
    pub shaping_identity: Option<ShapingIdentity>,
    pub visible_fraction: f64,
    /// 采集本帧的时间点。Issue #690 评论 5683759796: 不再作为新单元的 `started_at`，
    /// 仅保留做诊断/连续性断言；新单元 `started_at = None`，等进入 Rendering 再启动。
    /// Issue #701 评论 5699573227: `match_rebase_frames` 读取此字段写动画诊断日志。
    pub sampled_at: Instant,
    /// 旧单元剩余的播放时长；retarget 后作为新单元的 `duration_ms`。
    pub remaining_duration_ms: u64,
}

/// Issue #690 评论 5681206040 / Issue #815: coordinated caret 的正式视觉 track。
///
/// Issue #815 评论 6042062633 修改 3: 这条 track 是协同动画的唯一运动事实源。
/// cursor track 走自己的 `ease_out_cubic`；协同 InsertReveal/DeleteConceal 的逐帧
/// 吞吐边界直接用本 track 当前帧的 caret.x 算出，不再拥有自己的时间线。
/// 非协同文字动画（独立"打字动画"）和 Reflow 仍走自己的 `ease_out_quad` 时间线。
///
/// 收成为一个完整 caret track 后：
/// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
///   `started_at = None`，`duration_ms = 事务时长`。
/// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，`started_at = now`，
///   `duration_ms` 用旧 track 剩余时长，不再借任何文字 unit 的 progress。
/// - 文字层与光标层消费同一次 `sample_coordinated_motion_frame` 采样。
/// - 下一次 rebase 再从同一个 caret track 采样，不能回头使用逻辑 `old_cursor_rect`。
///
/// Issue #815 评论 5949097065 问题3: caret 运动轨迹的一段。
///
/// #815 第 1–5 轮把 cursor track 当成"old rect → new rect 一条直线"，
/// 跨软换行时那条直线是从右上飞到左下的对角线，文字拿到的是这条对角线上的
/// 某个 x 而不是"这一行上 caret 走到了哪里"。这里把轨迹正式拆成有序段，
/// 每段声明它是"换 layout 位置"还是"沿某一行真正扫过"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretTrackSegmentKind {
    /// 跨 layout 的几何换位：同一个逻辑位置在两套 canonical 之间重新安放。
    /// 出现在吞吐路径**之前**（Insert 从旧 caret 换到新快照的吐字起点）。
    /// 这一段只移动 caret，不吞吐任何文字——协同吞吐字保持初始状态
    /// （Insert 尚未吐出 / Delete 尚未吞掉）。绝不能拿这一段的对角线 x 去裁字。
    LayoutHandoff,
    /// 沿某个视觉行真正扫过：这一段的 x 就是该行的吞吐边界，
    /// 文字层与光标层消费同一个 x。
    IngestLine,
    /// 跨行 / 跨 layout 的几何换位，出现在吞吐路径**之外**：
    /// - 行与行之间的下潜或上移（Insert 换到下一行、Delete 换到上一行）；
    /// - Delete 吞吐完之后从旧快照的 `deleted_range.start` 换到新快照的最终 caret。
    ///
    /// 这一段只移动 caret，不吞吐任何文字：`ingest_line_ord` 指向刚扫完的那一行，
    /// 该行保持终态，更后面的行仍保持初始态。
    RowHandoff,
}

/// Issue #815 评论 5950887715: 这一段的 `ingest_line_ord` 属于哪一侧 canonical。
///
/// `VisualLine.id` 每次排版都从 0 重新按全文顺序编号，old/new 两份快照的同名
/// 行号是**两套互不相干的坐标系**。此前只能放弃 Mixed（Reveal + Conceal 同帧）
/// 路径，退回 old→new 一条斜线——那正是前几轮从普通 Insert/Delete 里删掉的旧问题。
/// 加上 side 之后，行序只在**同一 side 内**比较，Mixed 也能建出正确分段路线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IngestSnapshotSide {
    /// old snapshot（`DeleteConceal` 吞的是旧字）。
    Old,
    /// new snapshot（`InsertReveal` 吐的是新字）。
    New,
}

/// Issue #819 评论 5968931455: 跨事务 carried unit 的阶段/快照身份。
///
/// 每个 `CaretTrackSegment` 标明自己属于哪个 visual stage，
/// 避免跨事务 carried unit 的两个 "Old" 被当成同一侧。
///
/// 连续 Backspace 例子：`ABC|`，第一笔删 C 播到一半（C 约半个可见，caret 在 25），
/// 第二笔删 B。carried C 的旧 snapshot 是 stage A，当前 B 的 old snapshot 是 stage B。
/// 新事务的 route 合成为 `旧剩余段(stage A) + 新段(stage B)`。
/// `compute_frame_by_caret_ingest` 根据 stage_id 判断哪个 unit 该被当前 segment 驱动：
/// - 采样到 stage A 段时，只有 carried C（stage A）被驱动，新 B（stage B）保持初态；
/// - 采样到 stage B 段时，carried C（stage A）保持终态，新 B（stage B）被驱动。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct IngestStageId(pub(crate) u64);

/// 一段 caret 轨迹。`from`/`to` 都是**本段所属那侧 canonical** 的文档坐标，
/// 绝不跨 layout 混用。
///
/// Issue #824 评论 5971089641 第 4 节：segment **不再拥有“每段独立 easing”的语义**。
/// - 整条 active motion 只有一份连续 progress/velocity：全局 progress 只做一次
///   easing，然后按 `distance_weight` 映射到对应段；
/// - 段内线性插值，跨 segment 时速度连续（不再每段 `ease_out_cubic(local)` 把
///   每个段尾速度掉到 0）；
/// - `distance_weight` 只表达**几何路程/权重**（本段 from→to 的路径长度），
///   绝不代表“一段独立动画”的时长。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CaretTrackSegment {
    pub kind: CaretTrackSegmentKind,
    pub from: CursorRect,
    pub to: CursorRect,
    /// 仅 `IngestLine` 有值：本段正在扫的那一行在**本侧 canonical** 里的行序。
    /// 文字层用它判断"我是不是当前这一行"，不再靠 caret.y 反推。
    pub ingest_line_ord: Option<usize>,
    /// Issue #815 评论 5950887715: 本段的 `ingest_line_ord` 属于哪一侧快照。
    ///
    /// - old 侧 `DeleteConceal` 段：`Old`；
    /// - new 侧 `InsertReveal` 段：`New`；
    /// - 最开始纯几何换位（还没进入任何一侧的吞吐）：`None`；
    /// - old 吞完、准备切到 new 的中间换位：保留 `Old` +
    ///   `is_ingest_segment = false`，表示 old 已完成、new 尚未开始。
    pub ingest_side: Option<IngestSnapshotSide>,
    /// 本段所属那侧 canonical 的 `visual_line_id`（只用于光标层画 caret）。
    pub visual_line_id: Option<usize>,
    /// Issue #819 评论 5968931455: 本段所属的 visual stage id。
    ///
    /// Issue #824 评论 5971089641：新模型里一条 route 只属于**当前这笔** active
    /// motion，所有段都带本笔事务的 stage_id；不再存在“旧 route 剩余段保留旧
    /// stage_id”的串行拼接。
    pub ingest_stage_id: IngestStageId,
    /// 本段在整条 motion 里的**路程权重**。只表达几何路程：
    /// 全局 easing 后的 progress 按累计 `distance_weight` 映射到段，
    /// 段内线性插值。绝不代表“一段独立动画”的时长。
    pub distance_weight: f64,
}

impl CaretTrackSegment {
    /// 几何路径段构造：`distance_weight` 直接取 from→to 的路程长度。
    ///
    /// Issue #824 评论 5971089641 第 3/4 节：`IngestLine / RowHandoff /
    /// LayoutHandoff` 只描述几何路径，时间推进由整条 active motion 统一管理，
    /// 所以这里不接收任何时长参数——权重只来自几何。
    pub(crate) fn new(
        kind: CaretTrackSegmentKind,
        from: CursorRect,
        to: CursorRect,
        ingest_line_ord: Option<usize>,
        ingest_side: Option<IngestSnapshotSide>,
        visual_line_id: Option<usize>,
        ingest_stage_id: IngestStageId,
    ) -> Self {
        Self {
            kind,
            from,
            to,
            ingest_line_ord,
            ingest_side,
            visual_line_id,
            ingest_stage_id,
            distance_weight: segment_path_length(&from, &to),
        }
    }

    /// 覆盖段的**路程权重**（仍只表达几何，不表达时间）。
    ///
    /// 用于 `DeleteForwardBoundary` 这种「caret 不动、吞吐边界自己收拢」的段：
    /// 段自身的 from→to 长度为零，但它扫过的路程是**本行的吞吐宽度**。
    /// 时间推进依旧由整条 active motion 统一管理。
    pub(crate) fn with_distance_weight(mut self, weight: f64) -> Self {
        self.distance_weight = weight.max(1e-3);
        self
    }
}

/// 一段几何路径的长度（文档坐标，x/top 平面）。
///
/// 用 `max(eps)` 兜底：零长度段权重极小，映射时不会被选中，也不会让总权重为 0。
fn segment_path_length(from: &CursorRect, to: &CursorRect) -> f64 {
    let dx = to.x - from.x;
    let dy = to.top - from.top;
    (dx * dx + dy * dy).sqrt().max(1e-3)
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCursorVisualTrack {
    pub from: CursorRect,
    pub to: CursorRect,
    /// Issue #722 评论 5749164244 问题1: from/to 端的视觉行 id。
    ///
    /// `CursorRect`（Core 类型）不带 visual_line_id，但 `layout::CaretRect` 有。
    /// pipeline.rs 构造事务时把 `CaretRect.visual_line_id` 传进来，
    /// 不在 `make_cursor_rect_from_caret_doc()` 后丢掉。
    /// `None` 表示未知（fallback 路径或测试构造），采样时走 y fallback。
    pub from_visual_line_id: Option<usize>,
    pub to_visual_line_id: Option<usize>,
    /// Issue #722 评论 5749791161: from/to 端真实视觉行的 top/bottom。
    ///
    /// 这些值直接来自对应 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`，
    /// 不是 caret 自己的 `CursorRect.top/bottom`（caret 矩形只是光标那条细矩形，
    /// 不等于整条视觉行的边界）。`sampled_visual_line_id_at_progress` 用这些值
    /// 判断 caret 是否已进入下一条视觉行，避免跨软换行时因 caret top 离开旧
    /// caret 细矩形就提前切换行 id。
    /// fallback 路径（行几何未知）传 0.0/0.0，采样时走 y fallback。
    pub from_line_top: f64,
    pub from_line_bottom: f64,
    pub to_line_top: f64,
    pub to_line_bottom: f64,
    /// Issue #690 评论 5682867529: caret track 不再在事务创建时就启动计时，
    /// 而是等到进入 `Rendering` 状态才和文字 unit 共用同一个 `frame_now` 起跑。
    /// `None` 表示尚未开始播放，`progress` 返回 0、`remaining_duration_ms` 返回全长。
    pub started_at: Option<Instant>,
    pub duration_ms: u64,
    /// 暂停起点；`Some` 表示当前处于暂停中，`resume` 时把 `started_at` 推前暂停时长。
    pub pause_start: Option<Instant>,
    /// Issue #815 评论 5949097065 问题3: 正式的运动路径。
    ///
    /// Issue #824 评论 5971089641 第 4 节：整条 motion 只有**一份**全局
    /// progress/velocity。全局 progress 只做一次 `ease_out_cubic`，再按
    /// `distance_weight` 映射到各段，段内线性插值——`segments` 只负责把全局路程
    /// 映射到几何，不再各自 easing。
    /// `from`/`to` 字段保留为整条运动的起点/终点（给高度、epoch 判断等用），
    /// 但逐帧几何一律走 `segments`。
    pub segments: Vec<CaretTrackSegment>,
    /// Issue #819 评论 5968931455: 本 track 所属的 visual stage id。
    ///
    /// 新事务的 track 拥有新 stage_id；它的 segments 可能混合旧 stage_id
    ///（carried route 剩余段）和新 stage_id（当前新编辑的 route 段）。
    /// `sampled_ingest_at_progress` 返回当前段 的 stage_id，
    /// 供 `sample_unit_slice_frame` 做 stage_id 过滤。
    pub stage_id: IngestStageId,
}

impl PreparedCursorVisualTrack {
    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// `started_at = None`（尚未进入 Rendering）时返回 0。
    pub fn progress(&self, now: Instant) -> f64 {
        match self.started_at {
            None => 0.0,
            Some(start) => {
                if self.duration_ms == 0 {
                    return 1.0;
                }
                let elapsed = now.duration_since(start).as_millis() as f64;
                (elapsed / self.duration_ms as f64).clamp(0.0, 1.0)
            }
        }
    }

    /// 全局 progress → (段下标, 段内**线性**进度)。
    ///
    /// Issue #824 评论 5971089641 第 4 节：整条 active motion 只有一份连续
    /// progress/velocity：
    /// 1. 全局 progress 只做**一次** `ease_out_cubic`；
    /// 2. 用 eased 进度乘以总路程权重，按累计 `distance_weight` 定位到段；
    /// 3. 段内线性插值，**不再对每段二次 easing** —— 那正是每个段尾速度掉到 0
    ///    的根因。
    ///
    /// 由此跨 segment 时速度连续：单位路程的推进速率由全局 easing 决定，段与段
    /// 之间只改变行进方向，不会在段边界停顿。
    fn segment_at_progress(&self, progress: f64) -> Option<(usize, f64)> {
        if self.segments.is_empty() {
            return None;
        }
        let total: f64 = self.segments.iter().map(|s| s.distance_weight).sum();
        if total <= 0.0 {
            return Some((self.segments.len() - 1, 1.0));
        }
        let eased = AnimatedSlice::ease_out_cubic(progress.clamp(0.0, 1.0));
        let travelled = eased * total;
        let mut start = 0.0;
        for (index, segment) in self.segments.iter().enumerate() {
            let end = start + segment.distance_weight;
            if travelled < end {
                let local = (travelled - start) / segment.distance_weight;
                return Some((index, local.clamp(0.0, 1.0)));
            }
            start = end;
        }
        Some((self.segments.len() - 1, 1.0))
    }

    /// Issue #815 评论 5949097065 问题3: 本帧吞吐采样。
    ///
    /// 返回 `(本帧 caret 所在视觉行 id, 本段 ingest 行序, 是否处于吞吐段, 本段 side,
    /// 本段**局部**进度 0..1)`。
    ///
    /// Issue #824 评论 5971089641 第 4 节：局部进度直接取
    /// [`PreparedCursorVisualTrack::segment_at_progress`] 已经映射好的段内线性
    /// 进度，**不重算二次 easing**。全局 `progress` 仍供生命周期/完成判断使用；
    /// 吞吐边界（尤其 `DeleteForwardBoundary`）必须用局部进度，否则在多段 route
    /// 里某一行只能拿到整笔事务的几分之一。
    ///
    /// - `IngestLine` 段：三个值分别是 `(段上的 visual_line_id, Some(ingest_line_ord), true)`。
    ///   文字层看到 `true` 就必须用 `ingest_line_ord` 判行、用 `rect.x` 裁本行。
    /// - `LayoutHandoff`（吞吐路径之前的换位）：`(visual_line_id, None, false)`。
    ///   文字层看到 `false` 且行序为 `None` ⇒ 保持**初始**状态，一个字都不吞吐。
    /// - `RowHandoff`（行间/跨 layout 换位）：`(visual_line_id, Some(刚扫完那行的行序), false)`。
    ///   文字层看到 `false` 且行序有值 ⇒ 该行保持**终**状态，更后面的行仍是初始态。
    pub fn sampled_ingest_at_progress(
        &self,
        progress: f64,
    ) -> (
        Option<usize>,
        Option<usize>,
        bool,
        Option<IngestSnapshotSide>,
        f64,
    ) {
        match self.segment_at_progress(progress) {
            // 退化路线（完全没有分段）：x 仍然是一条可信的裁切边界，只是拿不到
            // 权威行序 ⇒ 交给文字层降级用 caret.y + 本行行范围判断相位。
            // 这与"有分段但正处在 LayoutHandoff 换位段"必须区分：后者 x 是几何
            // 换位值，不能当吞吐边界。
            //
            // Issue #815 评论 5950887715: side 传 `None`，文字层据此让**两侧**都保持
            // 初态——没有 side 就没有权威行序来源，绝不能让某一侧先动。
            // Issue #815 评论 5953049681 问题1: 完全没有分段时只有一条隐式轨迹，
            // 局部进度就是全局进度。
            None => (
                self.from_visual_line_id,
                None,
                true,
                None,
                progress.clamp(0.0, 1.0),
            ),
            Some((index, local_progress)) => {
                let segment = &self.segments[index];
                let side = segment.ingest_side;
                match segment.kind {
                    CaretTrackSegmentKind::IngestLine => (
                        segment.visual_line_id,
                        segment.ingest_line_ord,
                        true,
                        side,
                        local_progress,
                    ),
                    CaretTrackSegmentKind::LayoutHandoff => {
                        (segment.visual_line_id, None, false, side, local_progress)
                    }
                    CaretTrackSegmentKind::RowHandoff => (
                        segment.visual_line_id,
                        segment.ingest_line_ord,
                        false,
                        side,
                        local_progress,
                    ),
                }
            }
        }
    }

    /// Issue #819 评论 5968931455: 按 progress 采样当前路由段的 visual stage id。
    ///
    /// 返回当前 segment 的 `ingest_stage_id`。`segments` 为空（退化路线）时
    /// 返回 `None`，调用方退回 `track.stage_id`。
    pub(crate) fn sampled_stage_id_at_progress(&self, progress: f64) -> Option<IngestStageId> {
        self.segment_at_progress(progress)
            .map(|(index, _)| self.segments[index].ingest_stage_id)
    }

    /// Issue #815 评论 5949097065 问题3: 按 progress 采样当前 caret 所在视觉行 id。
    /// Issue #702: 用外部传入的 progress 采样 caret rect。
    /// Issue #815 评论 5947728704 问题2: 本方法给出的是**已经 easing 过的真实屏幕几何**，
    /// 协同 InsertReveal/DeleteConceal 与光标共同消费 `sample_caret_track_frame` 的
    /// 同一份采样；只有非协同 Timed 文字才有自己的 `ease_out_quad`。
    ///
    /// Issue #815 评论 5949097065 问题3: 逐帧几何走 `segments`，不再用
    /// `from.x → to.x` / `from.top → to.top` 一条直线——跨软换行时那条直线是
    /// 从右上飞到左下的对角线，文字拿到的是对角线上的某个 x，而不是
    /// "这一行上 caret 走到了哪里"。
    pub fn sampled_rect_at_progress(&self, progress: f64) -> CursorRect {
        match self.segment_at_progress(progress) {
            None => {
                // fallback：没有正式路径时退回旧的 from→to 直线（仅非协同 cursor-only）。
                let eased = AnimatedSlice::ease_out_cubic(progress.clamp(0.0, 1.0));
                let x = self.from.x + (self.to.x - self.from.x) * eased;
                let top = self.from.top + (self.to.top - self.from.top) * eased;
                let h = self.to.bottom - self.to.top;
                CursorRect {
                    x,
                    top,
                    bottom: top + h,
                    baseline_y: self.from.baseline_y
                        + (self.to.baseline_y - self.from.baseline_y) * eased,
                }
            }
            Some((index, eased)) => {
                let segment = &self.segments[index];
                let x = segment.from.x + (segment.to.x - segment.from.x) * eased;
                let top = segment.from.top + (segment.to.top - segment.from.top) * eased;
                let h = segment.to.bottom - segment.to.top;
                CursorRect {
                    x,
                    top,
                    bottom: top + h,
                    // Issue #712 评论 5739517945 第 2 项: baseline_y 段内插值，
                    // 不直接取 to.baseline_y，消除垂直动画跳终点。
                    baseline_y: segment.from.baseline_y
                        + (segment.to.baseline_y - segment.from.baseline_y) * eased,
                }
            }
        }
    }

    /// 首次事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 事务时长`。
    ///
    /// Issue #722 评论 5749791161: `from_line_top/bottom` 和 `to_line_top/bottom`
    /// 来自对应 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`，
    /// 是真实视觉行边界，不是 caret 自己的细矩形边界。
    #[allow(clippy::too_many_arguments)]
    pub fn new_first(
        from: CursorRect,
        to: CursorRect,
        from_visual_line_id: Option<usize>,
        to_visual_line_id: Option<usize>,
        from_line_top: f64,
        from_line_bottom: f64,
        to_line_top: f64,
        to_line_bottom: f64,
        duration_ms: u64,
        stage_id: IngestStageId,
    ) -> Self {
        Self {
            from,
            to,
            from_visual_line_id,
            to_visual_line_id,
            from_line_top,
            from_line_bottom,
            to_line_top,
            to_line_bottom,
            started_at: None,
            duration_ms,
            pause_start: None,
            // Issue #815 评论 5949097065 问题3: `new_first` 是没有吞吐单元的
            // cursor-only 事务（只有光标要平滑移动），不存在"文字以 caret 为吞吐边界"，
            // 因此不建吞吐路径，逐帧几何走 `from → to` fallback。
            segments: Vec::new(),
            stage_id,
        }
    }

    /// Issue #815 评论 5949097065 问题3: 写入正式的运动路径。
    ///
    /// 调用方（`transaction_builder`）在 slice 建完、`assign_shared_line_masks`
    /// 之后才调它：那时每一行的真实吞吐范围才已知，路径必须由**同侧**的 slice
    /// 几何生成，不能拿另一侧 canonical 的坐标凑。
    pub fn set_segments(&mut self, segments: Vec<CaretTrackSegment>) {
        self.segments = segments;
    }

    /// Issue #690 评论 5682867529: caret track 跟文字视觉单元一起暂停/恢复，
    /// 不自己按墙钟继续走。暂停时记录 pause_start，resume 时把 started_at 推前
    /// 暂停时长，这样 progress(now) 自动跳过暂停区间。
    pub fn pause(&mut self, now: Instant) {
        if self.started_at.is_none() || self.pause_start.is_some() {
            return;
        }
        self.pause_start = Some(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if let (Some(start), Some(pause_start)) =
            (self.started_at.as_mut(), self.pause_start.take())
        {
            let paused_duration = now.duration_since(pause_start);
            *start += paused_duration;
        }
    }
}

/// Issue #701 评论 5699573227: `rebase_to` 仅在测试中直接调用（生产代码走
/// `build_cursor_visual_track` 的 handoff 分支，逻辑等价）。放在 `#[cfg(test)]`
/// impl 块里，避免 clippy 误报 dead_code，也不需要 `#[allow(dead_code)]`。
/// Issue #722 评论 5747719529: `eased` 和 `sampled_rect` 也仅在此 `#[cfg(test)]`
/// 块中被 `rebase_to` 调用，主路径改用 `sampled_rect_at_progress(progress(now))`。
#[cfg(test)]
impl PreparedCursorVisualTrack {
    /// Issue #808: 光标 track 用自己的 `ease_out_cubic` easing，不再共用文字的 `ease_out_quad`。
    pub fn eased(&self, now: Instant) -> f64 {
        AnimatedSlice::ease_out_cubic(self.progress(now))
    }

    /// 在 `now` 时刻按 `from -> to` 插值采样当前屏幕 caret rect。
    pub fn sampled_rect(&self, now: Instant) -> CursorRect {
        let eased = self.eased(now);
        let x = self.from.x + (self.to.x - self.from.x) * eased;
        let top = self.from.top + (self.to.top - self.from.top) * eased;
        let h = self.to.bottom - self.to.top;
        CursorRect {
            x,
            top,
            bottom: top + h,
            // Issue #712 评论 5739517945 第 2 项: baseline_y 从 from 到 to 插值，
            // 不再直接取 to.baseline_y，消除垂直动画跳终点。
            baseline_y: self.from.baseline_y + (self.to.baseline_y - self.from.baseline_y) * eased,
        }
    }

    /// 测试专用：从当前帧重新起一段 `from = sampled caret` → `to = new_to`。
    ///
    /// Issue #824 评论 5971089641：retarget 只保留“当前屏幕状态 → 最新目标”，
    /// 不携带旧 route 剩余段；这里也不再把旧段排进新 track。
    /// 生产代码走 `build_cursor_visual_track` 的 handoff 分支（由 retarget_motion
    /// 重建几何路径），本方法只服务旧测试的连续性断言。
    pub fn rebase_to(&self, new_to: CursorRect, now: Instant) -> Self {
        let progress = self.progress(now);
        let sampled = self.sampled_rect(now);
        // Issue #815 评论 5949097065 问题3: rebase 落在中间行时，新 track 必须接住
        // 本帧**实际**的行身份与几何，不能退回逻辑 old 行，也不能只记 from/to 两个 id。
        let (sampled_line_id, _, _, _, _) = self.sampled_ingest_at_progress(progress);
        Self {
            from: sampled,
            to: new_to,
            from_visual_line_id: sampled_line_id.or(self.to_visual_line_id),
            to_visual_line_id: self.to_visual_line_id,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: None,
            // Issue #824: 新一段 motion 用完整的单笔时长，不吃旧 route 预算。
            duration_ms: self.duration_ms.max(1),
            pause_start: None,
            // 几何路径由 retarget_motion 重建，不从旧 route 续段。
            segments: Vec::new(),
            stage_id: self.stage_id,
        }
    }
}

/// 一次平台视觉事务持有的全部资源。
///
/// 它拥有一次动画所需的 units、patches、cursor transition
/// 和 old/new snapshots。事务 key 负责资源和 snapshot 所有权；
/// 逐个 `PreparedVisualUnit` 负责自己显示到哪里（见 `PreparedVisualUnit`）。
/// `texture_prepared` 为 true 后，静态层才允许隐藏对应范围，否则会出现一帧空洞。
/// 事务完成、取消或超时移除后，对应快照资源才可以释放。
#[derive(Clone, Debug)]
pub(crate) struct PreparedTextVisualTransaction {
    pub key: VisualTransactionKey,
    pub state: TextVisualTransactionState,
    pub operation_kind: TextVisualOperationKind,
    pub timeline: TransactionTimeline,
    pub units: Vec<PreparedVisualUnit>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    /// Issue #690 评论 5681206040 / Issue #808: coordinated caret 的正式视觉 track。
    ///
    /// 替代之前的 `cursor_visual_from` / `cursor_visual_to`（只有端点没有时间状态）。
    /// 自带 `started_at` / `duration_ms`，不再借任何文字 unit 的 progress。
    /// Issue #815 评论 6042062633 修改 3: 协同动画里这条 track 是**唯一**的运动事实源。
    /// 每帧只采样一次（`sample_coordinated_motion_frame`），采到的 caret x/y/
    /// visual_line_id/progress/rect 同时喂给文字吞吐层和光标层，文字不再自己算一次时间。
    ///
    /// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`。
    /// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
    ///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，
    ///   `started_at = None`，时长为旧路线剩余预算加本次编辑的完整预算。
    /// - `None` 表示本事务没有视觉 caret track（CursorOnly 或无 old/new cursor rect）。
    ///   协同模式下这笔事务会被 builder 拒绝入队并记 `editor.anim.transaction_skipped`，
    ///   不允许退化成"文字自己播、光标不动"。
    pub cursor_visual_track: Option<PreparedCursorVisualTrack>,
    pub cancel_reason: Option<String>,
    pub texture_prepared: bool,
    pub old_snapshot: Option<EditorLayoutSnapshot>,
    pub new_snapshot: Option<EditorLayoutSnapshot>,
    /// Issue #705 评论 5717380886: 创建本事务时记录的 `cursor_owner_epoch`。
    ///
    /// 这笔正文事务只在该 epoch 下拥有 coordinated caret。之后任何非正文事务
    /// 导致的逻辑 cursor 移动（鼠标点击、方向键、Home/End、拖选等）会 bump
    /// `CursorController::cursor_owner_epoch`，使本事务的 `cursor_owner_epoch`
    /// 不再等于当前 epoch。
    ///
    /// Issue #735 评论 5773604666 问题3 / Issue #815: 失去 caret ownership 时，
    /// 退休本事务的 caret motion（`caret_motion_retired = true` +
    /// `retire_caret_driven_units()`），协同 InsertReveal/DeleteConceal（CaretTrack）
    /// 立刻收口到终态——它们的边界来自这条 track，track 不再推进就不能停在半路。
    /// `Timed` unit（ReflowMove/ReflowCrossFade、非协同吞吐字）按自己的时间线继续播完。
    /// 不再存在"同一笔正文吞吐 transaction 还活着，但 caret_owner 已经不是它"
    /// 的状态。
    pub cursor_owner_epoch: u64,
    /// Issue #727 评论 5760650874 / Issue #735 评论 5773604666 问题3 / Issue #785:
    /// 该事务是否已永久失去 caret motion ownership。
    ///
    /// 一旦在 `build_text_animation_plan_with_sample` 中发现 `has_caret_driven_units
    /// && !owns_caret`（本帧有 CaretDriven units 但不是 owner），或在
    /// `find_cursor_transaction_for_target` 中发现 epoch 不一致时，此字段置 true。
    ///
    /// Issue #815: `retire_caret_driven_units()` 把所有 CaretTrack unit 置 retired，
    /// 协同吞吐字立刻收口到终态；`Timed` unit 不受影响，按自己的时间线继续播完。
    ///
    /// 之后 `active_text_transaction_key_with_epoch` 永远跳过此事务，
    /// `sample_coordinated_motion_frame` 不会再给它 `owner_key`，
    /// 已 Snap 回 canonical 的旧 caret 轨迹不会重新接管。
    /// ReflowMove/ReflowCrossFade 和非协同 InsertReveal/DeleteConceal 作为独立 Timed
    /// track 继续播完，事务只等剩余 Timed unit 与 cursor track 完成。
    pub caret_motion_retired: bool,
    /// Issue #710 评论 5731145076 症状五/六 / 评论 5732160521 问题 3:
    /// 事务的视觉 affected byte range，分 old/new 两侧保存。
    ///
    /// 基于段落边界扩展后的 byte range，而非原始的 inserted_range/deleted_range。
    /// 用于判断视觉区域重叠——newline 这种"改动 byte 很小、视觉影响很大"的事务，
    /// 原始 byte range 很小，但视觉 affected range 覆盖整个段落。
    ///
    /// 之前只有一个 `visual_affected_byte_range: Option<(usize, usize)>`，Insert 存
    /// new-text 坐标系、Delete 存 old-text 坐标系，`find_conflicting_transaction`
    /// 直接做数值 overlap，跨 revision 不可比，导致误判/漏判冲突。
    ///
    /// 现在拆成 old/new 两侧：
    /// - `visual_affected_byte_range_old`: old_text 坐标系（事务应用前的文本）
    /// - `visual_affected_byte_range_new`: new_text 坐标系（事务应用后的文本）
    ///
    ///   Insert 事务 old 侧是插入点、new 侧是 inserted_range；
    ///   Delete 事务 old 侧是 deleted_range、new 侧是删除后落点。
    ///
    /// `find_conflicting_transaction` 接收新事务的 `OffsetMap`（从旧事务 new 坐标系
    /// 到新事务查询坐标系的映射），把旧事务的 `visual_affected_byte_range_new`
    /// 映射到查询坐标系再做 overlap。映射失败时保守判定为冲突（避免漏判）。
    ///
    /// `None` 表示事务没有视觉 affected region（如 CursorOnly）。
    pub visual_affected_byte_range_old: Option<(usize, usize)>,
    pub visual_affected_byte_range_new: Option<(usize, usize)>,
    /// Issue #738 评论 5787277777: 活动事务绑定的 canonical layout basis revision。
    ///
    /// 事务创建时记录当时的 canonical layout revision。当 canonical layout 推进
    ///（换行导致后面整段 y 下移、宽度变化、字号变化等）时，`rebind_timed_units_to_canonical`
    /// 把仍存活的 Timed Reflow unit 从旧 basis 几何重绑到新 canonical 几何，并把这个
    /// 字段推进到当前 revision。`build_render_plan_full` 只接受 basis revision 不旧于
    /// 当前 canonical revision 的 unit，避免过期绝对坐标混进 scene graph。
    ///
    /// byte range + shaping identity 仍保留在 `PreparedVisualUnit` / `AnimatedSlice`，
    /// 作为到下一份 canonical layout 里重新找目标几何的锚点。
    pub layout_basis_revision: LayoutRevision,
}

impl PreparedTextVisualTransaction {
    pub fn is_composition(&self) -> bool {
        matches!(
            self.operation_kind,
            TextVisualOperationKind::CompositionUpdate
                | TextVisualOperationKind::CompositionCommitOrCancel
        )
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        let Some(effective_start) = self.timeline.effective_start() else {
            return false;
        };
        let elapsed = now.duration_since(effective_start).as_millis() as u64;
        // 连续编辑的 caret route 可能跨多笔预算，不能按单笔文字时钟提前过期。
        let effective_duration = self.timeline.duration_ms.max(
            self.cursor_visual_track
                .as_ref()
                .map(|track| track.duration_ms)
                .unwrap_or(0),
        ) + self.timeline.accumulated_paused_duration_ms;
        let timeout = effective_duration * 3 + 500;
        elapsed > timeout
    }

    pub fn progress(&self, now: Instant) -> f64 {
        self.timeline.progress(now)
    }

    /// Issue #735 评论 5773604666 问题3 / Issue #815: 收口本事务的 CaretTrack units。
    ///
    /// 协同 InsertReveal/DeleteConceal 的逐帧边界来自本事务的 cursor track。
    /// 一旦失去 caret motion ownership（epoch 切换 / layout basis 过期，由
    /// `caret_motion_retired = true` 在 coordinator.rs / render_plan_builder.rs
    /// 中标记），这条 track 不再推进，吞吐字必须立刻收口到终态。
    /// `Timed` unit（ReflowMove/ReflowCrossFade、非协同吞吐字）不受影响，
    /// 按自己的时间线播完。
    pub(crate) fn retire_caret_driven_units(&mut self) {
        // Issue #815 评论 6042062633 修改 3: 协同 InsertReveal/DeleteConceal 是
        // `VisualUnitTiming::CaretTrack`，逐帧边界来自本事务的 cursor track。
        // 失去 caret motion ownership（epoch 切换 / layout basis 过期）后，这条
        // track 不再推进，吞吐字必须立刻收口到终态，不能停在半路等一条死掉的轨迹。
        // `retired` 之后 `progress()` 返回 1.0，事务完成判断也不再等它。
        // `Timed` unit（ReflowMove/ReflowCrossFade、非协同吞吐字）不受影响，
        // 按自己的时间线播完。
        for unit in &mut self.units {
            unit.timing.retire_caret_motion();
        }
    }

    /// Issue #735 评论 5773604666 问题3: 判断本事务是否还有未播完的 Timed unit
    ///（ReflowMove/ReflowCrossFade）。
    ///
    /// 供测试验证收口语义：`retire_caret_driven_units` 只收口 CaretTrack unit，Timed unit 不受影响，
    /// - 返回 `false`：没有 Timed unit 或 Timed unit 已全部播完，事务可立即 Completed。
    /// - 返回 `true`：还有 Timed unit 在播，事务需等它们播完再 Completed。
    #[cfg(test)]
    pub(crate) fn has_active_timed_units(&self, now: Instant) -> bool {
        self.units.iter().any(|u| match u.slice.kind {
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                u.timing.progress(now) < 1.0
            }
            _ => false,
        })
    }

    /// Issue #735 评论 5773604666 问题3 / Issue #815: 判断本事务是否还有**未被收口**的
    /// CaretTrack units（协同 InsertReveal/DeleteConceal）。
    ///
    /// 已被 `retire_caret_driven_units` 收口的单元返回 false。
    pub(crate) fn has_caret_driven_units(&self) -> bool {
        self.units.iter().any(|u| u.timing.is_caret_driven())
    }

    /// 采集本事务中尚未播完的视觉单元当前帧，交棒给下一个事务。
    /// Issue #690 评论 5675007226 步骤 3: 逐单元用自己的 `progress`，不再用事务级
    /// timeline progress 一刀切——后者会把"已经吐到 60%"的单元算成事务的 30%，
    /// 交棒后视觉上仍会跳回一半。已播完（progress >= 1）的单元已是稳定终态，不采集。
    ///
    /// Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，retarget 时从当前帧
    /// 重新起一段，避免同时继承可见比例和已走过的时间线导致进度被重复应用。
    ///
    /// Issue #722 评论 5749164244 问题3: 生产路径（take_rebase_frames）不再调用此方法，
    /// 改用 `animation_coordinator::collect_rebase_frame_for_unit_without_caret` 对 Reveal/Conceal
    /// 统一从自己的 Timed 文字 timeline 取 visible_fraction。此方法保留供 #690 测试验证 per-unit progress 行为。
    ///
    /// Issue #815: `Timed` unit 从自己的时间线算 visible_fraction 和
    /// remaining_duration_ms；`CaretTrack` unit 没有独立时间线（elapsed/duration 都为 0），
    /// 它的 rebase 连续性由 cursor track 的 handoff 承担。
    #[cfg(test)]
    pub fn collect_rebase_frames(&self, now: Instant) -> Vec<RebaseFrame> {
        self.units
            .iter()
            .filter(|unit| !unit.is_finished(now))
            .map(|unit| {
                let visible_fraction = unit.current_visible_fraction(now);
                let frame = unit.slice.compute_frame(visible_fraction);
                // Issue #815: Timed 从自己的时间线算 elapsed/duration；CaretTrack 无独立时间线。
                let (elapsed_ms, duration_ms) = match &unit.timing {
                    VisualUnitTiming::Timed {
                        started_at,
                        duration_ms,
                        ..
                    } => {
                        let elapsed = match started_at {
                            Some(start) => now.duration_since(*start).as_millis() as u64,
                            None => 0,
                        };
                        (elapsed, *duration_ms)
                    }
                    // Issue #815: CaretTrack 没有自己的时间线。协同吞吐字的 rebase 连续性
                    // 由 cursor track 的 handoff 承担，不靠这里的 visible_fraction 交棒。
                    VisualUnitTiming::CaretTrack { .. } => (0, 0),
                };
                let remaining_duration_ms = duration_ms.saturating_sub(elapsed_ms);
                RebaseFrame {
                    byte_start: unit.slice.byte_start,
                    byte_end: unit.slice.byte_end,
                    x: frame.x,
                    y: frame.y,
                    opacity: frame.opacity,
                    shaping_identity: unit.slice.shaping_identity.clone(),
                    visible_fraction,
                    sampled_at: now,
                    remaining_duration_ms,
                }
            })
            .collect()
    }

    pub fn pause(&mut self) {
        if self.state == TextVisualTransactionState::Rendering {
            self.state = TextVisualTransactionState::Paused;
            let now = Instant::now();
            self.timeline.pause(now);
            // Issue #690 评论 5682867529: caret track 跟文字 timeline 一起暂停。
            if let Some(track) = self.cursor_visual_track.as_mut() {
                track.pause(now);
            }
        }
    }

    pub fn resume(&mut self) {
        if self.state == TextVisualTransactionState::Paused {
            let now = Instant::now();
            self.timeline.resume(now);
            // Issue #690 评论 5682867529: caret track 跟文字 timeline 一起恢复。
            if let Some(track) = self.cursor_visual_track.as_mut() {
                track.resume(now);
            }
            self.state = TextVisualTransactionState::Rendering;
        }
    }

    /// Issue #710 评论 5733109905: 冲突检测改为 **current-old 坐标系逐事务映射**。
    ///
    /// 本事务的 `visual_affected_byte_range_new` / units 的 byte range
    /// 都基于**本事务 new 坐标系**（事务应用后的文本）。查询 range `[byte_start, byte_end)`
    /// 基于**current-old 坐标系**（当前事务应用前的文本）。
    ///
    /// 要在同一坐标系比较，需要用 `OffsetMap::build(&本事务.new_text, current_old_text)`
    /// 把本事务 new 坐标系的 range 映射到 current-old 坐标系，再与查询 range 做 overlap。
    /// 本事务的 new_text 取自 `new_snapshot.virtual_text`。
    ///
    /// 如果 `new_snapshot` 为 `None`（无法获取本事务 new_text），退化为保守策略：
    /// 用 `visual_affected_byte_range_old` 直接和查询 range 做 old 坐标系数值比较
    ///（假设 old 坐标系 == current-old，这是无 new_text 时的最佳近似），
    /// units 也退化为裸数值比较。任一侧重叠即判定重叠。
    ///
    /// 映射失败（范围跨映射边界）时保守判定为冲突（返回 true），避免漏判。
    pub fn overlaps_byte_range(
        &self,
        byte_start: usize,
        byte_end: usize,
        current_old_text: &str,
    ) -> bool {
        let tx_new_text = self.new_snapshot.as_ref().map(|s| s.virtual_text.as_str());

        if let Some(tx_new_text) = tx_new_text {
            // 有 new_text：构造 per-tx offset_map（本事务 new 坐标系 → current-old 坐标系）
            let offset_map = writer_core::editor::OffsetMap::build(tx_new_text, current_old_text);

            // 映射 units 的 byte range 到 current-old 坐标系再比较
            for u in &self.units {
                match offset_map.map_old_range_to_new(u.slice.byte_start, u.slice.byte_end) {
                    Some((ms, me)) => {
                        if me > byte_start && ms < byte_end {
                            return true;
                        }
                    }
                    None => return true, // 映射失败，保守判定为冲突
                }
            }

            // 映射 visual_affected_byte_range_new 到 current-old 坐标系再比较
            if let Some((s, e)) = self.visual_affected_byte_range_new {
                match offset_map.map_old_range_to_new(s, e) {
                    Some((ms, me)) => {
                        if me > byte_start && ms < byte_end {
                            return true;
                        }
                    }
                    None => return true, // 映射失败，保守判定为冲突
                }
            }

            false
        } else {
            // new_snapshot 为 None：退化为保守 old 坐标系数值比较
            if let Some((os, oe)) = self.visual_affected_byte_range_old {
                if oe > byte_start && os < byte_end {
                    return true;
                }
            }
            self.units
                .iter()
                .any(|u| u.slice.byte_end > byte_start && u.slice.byte_start < byte_end)
        }
    }

    pub fn snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = self.units.iter().map(|u| u.slice.snapshot_id).collect();
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
    }
}
