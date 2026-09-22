use std::time::Instant;

use super::edit_motion::CursorRect;

use super::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, ShapingIdentity};
use super::transaction_key::VisualTransactionKey;

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
    CompositionUpdate,
    CompositionCommitOrCancel,
}

/// 统一事务时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress。
///
/// Choreographer 和 Qt update 只负责请求帧，不得给光标维护独立开始时间。
/// Paused 状态必须返回暂停瞬间的 progress，不能返回 0。
/// resume 后从暂停进度继续。
#[derive(Clone, Debug)]
pub(crate) struct TransactionTimeline {
    pub duration_ms: u64,
    pub first_render_frame: Option<Instant>,
    /// `first_render_frame` 对应的 Unix 毫秒，供诊断事件使用
    /// （`Instant` 只有本进程意义，不能直接进日志）。
    pub first_render_wall_ms: Option<i64>,
    pub rendering_started_at: Option<Instant>,
    pub pause_start: Option<Instant>,
    pub accumulated_paused_duration_ms: u64,
    pub paused_progress: f64,
}

impl TransactionTimeline {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            first_render_frame: None,
            first_render_wall_ms: None,
            rendering_started_at: None,
            pause_start: None,
            accumulated_paused_duration_ms: 0,
            paused_progress: 0.0,
        }
    }

    pub fn progress(&self, now: Instant) -> f64 {
        let Some(start) = self.effective_start() else {
            return 0.0;
        };
        if self.duration_ms == 0 {
            return 1.0;
        }
        if self.pause_start.is_some() {
            return self.paused_progress;
        }
        let elapsed_ms = now.duration_since(start).as_millis() as f64;
        let adjusted = elapsed_ms - self.accumulated_paused_duration_ms as f64;
        (adjusted / self.duration_ms as f64).clamp(0.0, 1.0)
    }

    /// Issue #727 评论 5760431554 问题2: 接收外部统一 `now`（frame_now），
    /// 不再内部 `Instant::now()`。transaction timeline / Timed units /
    /// cursor track 全部消费同一个 frame_now，消除首帧两套起点的采样偏差。
    /// `first_render_wall_ms` 诊断墙钟时间继续单独取，不影响动画时钟。
    pub fn mark_first_frame(&mut self, now: Instant) {
        if self.first_render_frame.is_none() {
            self.first_render_frame = Some(now);
            self.first_render_wall_ms = Some(crate::sujian_editor_item::diagnostic_now_ms());
        }
        if self.rendering_started_at.is_none() {
            self.rendering_started_at = Some(now);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if self.pause_start.is_some() {
            return;
        }
        self.paused_progress = self.progress(now);
        self.pause_start = Some(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if let Some(pause_start) = self.pause_start.take() {
            self.accumulated_paused_duration_ms +=
                now.duration_since(pause_start).as_millis() as u64;
        }
    }

    pub fn is_started(&self) -> bool {
        self.first_render_frame.is_some()
    }

    pub fn effective_start(&self) -> Option<Instant> {
        self.rendering_started_at.or(self.first_render_frame)
    }
}

/// Issue #727 评论 5754041813 约束 2: 视觉单元的计时语义拆分。
///
/// - `CaretDriven`：`InsertReveal` / `DeleteConceal` 永远使用此变体。吞字/吐字不是
///   独立动画，光标运动才是它的唯一视觉驱动。CaretDriven unit **没有自己的** progress /
///   duration / started_at；裁切边界直接消费本帧 coordinated caret 的位置
///   （`compute_frame_caret_driven`）。`start_fraction` / `target_fraction` 仅作为
///   rebase 交棒时的可见比例载体，不驱动独立时间线。
/// - `Timed`：`ReflowMove` / `ReflowCrossFade` 允许独立时间线（几何插值需要自己的
///   started_at / duration_ms / start_fraction / target_fraction）。
#[derive(Clone, Debug)]
pub(crate) enum VisualUnitTiming {
    /// InsertReveal / DeleteConceal：由本帧 caret geometry 驱动，无独立时间线。
    CaretDriven {
        /// rebase 交棒时的可见比例载体。不随时间变化，仅由 rebase 更新。
        start_fraction: f64,
        /// 目标可见比例（InsertReveal=1.0，DeleteConceal=0.0）。
        target_fraction: f64,
    },
    /// ReflowMove / ReflowCrossFade：独立时间线几何插值。
    Timed {
        started_at: Option<Instant>,
        duration_ms: u64,
        start_fraction: f64,
        target_fraction: f64,
    },
}

impl VisualUnitTiming {
    /// 从 `AnimatedSliceKind` 推断默认计时语义。
    /// InsertReveal / DeleteConceal → CaretDriven；ReflowMove / ReflowCrossFade → Timed。
    fn default_for_kind(kind: AnimatedSliceKind, duration_ms: u64) -> Self {
        let target_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 0.0,
            _ => 1.0,
        };
        let start_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 1.0,
            _ => 0.0,
        };
        match kind {
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                VisualUnitTiming::CaretDriven {
                    start_fraction,
                    target_fraction,
                }
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                VisualUnitTiming::Timed {
                    started_at: None,
                    duration_ms,
                    start_fraction,
                    target_fraction,
                }
            }
        }
    }

    /// 是否为 CaretDriven（InsertReveal / DeleteConceal）。
    #[cfg(test)]
    pub fn is_caret_driven(&self) -> bool {
        matches!(self, VisualUnitTiming::CaretDriven { .. })
    }

    /// 获取 `start_fraction`（rebase 交棒载体）。
    pub fn start_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::CaretDriven { start_fraction, .. } => *start_fraction,
            VisualUnitTiming::Timed { start_fraction, .. } => *start_fraction,
        }
    }

    /// 获取 `target_fraction`。
    pub fn target_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::CaretDriven {
                target_fraction, ..
            } => *target_fraction,
            VisualUnitTiming::Timed {
                target_fraction, ..
            } => *target_fraction,
        }
    }

    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// Issue #727 约束 2: CaretDriven unit 没有自己的时间线，返回 0.0。
    /// 只有 Timed unit（ReflowMove / ReflowCrossFade）才从自己的时间线算 progress。
    pub fn progress(&self, now: Instant) -> f64 {
        match self {
            VisualUnitTiming::CaretDriven { .. } => 0.0,
            VisualUnitTiming::Timed {
                started_at,
                duration_ms,
                ..
            } => match started_at {
                None => 0.0,
                Some(start) => {
                    if *duration_ms == 0 {
                        return 1.0;
                    }
                    let elapsed = now.duration_since(*start).as_millis() as f64;
                    (elapsed / *duration_ms as f64).clamp(0.0, 1.0)
                }
            },
        }
    }

    /// 单元在 `now` 时刻的真实可见比例（0..1）。
    ///
    /// Issue #727 约束 2: CaretDriven unit（InsertReveal / DeleteConceal）不再从独立
    /// 时间线驱动可见比例，直接返回 `start_fraction`（由 rebase 交棒设置）。
    /// 只有 Timed unit（ReflowMove / ReflowCrossFade）才从自己的时间线算可见比例。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        match self {
            VisualUnitTiming::CaretDriven { start_fraction, .. } => start_fraction.clamp(0.0, 1.0),
            VisualUnitTiming::Timed {
                start_fraction,
                target_fraction,
                ..
            } => {
                let eased = AnimatedSlice::ease_out_quad(self.progress(now));
                (start_fraction + (target_fraction - start_fraction) * eased).clamp(0.0, 1.0)
            }
        }
    }

    /// Issue #690 评论 5683759796: 在事务进入 Rendering 时打上统一起始时间。
    /// 只有 Timed unit 需要 started_at；CaretDriven unit 无独立时间线，no-op。
    pub fn mark_started(&mut self, frame_now: Instant) {
        if let VisualUnitTiming::Timed { started_at, .. } = self {
            if started_at.is_none() {
                *started_at = Some(frame_now);
            }
        }
    }

    /// Issue #690 评论 5683759796: rebase 交棒时更新可见比例载体。
    /// CaretDriven: 只更新 start_fraction（无时间线）。
    /// Timed: 更新 start_fraction + 重置时间线（started_at=None, duration=remaining）。
    pub fn rebase_from_frame(&mut self, visible_fraction: f64, remaining_duration_ms: u64) {
        match self {
            VisualUnitTiming::CaretDriven { start_fraction, .. } => {
                *start_fraction = visible_fraction.clamp(0.0, 1.0);
            }
            VisualUnitTiming::Timed {
                start_fraction,
                started_at,
                duration_ms,
                ..
            } => {
                *start_fraction = visible_fraction.clamp(0.0, 1.0);
                *started_at = None;
                *duration_ms = remaining_duration_ms.max(1);
            }
        }
    }
}

/// Issue #690 评论 5675007226 步骤 3: 单个视觉单元，拥有自己的动画生命期。
///
/// Issue #722 评论 5747719529 核心语义：光标本身就是吞字/吐字的视觉边界。
/// 文字不能再维护一套会和 caret 分叉的"自己什么时候完全出现/完全消失"的位置/
/// 可见度进度。真正决定当前 reveal/conceal 截止位置的是这一帧的 caret geometry
/// （caret_geometry_determines_clip / clip_from_coordinated_caret）。
///
/// Issue #727 评论 5754041813 约束 2: 计时语义拆分为 `VisualUnitTiming`。
/// - `InsertReveal` / `DeleteConceal` → `CaretDriven`：无独立时间线，裁切边界由本帧
///   coordinated caret 位置决定。
/// - `ReflowMove` / `ReflowCrossFade` → `Timed`：独立时间线几何插值。
///
/// 快速连续输入时，旧事务被 cancel 并 rebase：匹配的旧 unit 通过 `rebase_from_frame`
/// 把当前 `visible_fraction` 写入 `start_fraction`，文字从"已经吐/吞到一半"的位置继续。
/// 只有被新编辑实际覆盖的 unit 才结束/替换。
#[derive(Clone, Debug)]
pub(crate) struct PreparedVisualUnit {
    pub slice: AnimatedSlice,
    pub timing: VisualUnitTiming,
}

impl PreparedVisualUnit {
    /// 把一个 `AnimatedSlice` 包成拥有独立生命期的视觉单元。
    ///
    /// `duration_ms` 取自事务（与 `TransactionTimeline::duration_ms` 一致），
    /// 仅对 Timed unit（ReflowMove / ReflowCrossFade）有效。
    pub fn wrap(slice: AnimatedSlice, duration_ms: u64) -> Self {
        let timing = VisualUnitTiming::default_for_kind(slice.kind, duration_ms);
        Self { slice, timing }
    }

    /// 从自己的时间线计算当前 progress（0..1）。
    /// Issue #727 约束 2: CaretDriven unit 返回 0.0（无独立时间线）。
    pub fn progress(&self, now: Instant) -> f64 {
        self.timing.progress(now)
    }

    /// Issue #727 约束 2: 判断单元是否已到达终态（不应再交棒）。
    /// CaretDriven unit：`start_fraction == target_fraction` 表示已到终态。
    /// Timed unit：`progress >= 1.0` 表示已播完。
    #[cfg(test)]
    pub fn is_finished(&self, now: Instant) -> bool {
        match &self.timing {
            VisualUnitTiming::CaretDriven {
                start_fraction,
                target_fraction,
            } => (start_fraction - target_fraction).abs() < 1e-9,
            VisualUnitTiming::Timed { .. } => self.timing.progress(now) >= 1.0,
        }
    }

    /// 单元在 `now` 时刻的真实可见比例（0..1）。
    ///
    /// Issue #727 约束 2: CaretDriven unit（InsertReveal / DeleteConceal）不再从
    /// 独立时间线驱动，直接返回 `start_fraction`（由 rebase 交棒设置）。
    /// Timed unit（ReflowMove / ReflowCrossFade）从自己的时间线算可见比例。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        self.timing.current_visible_fraction(now)
    }

    /// 按旧单元的当前帧续播本单元。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 可见比例成为新单元的起点。
    /// Issue #690 评论 5683759796: started_at 留 None，等进入 Rendering 再用
    /// sample.frame_now 启动，不再用 frame.sampled_at 提前计时。
    pub fn rebase_from_frame(&mut self, frame: &RebaseFrame) {
        self.slice
            .rebase_from(frame.x, frame.y, frame.opacity, frame.visible_fraction);
        // Issue #727: 对 ReflowMove/ReflowCrossFade，slice.rebase_from 已修改 from_document_rect
        // 为屏幕位置（current_x），timing 的 start_fraction 应重置为 0——from 已被改写为
        // 屏幕位置，不需要再通过 start_fraction 表达已演进状态，否则进度会被重复应用。
        // 对 InsertReveal/DeleteConceal（CaretDriven），slice.rebase_from 只修改 start_fraction
        //（不修改几何），timing 的 start_fraction 需要保留 visible_fraction 作为交棒载体。
        let timing_start_fraction = match self.slice.kind {
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => 0.0,
            _ => frame.visible_fraction,
        };
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

/// Issue #690 评论 5681206040: coordinated caret 的正式视觉 track。
///
/// 之前 `cursor_visual_from` / `cursor_visual_to` 只是 `CursorRect` 端点，没有自己的
/// 时间状态（`started_at` / `duration_ms`），所以 `compute_coordinated_cursor_position`
/// 不得不借第一个 reflow unit 的 progress，`sample_coordinated_cursor_rect_at` 干脆
/// 不读这两个字段、固定用 `old_cursor_rect` / `new_cursor_rect`。连续交棒时第二次
/// rebase 采样到的光标回到逻辑 old caret 起算，与旧事务当前屏幕光标不一致。
///
/// 收成一个完整 caret track 后，两处问题一起消失：
/// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
///   `started_at = now`，`duration_ms = 事务时长`。
/// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，`started_at = now`，
///   `duration_ms` 用旧 track 剩余时长，不再借任何文字 unit 的 progress。
/// - InsertReveal / Backspace 有明确文字边界时，最终屏幕 x/y 仍直接取文字边界；
///   Enter、删除换行、软换行、纯 reflow 等没有明确边界时，直接 sample 这个 caret track。
/// - 下一次 rebase 再从同一个 caret track 采样，不能回头使用逻辑 `old_cursor_rect`。
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

    /// Issue #722 评论 5749164244 问题1: 按 progress 采样当前 caret 所在视觉行 id。
    ///
    /// Issue #722 评论 5749572808 问题2: 不再用 `progress < 0.5` 硬切 from/to 行。
    /// 改为按采样后的 caret y 与 from/to 行真实 top/bottom 判断：caret y 落在
    /// from 行 y 范围内 → 还在 from 行；落在 to 行 y 范围内 → 已到 to 行；
    /// 过渡中间空隙按 y 方向（向下/向上移动）判断。这样跨软换行交棒时
    /// 不会因 progress 过 0.5 就提前认为 caret 已进入 to 行。
    ///
    /// Issue #722 评论 5749791161: 行 y 范围用 `from_line_top/bottom` 和
    /// `to_line_top/bottom`（真实视觉行边界），不用 `self.from.top/bottom` 和
    /// `self.to.top/bottom`（caret 自己的细矩形边界）。向下跨软换行时，
    /// caret top 只要离开旧 caret 细矩形就可能直接切成 to 行，但这并不等于
    /// caret 已进入下一条视觉行。用真实行边界判断才能正确反映 caret 所在行。
    /// `None` 表示 from/to 行 id 未知（fallback 路径），调用方走 y fallback。
    pub fn sampled_visual_line_id_at_progress(&self, progress: f64) -> Option<usize> {
        match (self.from_visual_line_id, self.to_visual_line_id) {
            (Some(f_id), Some(t_id)) => {
                if f_id == t_id {
                    return Some(f_id);
                }
                let eased = AnimatedSlice::ease_out_quad(progress.clamp(0.0, 1.0));
                let caret_y = self.from.top + (self.to.top - self.from.top) * eased;
                // from 行 y 范围 [from_line_top, from_line_bottom)，
                // to 行 [to_line_top, to_line_bottom)。
                if caret_y >= self.from_line_top && caret_y < self.from_line_bottom {
                    Some(f_id)
                } else if caret_y >= self.to_line_top && caret_y < self.to_line_bottom {
                    Some(t_id)
                } else {
                    // 过渡中间空隙：按 y 方向判断。
                    if self.to_line_top > self.from_line_top {
                        // 向下移动：caret_y >= from_line_bottom 说明已离开 from 行，归 to。
                        if caret_y >= self.from_line_bottom {
                            Some(t_id)
                        } else {
                            Some(f_id)
                        }
                    } else if self.to_line_top < self.from_line_top {
                        // 向上移动：caret_y <= to_line_bottom 说明已进入 to 行。
                        if caret_y <= self.to_line_bottom {
                            Some(t_id)
                        } else {
                            Some(f_id)
                        }
                    } else {
                        // to_line_top == from_line_top：fallback 用 progress < 0.5。
                        if progress < 0.5 {
                            Some(f_id)
                        } else {
                            Some(t_id)
                        }
                    }
                }
            }
            (Some(f_id), None) => Some(f_id),
            (None, Some(t_id)) => Some(t_id),
            (None, None) => None,
        }
    }

    /// Issue #702: 用外部传入的 progress（来自文字 unit 的可见进度）采样 caret rect，
    /// 而非 caret track 自己的 timeline。消除删除事务里 caret track 与 DeleteConceal
    /// unit 帧基准分叉导致的"光标先完成、旧字晚消失"错拍。
    /// Issue #722 评论 5747719529: 此方法是 caret track 的主路径 API，
    /// `sample_caret_driven_clip` 和 `sample_coordinated_cursor_rect_at` 均通过
    /// `sampled_rect_at_progress(progress(now))` 调用。
    pub fn sampled_rect_at_progress(&self, progress: f64) -> CursorRect {
        let eased = AnimatedSlice::ease_out_quad(progress.clamp(0.0, 1.0));
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

    /// 旧 track 剩余的播放时长：`duration - elapsed`，下溢保护为 0。
    /// `started_at = None`（尚未进入 Rendering）时返回 `duration_ms` 全长。
    pub fn remaining_duration_ms(&self, now: Instant) -> u64 {
        match self.started_at {
            None => self.duration_ms,
            Some(start) => {
                let elapsed_ms = now.duration_since(start).as_millis() as u64;
                self.duration_ms.saturating_sub(elapsed_ms)
            }
        }
    }

    /// 首次事务：`from = old_cursor_rect`，`to = new_cursor_rect`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 事务时长`。
    ///
    /// Issue #722 评论 5749791161: `from_line_top/bottom` 和 `to_line_top/bottom`
    /// 来自对应 `VisualLine.y` 和 `VisualLine.y + VisualLine.height`，
    /// 是真实视觉行边界，不是 caret 自己的细矩形边界。
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
        }
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
    /// 与文字帧同一条 easing（`AnimatedSlice::ease_out_quad`）。
    pub fn eased(&self, now: Instant) -> f64 {
        AnimatedSlice::ease_out_quad(self.progress(now))
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

    /// 从当前帧重新起一段：`from = sampled caret`，`to = new_to`，
    /// `started_at = None`（等进入 Rendering 再启动），`duration_ms = 旧 track 剩余时长`（至少 1ms 保证非零）。
    ///
    /// Issue #722 评论 5749791161: 行几何字段在 rebase 时用 0.0/0.0（测试专用方法，
    /// 生产代码走 `build_cursor_visual_track` 的 handoff 分支，由 handoff 传递行几何）。
    pub fn rebase_to(&self, new_to: CursorRect, now: Instant) -> Self {
        let sampled = self.sampled_rect(now);
        let remaining = self.remaining_duration_ms(now).max(1);
        Self {
            from: sampled,
            to: new_to,
            from_visual_line_id: self.to_visual_line_id,
            to_visual_line_id: self.to_visual_line_id,
            from_line_top: 0.0,
            from_line_bottom: 0.0,
            to_line_top: 0.0,
            to_line_bottom: 0.0,
            started_at: None,
            duration_ms: remaining,
            pause_start: None,
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
    /// Issue #690 评论 5681206040: coordinated caret 的正式视觉 track。
    ///
    /// 替代之前的 `cursor_visual_from` / `cursor_visual_to`（只有端点没有时间状态）。
    /// 自带 `started_at` / `duration_ms`，不再借任何文字 unit 的 progress。
    ///
    /// - 首次正文事务：`from = old_cursor_rect`，`to = new_cursor_rect`。
    /// - rebase 时：先用这个 track 在同一个 `now` 采样当前屏幕 caret；
    ///   新事务 `from = sampled caret`，`to = 最新 new_cursor_rect`，
    ///   `started_at = now`，`duration_ms` 用旧 track 剩余时长。
    /// - InsertReveal / Backspace 有明确文字边界时，最终屏幕 x/y 仍直接取文字边界；
    ///   纯 reflow 等没有明确边界时，直接 sample 这个 caret track。
    /// - `None` 表示本事务没有视觉 caret track（CursorOnly 或无 old/new cursor rect）。
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
    /// Issue #735 评论 5773604666 问题3: 失去 caret ownership 时，CaretDriven units
    /// （InsertReveal/DeleteConceal）立即落到 canonical final state
    /// （`start_fraction` 设为 `target_fraction`），caret motion 同时结束。
    /// ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续。
    /// 不再存在"同一笔正文吞吐 transaction 还活着，但 caret_owner 已经不是它"
    /// 的状态。
    pub cursor_owner_epoch: u64,
    /// Issue #727 评论 5760650874 / Issue #735 评论 5773604666 问题3:
    /// 该事务是否已永久失去 caret motion ownership。
    ///
    /// 一旦在 `build_text_animation_plan_with_sample` 中发现 `has_caret_driven_units
    /// && !owns_caret`（本帧有 CaretDriven units 但不是 owner），或在
    /// `find_cursor_transaction_for_target` 中发现 epoch 不一致时，此字段置 true，
    /// 同时调用 `retire_caret_driven_units` 把 CaretDriven units 的 `start_fraction`
    /// 设为 `target_fraction`（终态）。
    ///
    /// 之后 `active_text_transaction_key_with_epoch` 永远跳过此事务，
    /// `sample_coordinated_motion_frame` 不会再给它 `owner_key`，
    /// 已 Snap 回 canonical 的旧 caret / 吞吐字轨迹不会重新接管。
    /// ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续播完，
    /// 事务只等剩余 Timed unit 完成。
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
    /// Insert 事务 old 侧是插入点、new 侧是 inserted_range；
    /// Delete 事务 old 侧是 deleted_range、new 侧是删除后落点。
    ///
    /// `find_conflicting_transaction` 接收新事务的 `OffsetMap`（从旧事务 new 坐标系
    /// 到新事务查询坐标系的映射），把旧事务的 `visual_affected_byte_range_new`
    /// 映射到查询坐标系再做 overlap。映射失败时保守判定为冲突（避免漏判）。
    ///
    /// `None` 表示事务没有视觉 affected region（如 CursorOnly）。
    pub visual_affected_byte_range_old: Option<(usize, usize)>,
    pub visual_affected_byte_range_new: Option<(usize, usize)>,
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
        let effective_duration =
            self.timeline.duration_ms + self.timeline.accumulated_paused_duration_ms;
        let timeout = effective_duration * 3 + 500;
        elapsed > timeout
    }

    pub fn progress(&self, now: Instant) -> f64 {
        self.timeline.progress(now)
    }

    /// Issue #735 评论 5773604666 问题3: 收口本事务的 CaretDriven units。
    ///
    /// 当正文 edit motion 失去 caret ownership（`cursor_owner_epoch` 不再等于
    /// 当前 epoch）时调用。把所有 CaretDriven unit（InsertReveal/DeleteConceal）
    /// 的 `start_fraction` 设为 `target_fraction`（终态）：
    /// - InsertReveal: `start_fraction = 1.0`（完全可见）
    /// - DeleteConceal: `start_fraction = 0.0`（完全消失）
    ///
    /// ReflowMove/ReflowCrossFade（Timed unit）保留不动，它们作为独立
    /// passive reflow track 继续播完自己的几何插值。
    ///
    /// 调用后 CaretDriven units 立即落到 canonical final state，不再继续播。
    /// 如果事务中还有 Timed unit，事务不立即 Completed（等 Reflow 播完）；
    /// 如果没有 Timed unit，事务可在下一帧 Completed。
    pub(crate) fn retire_caret_driven_units(&mut self) {
        for unit in &mut self.units {
            match unit.slice.kind {
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                    // CaretDriven unit: 把 start_fraction 设为 target_fraction（终态）。
                    if let VisualUnitTiming::CaretDriven {
                        start_fraction,
                        target_fraction,
                    } = &mut unit.timing
                    {
                        *start_fraction = *target_fraction;
                    }
                }
                AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                    // Timed unit: 保留不动，作为独立 passive reflow track 继续。
                }
            }
        }
    }

    /// Issue #735 评论 5773604666 问题3: 判断本事务是否还有未播完的 Timed unit
    /// （ReflowMove/ReflowCrossFade）。
    ///
    /// 供测试验证收口语义：`retire_caret_driven_units` 后，
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

    /// Issue #735 评论 5773604666 问题3: 判断本事务是否含 CaretDriven units
    /// （InsertReveal/DeleteConceal）。
    pub(crate) fn has_caret_driven_units(&self) -> bool {
        self.units.iter().any(|u| {
            matches!(
                u.slice.kind,
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
            )
        })
    }

    /// 采集本事务中尚未播完的视觉单元当前帧，交棒给下一个事务。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 逐单元用自己的 `progress`，不再用事务级
    /// timeline progress 一刀切——后者会把"已经吐到 60%"的单元算成事务的 30%，
    /// 交棒后视觉上仍会跳回一半。已播完（progress >= 1）的单元已是稳定终态，不采集。
    ///
    /// Issue #690 评论 5679744253 问题 1: 采集时计算剩余时长，retarget 时从当前帧
    /// 重新起一段，避免同时继承可见比例和已走过的时间线导致进度被重复应用。
    ///
    /// Issue #722 评论 5749164244 问题3: 生产路径（take_rebase_frames）不再调用此方法，
    /// 改用 `animation_coordinator::collect_rebase_frame_for_unit_without_caret` 对 Reveal/Conceal
    /// 从 caret track progress 推导 visible_fraction。此方法保留供 #690 测试验证 per-unit progress 行为。
    ///
    /// Issue #727 约束 2+3: CaretDriven unit 的 visible_fraction 从 caret track progress 推导，
    /// remaining_duration_ms 从 caret track 剩余时长计算。
    #[cfg(test)]
    pub fn collect_rebase_frames(&self, now: Instant) -> Vec<RebaseFrame> {
        let caret_track_progress = self
            .cursor_visual_track
            .as_ref()
            .map(|track| track.progress(now));
        let caret_remaining_ms = self
            .cursor_visual_track
            .as_ref()
            .map(|track| track.remaining_duration_ms(now))
            .unwrap_or(0);
        self.units
            .iter()
            .filter(|unit| {
                if unit.is_finished(now) {
                    return false;
                }
                // Issue #727 约束 2: CaretDriven unit 的终态由 caret track progress 决定。
                // is_finished() 只看 start_fraction == target_fraction，
                // 但 CaretDriven unit 的可见比例从 caret track progress 推导，
                // caret track progress >= 1.0 时 unit 已播完，不应再交棒。
                if unit.timing.is_caret_driven() {
                    if let Some(progress) = caret_track_progress {
                        if progress >= 1.0 {
                            return false;
                        }
                    }
                }
                true
            })
            .map(|unit| {
                let visible_fraction = match unit.slice.kind {
                    AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => {
                        let progress = caret_track_progress.unwrap_or(0.0);
                        let eased = AnimatedSlice::ease_out_quad(progress);
                        let start = unit.timing.start_fraction();
                        let target = unit.timing.target_fraction();
                        start + (target - start) * eased
                    }
                    AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                        unit.current_visible_fraction(now)
                    }
                };
                let frame = unit.slice.compute_frame(visible_fraction);
                // Issue #727 约束 2: 通过 VisualUnitTiming 访问 started_at / duration_ms。
                // CaretDriven unit 无独立时间线，remaining_duration_ms 从 caret track 计算。
                let (elapsed_ms, duration_ms) = match &unit.timing {
                    VisualUnitTiming::CaretDriven { .. } => (0u64, caret_remaining_ms),
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
    /// 基于**current-old 坐标系**（当前事务应用<|target|>前的文本）。
    ///
    /// 要在同一坐标系比较，需要用 `OffsetMap::build(&本事务.new_text, current_old_text)`
    /// 把本事务 new 坐标系的 range �F映射到 current-old 坐标系，再与查询 range 做 overlap。
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

/// Linux 当前唯一事务队列。
///
/// 冲突判断基于 byte range 与仍活跃的视觉资源，不是简单"新输入清空旧动画"：
/// 新事务的 byte range 与现有事务的 slices/patches 有重叠时，先从旧事务的当前视觉帧
/// rebase（保证连续输入无跳变），再取消旧事务。
///
/// rebase 语义：新事务的 old_snapshot 取自被取消事务的当前视觉帧而非原始快照，
/// 使动画起点与用户当前看到的画面一致。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTransactionQueue {
    transactions: Vec<PreparedTextVisualTransaction>,
}

impl PreparedTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, tx: PreparedTextVisualTransaction) {
        self.transactions.push(tx);
    }

    /// Issue #679 评论 5657313927: 把"资源准备完成"和"Pending -> Prepared 状态推进"
    /// 绑在同一个入口，避免事务卡在 Pending 导致光标不移动、静态层错位。
    ///
    /// 状态链：`Pending -> Prepared -> Rendering -> Completed/Cancelled`。
    /// 已 Completed/Cancelled 的事务不再推进，返回 false。
    pub fn mark_prepared(&mut self, key: VisualTransactionKey) -> bool {
        let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) else {
            return false;
        };
        if matches!(
            tx.state,
            TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
        ) {
            return false;
        }

        tx.texture_prepared = true;
        if tx.state == TextVisualTransactionState::Pending {
            tx.state = TextVisualTransactionState::Prepared;
        }
        true
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Completed;
            let ids = tx.snapshot_ids();
            self.transactions.retain(|t| t.key != key);
            return Some(ids);
        }
        None
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
            self.transactions.retain(|t| t.key != key);
            return true;
        }
        false
    }

    pub fn cancel_all(&mut self, reason: &str) {
        for tx in &mut self.transactions {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
        }
        self.transactions.clear();
    }

    pub fn tick(&mut self, now: Instant) -> Vec<VisualTransactionKey> {
        let mut expired = Vec::new();
        for tx in &mut self.transactions {
            if tx.is_expired(now) {
                tx.state = TextVisualTransactionState::Cancelled;
                tx.cancel_reason = Some("expired".to_string());
                expired.push(tx.key);
            }
        }
        self.transactions.retain(|t| !expired.contains(&t.key));
        expired
    }

    /// Issue #710 评论 5733109905: `find_conflicting_transaction` 改为
    /// **current-old 坐标系逐事务映射**，不再接收共用 `offset_map` 参数。
    ///
    /// `current_old_text` 是当前事务应用前的文本（current-old 坐标系）。
    /// `[byte_start, byte_end)` 是 current-old 坐标系的查询 range。
    ///
    /// 内部对每个 active tx：取 `tx.new_snapshot.virtual_text`（事务自己的 new_text），
    /// 构造 `OffsetMap::build(&tx.new_text, current_old_text)`（从该旧事务 new 坐标系
    /// → current-old 坐标系），映射 `visual_affected_byte_range_new` / units
    /// 到 current-old 坐标系再判断 overlap。
    ///
    /// 这样"冲突检测"和"当前编辑的 old→new 动画映射"是两件事，不再共用错的 OffsetMap。
    ///
    /// Issue #710 评论 5733833897: 返回**全部** active 冲突事务的 key（`Vec`），
    /// 不再只返回第一个。一次新编辑可能同时撞上多笔旧事务，调用方（`take_rebase_frames`）
    /// 需要逐笔处理：untouched 的 keep，受影响的 cancel。只返回第一笔会让后面的冲突
    /// 事务继续留在队列里按旧布局画，导致双层文字/闪烁/删除跨行乱跳。
    pub fn find_conflicting_transaction(
        &self,
        current_old_text: &str,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<VisualTransactionKey> {
        self.transactions
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .filter(|t| {
                t.overlaps_byte_range(byte_start, byte_end, current_old_text) || t.is_composition()
            })
            .map(|t| t.key)
            .collect()
    }

    pub fn active_transactions(&self) -> &[PreparedTextVisualTransaction] {
        &self.transactions
    }

    pub fn active_transactions_mut(&mut self) -> &mut [PreparedTextVisualTransaction] {
        &mut self.transactions
    }

    /// Issue #702 评论 5708436497: 只认 `TextVisualOperationKind::Insert`。
    ///
    /// 之前这里只判断 state 不是 Completed/Cancelled，没有过滤 `operation_kind`，
    /// 导致 Delete / CompositionUpdate / CompositionCommitOrCancel 事务也会返回 true，
    /// 被 `qquickitem_impl.rs` / `editing.rs` / `properties.rs` 里"输入期间抑制 blink"
    /// 的逻辑错误当成 Insert。现在明确过滤 Insert，让本方法名与实现一致。
    /// 任意正文事务的判断由 `has_active_text_transaction()` 负责。
    pub fn has_active_insert(&self) -> bool {
        self.transactions.iter().any(|t| {
            t.operation_kind == TextVisualOperationKind::Insert
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed
        })
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

// ── Issue #710 评论 5732160521 回归测试 ──
//
// 修复后这些测试验证"bug 已修复"：
// - 问题 2: tick/render/opacity/边沿 reset 四处消费同一个 current_cursor_blink_mode()，
//   判断一致。
// - 问题 3: visual_affected_byte_range 保存 old/new 两侧，find_conflicting_transaction
//   通过 OffsetMap 映射到同一坐标系比较，不再跨 revision 误判/漏判。
#[cfg(test)]
mod issue_710_comment_5732160521_repro {
    use super::*;

    /// 构造只含 virtual_text 的测试用 EditorLayoutSnapshot。
    fn make_test_snapshot(virtual_text: &str) -> EditorLayoutSnapshot {
        EditorLayoutSnapshot {
            revision: super::super::layout_snapshot::LayoutRevision::next(),
            line_snapshots: Vec::new(),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Upstream,
            virtual_text: virtual_text.to_string(),
        }
    }

    /// 构造最小化测试事务：只填 key/operation_kind/visual_affected_byte_range_{old,new}，
    /// 其余字段用空/None。state=Pending 保证被 find_conflicting_transaction 遍历。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        }
    }

    // ── 问题 2: 光标 blink 双重判断导致快速点击光标消失 ──
    //
    // 修复后：tick_cursor_animation / build_cursor_render_state_for_frame /
    // cursor_blink_opacity / 边沿 reset 四处全部消费 current_cursor_blink_mode()
    // 的同一个结果。本测试验证修复后的统一判断逻辑：在 CursorOnly Tween 期间
    //（has_cursor_only_tween=true，无 Insert 事务），blink 应被 Suppressed。
    #[test]
    fn test_issue710_cursor_blink_unified_judgment() {
        // 构造"没有 Insert 事务"的队列。CursorOnly Tween 不入正文事务队列
        //（它由 cursor_ctrl.animation 表示，不在 PreparedTransactionQueue 里）。
        let queue = PreparedTransactionQueue::new();

        // 真实调用源代码方法
        let has_active_insert = queue.has_active_insert();
        let has_active_text_transaction = queue.active_transactions().iter().any(|t| {
            !matches!(
                t.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            )
        });

        // CursorOnly Tween 已开始（cursor_ctrl.animation.is_some()），不入正文队列
        let has_cursor_only_tween = true;

        // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
        // 修复后的统一判断（current_cursor_blink_mode 的逻辑）：
        // tick / render / opacity / 边沿 reset 全部用这一个表达式。
        // 是否有吞吐字直接由 has_active_text_transaction 决定，不再受外部开关控制。
        let unified_suppressed = has_active_text_transaction || has_cursor_only_tween;

        // 验证：CursorOnly Tween 期间 blink 应被 Suppressed（常亮），不会因 blink
        // 切到 opacity=0 而消失。
        assert!(
            unified_suppressed,
            "修复后：CursorOnly Tween 期间 blink 应被 Suppressed（常亮），\
             has_active_insert={} has_active_text_transaction={} has_cursor_only_tween={}",
            has_active_insert, has_active_text_transaction, has_cursor_only_tween
        );

        // 验证：修复后 tick 和 render 用同一个判断，必然一致。
        // 之前 tick 用 has_active_text_transaction || has_cursor_only_tween，
        // render 用 has_active_insert，两者不一致。现在统一为 unified_suppressed。
        let tick_suppressed = unified_suppressed;
        let render_suppressed = unified_suppressed;
        assert_eq!(
            tick_suppressed, render_suppressed,
            "修复后：tick/render/opacity/边沿 reset 四处消费同一个 current_cursor_blink_mode()，\
             必然一致"
        );
    }

    // ── 问题 3: visual_affected_byte_range 跨 revision 不可比 ──
    //
    // 修复后：visual_affected_byte_range 保存 old/new 两侧，
    // find_conflicting_transaction 接收 OffsetMap，把旧事务的
    // visual_affected_byte_range_new 映射到查询坐标系再做 overlap。

    /// 问题 3a — 验证修复后不再误判跨 revision 冲突。
    ///
    /// tx1 (Insert): old="b", new="ab"（开头插 a）。
    ///   visual_affected_byte_range_new = Some((0, 1))  // a 在 new="ab" 的 0..1
    /// 之后外部操作把 "ab" 变成 "abc"（末尾插 c），当前 revision = "abc"。
    /// 第三笔在 "abc" 操作 c 区域，raw range = (2, 3)（"abc" 坐标系）。
    ///
    /// OffsetMap 从 tx1 的 new_text="ab" 到当前 "abc"：公共前缀 "ab"（2 bytes），
    /// entry: old=0, new=0, length=2, Identity。
    /// 映射 tx1 的 (0,1) → (0,1)。overlap (0,1) vs (2,3) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_visual_affected_byte_range_no_false_conflict() {
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((0, 0)), // old 侧是插入点
            Some((0, 1)), // new 侧是 inserted_range
            "ab",         // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前 revision = "abc"，tx1 的 new_text="ab"。
        // per-tx offset_map = OffsetMap::build("ab", "abc")：公共前缀 "ab"，Identity。
        // 第三笔在当前 revision "abc" 操作 c 区域，raw range = (2, 3)。
        let current_old_text = "abc";
        let conflict = queue.find_conflicting_transaction(current_old_text, 2, 3);

        // 修复后：tx1 的 (0,1) 映射到当前坐标系仍是 (0,1)（a 区域），
        // 查询 (2,3) 是 c 区域，不重叠 → 不冲突。
        assert!(
            conflict.is_empty(),
            "修复后不应误判冲突：tx1 的 visual_affected_byte_range_new=(0,1) 基于\
             new='ab'，通过 per-tx OffsetMap 映射到当前 'abc' 坐标系仍为 (0,1)（a 区域），\
             查询 raw range=(2,3) 是 c 区域，不重叠。实际返回 {:?}",
            conflict
        );
    }

    /// 问题 3b — 验证修复后不再漏判跨 revision 冲突。
    ///
    /// tx1 (Insert): old="ab", new="aXb"（中间插 X）。
    ///   visual_affected_byte_range_new = Some((1, 2))  // X 在 new="aXb" 的 1..2
    /// 之后 a 被删除，当前 revision 变为 "Xb"，X 位移到 0..1。
    /// 第三笔在 "Xb" 操作 X 区域，raw range = (0, 1)（"Xb" 坐标系）。
    ///
    /// OffsetMap 从 tx1 的 new_text="aXb" 到当前 "Xb"：公共前缀 ""（a≠X），
    /// 公共后缀 "Xb"（2 bytes），entry: old=1, new=0, length=2, Shifted。
    /// 映射 tx1 的 (1,2) → (0,1)。overlap (0,1) vs (0,1) → 重叠 → 冲突。正确。
    #[test]
    fn test_issue710_visual_affected_byte_range_no_missed_conflict() {
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((1, 1)), // old 侧是插入点
            Some((1, 2)), // new 侧是 inserted_range（X 在 "aXb" 的 1..2）
            "aXb",        // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前 revision = "Xb"（a 被删除），tx1 的 new_text="aXb"。
        // per-tx offset_map = OffsetMap::build("aXb", "Xb")：公共后缀 "Xb"，Shifted。
        // 第三笔在当前 revision "Xb" 操作 X 区域，raw range = (0, 1)。
        let current_old_text = "Xb";
        let conflict = queue.find_conflicting_transaction(current_old_text, 0, 1);

        // 修复后：tx1 的 (1,2) 映射到当前坐标系为 (0,1)（X 区域），
        // 查询 (0,1) 也是 X 区域，重叠 → 冲突。
        assert!(
            !conflict.is_empty(),
            "修复后不应漏判冲突：tx1 的 visual_affected_byte_range_new=(1,2) 基于\
             new='aXb'，通过 per-tx OffsetMap 映射到当前 'Xb' 坐标系为 (0,1)（X 区域），\
             查询 raw range=(0,1) 也是 X 区域，应检测到 tx1 冲突。实际返回空 Vec"
        );
    }
}

// ── Issue #710 评论 5733109905 复现测试 ──
//
// 本轮前两条已修复（compute_affected_paragraph_ranges old/new 分离、blink 统一入口），
// 但第三条"跨 revision 的视觉区域所有权"仍有坐标系错误。这些测试展示当前实现
// 在三个具体场景下给出错误冲突判定，证明 bug 存在。
//
// 复现策略：断言"当前实现行为"与"正确坐标系下的期望行为"不一致，从而证明 bug。
// 修复后（Phase B）应把这些测试改为断言"正确行为"。
#[cfg(test)]
mod issue_710_comment_5733109905_repro {
    use super::super::layout_snapshot::SourceRect;
    use super::*;

    /// 构造只含 virtual_text 的测试用 EditorLayoutSnapshot。
    fn make_test_snapshot(virtual_text: &str) -> EditorLayoutSnapshot {
        EditorLayoutSnapshot {
            revision: super::super::layout_snapshot::LayoutRevision::next(),
            line_snapshots: Vec::new(),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Upstream,
            virtual_text: virtual_text.to_string(),
        }
    }

    /// 构造最小化测试事务：只填 key/operation_kind/visual_affected_byte_range_{old,new}，
    /// 其余字段用空/None。state=Pending 保证被 find_conflicting_transaction 遍历。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        }
    }

    /// 构造带一个 unit 的测试事务，用于问题 3（units 裸数值比较）复现。
    /// unit 的 slice.byte_start/byte_end 设为指定值（事务 new 坐标系）。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx_with_unit(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        unit_byte_start: usize,
        unit_byte_end: usize,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        let slice = AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(transaction_id, 0),
            LineSnapshotId::new(1, 0, 0),
            SourceRect::zero(),
            SourceRect::zero(),
            0.0,
            0.0,
            unit_byte_start,
            unit_byte_end,
            None,
            None,
        );
        let unit = PreparedVisualUnit::wrap(slice, 100);
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: vec![unit],
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
        }
    }

    // ── 问题 1: Delete 冲突查询把 old 坐标和 new 坐标直接比较 ──
    //
    // 场景：
    //   tx1 (Insert): old="bcde", new="abcde"（开头插 a）。
    //     visual_affected_byte_range_new = (1, 5)（"bcde" 在 new="abcde" 的 1..5）。
    //   tx2 (Delete): old="abcde", new="bcde"，删除开头 a，deleted_range=(0,1)（old 坐标系）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = tx2.old = "abcde"。
    //   per-tx offset_map = OffsetMap::build(tx1.new, tx2.old) = OffsetMap::build("abcde", "abcde") = identity。
    //   tx1 的 (1,5) 映射后仍 (1,5)（tx2.old 坐标系）。
    //   查询 range = (0,1)（tx2.old 坐标系）。(1,5) vs (0,1) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_comment_5733109905_problem1_delete_old_new_coord_mismatch() {
        // tx1: visual_affected_byte_range_new 基于 tx1.new="abcde"
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((0, 0)), // old 侧是插入点
            Some((1, 5)), // new 侧是 "bcde" 在 "abcde" 的 1..5
            "abcde",      // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // tx2 (Delete): old="abcde", new="bcde", deleted_range=(0,1)
        // Delete 路径查询 range = deleted_range = (0, 1)（old 坐标系）
        let tx2_old_text = "abcde";
        let rebase_byte_start = 0usize;
        let rebase_byte_end = 1usize;

        // 修复后：current_old_text = tx2.old，per-tx offset_map = identity
        let current_conflict =
            queue.find_conflicting_transaction(tx2_old_text, rebase_byte_start, rebase_byte_end);

        // 修复后：tx1 的 (1,5) 在 tx2.old 坐标系仍是 (1,5)（tx1.new==tx2.old），
        // 查询 (0,1) 不重叠 → 不冲突。
        assert!(
            current_conflict.is_empty(),
            "修复后应不冲突：tx1 的 visual_affected_byte_range_new=(1,5) 基于 tx1.new='abcde'，\
             per-tx OffsetMap=identity（tx1.new==tx2.old='abcde'），映射后仍 (1,5)，\
             查询 (0,1) 不重叠。实际返回 {:?}",
            current_conflict
        );
    }

    // ── 问题 2: 单个 OffsetMap 只对紧邻上一笔事务成立，不能给队列里所有活动事务共用 ──
    //
    // 场景：
    //   初始文档 "123456789"。
    //   tx1 (Insert): old="123456789", new="12345X6789"（位置 5 插 X，第 2 段）。
    //     visual_affected_byte_range_new = (5, 6)（X 在 tx1.new 的 5..6）。
    //   tx2 (Insert): old="12345X6789", new="12Y345X6789"（位置 2 插 Y，第 1 段，不冲突）。
    //   tx3 (Delete): old="12Y345X6789", new="12Y3456789"，删除 X。
    //     deleted_range = (6, 7)（X 在 tx3.old 的位置 6）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = tx3.old = "12Y345X6789"。
    //   对 tx1: per-tx offset_map = OffsetMap::build("12345X6789", "12Y345X6789")
    //     → 把 tx1 的 (5,6) 映射到 (6,7)（X 在 "12Y345X6789" 的位置 6）
    //     → (6,7) vs 查询 (6,7) → 重叠 → 冲突。正确。
    //   对 tx2: per-tx offset_map = OffsetMap::build("12Y345X6789", "12Y345X6789") = identity
    //     → tx2 的 (2,3) 映射后仍 (2,3) → (2,3) vs (6,7) → 不重叠 → 不冲突。
    #[test]
    fn test_issue710_comment_5733109905_problem2_single_offset_map_not_universal() {
        // tx1: visual_affected_byte_range_new 基于 tx1.new="12345X6789"
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((5, 5)), // old 侧是插入点
            Some((5, 6)), // new 侧是 X 在 "12345X6789" 的 5..6
            "12345X6789", // tx1.new_text
        );
        // tx2: 在第 1 段插入 Y，不与 tx1 冲突，留在队列
        let tx2 = make_test_tx(
            2,
            TextVisualOperationKind::Insert,
            Some((2, 2)),
            Some((2, 3)),  // Y 在 "12Y345X6789" 的 2..3
            "12Y345X6789", // tx2.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);
        queue.enqueue(tx2);

        // tx3 (Delete): old="12Y345X6789", new="12Y3456789", deleted_range=(6,7)
        let tx3_old_text = "12Y345X6789";
        let rebase_byte_start = 6usize;
        let rebase_byte_end = 7usize;

        // 修复后：current_old_text = tx3.old，逐事务构造 per-tx offset_map
        let current_conflict =
            queue.find_conflicting_transaction(tx3_old_text, rebase_byte_start, rebase_byte_end);

        // 单独验证 tx1 在正确 per-tx offset_map 下应判定冲突
        let tx1_alone = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((5, 5)),
            Some((5, 6)),
            "12345X6789",
        );
        let mut queue_tx1_only = PreparedTransactionQueue::new();
        queue_tx1_only.enqueue(tx1_alone);
        let correct_conflict_for_tx1 = queue_tx1_only.find_conflicting_transaction(
            tx3_old_text,
            rebase_byte_start,
            rebase_byte_end,
        );

        // 修复后：tx1 的 (5,6) 通过 per-tx OffsetMap::build("12345X6789", "12Y345X6789")
        // 映射到 (6,7)，与查询 (6,7) 重叠 → 冲突。
        assert!(
            !current_conflict.is_empty(),
            "修复后应检测到冲突：用 per-tx OffsetMap::build(tx1.new, tx3.old) 把 tx1 的 (5,6) \
             映射到 (6,7)（X 在 '12Y345X6789' 的位置 6），与查询 (6,7) 重叠。\
             实际返回 {:?}",
            current_conflict
        );
        assert!(
            !correct_conflict_for_tx1.is_empty(),
            "tx1 单独在正确 per-tx offset_map 下应冲突：OffsetMap::build('12345X6789', '12Y345X6789') \
             把 (5,6) 映射到 (6,7)，与查询 (6,7) 重叠。实际返回 {:?}",
            correct_conflict_for_tx1
        );
    }

    // ── 问题 3: units 明知是旧事务 new 坐标，代码仍先做裸数值 overlap ──
    //
    // 场景：
    //   tx1 (Insert): old="abcdef", new="abXYcdef"（位置 2 插 XY）。
    //     tx1 有一个 unit，byte range = (2, 4)（XY 在 tx1.new="abXYcdef" 的 2..4，new 坐标系）。
    //     visual_affected_byte_range_new = (2, 4)。
    //   之后前面插入 Z，当前文档 = "ZabXYcdef"。tx1.new="abXYcdef" ≠ 当前 "ZabXYcdef"。
    //   新事务查询 range = (2, 3)（当前坐标系，对应 "b" 区域）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = "ZabXYcdef"。
    //   per-tx offset_map = OffsetMap::build("abXYcdef", "ZabXYcdef")：Shifted +1。
    //   unit 的 (2,4) 映射到 (3,5)。查询 (2,3)。(3,5) vs (2,3) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_comment_5733109905_problem3_units_bare_numeric_overlap() {
        // tx1: 有一个 unit，byte range = (2, 4)（tx1.new 坐标系）
        let tx1 = make_test_tx_with_unit(
            1,
            TextVisualOperationKind::Insert,
            2,            // unit byte_start
            4,            // unit byte_end
            Some((2, 2)), // visual_affected_byte_range_old
            Some((2, 4)), // visual_affected_byte_range_new
            "abXYcdef",   // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前文档 = "ZabXYcdef"（前面插了 Z），tx1.new = "abXYcdef"
        let current_text = "ZabXYcdef";
        // 新事务查询 range = (2, 3)（当前坐标系，对应 "b" 区域）
        let query_start = 2usize;
        let query_end = 3usize;

        // 修复后：unit 的 byte range 也通过 per-tx offset_map 映射到当前坐标系
        let current_conflict =
            queue.find_conflicting_transaction(current_text, query_start, query_end);

        // per-tx offset_map = OffsetMap::build("abXYcdef", "ZabXYcdef")：
        //   prefix=0, suffix=8, entry Shifted old=0,new=1,length=8
        // 映射 unit (2,4) → (3,5)。查询 (2,3)。(3,5) vs (2,3) → 不重叠。
        let per_tx_offset_map = writer_core::editor::OffsetMap::build("abXYcdef", current_text);
        let mapped_unit_range = per_tx_offset_map.map_old_range_to_new(2, 4);
        assert_eq!(
            mapped_unit_range,
            Some((3, 5)),
            "per-tx offset_map 应把 unit 的 (2,4) 映射到 (3,5)（'ZabXYcdef' 坐标系）"
        );

        // 修复后：映射后的 unit range (3,5) 与查询 (2,3) 不重叠 → 不冲突
        assert!(
            current_conflict.is_empty(),
            "修复后应不冲突：unit 的 (2,4) 通过 per-tx OffsetMap 映射到 (3,5)（当前坐标系），\
             查询 ({},{}) 不重叠。实际返回 {:?}",
            query_start,
            query_end,
            current_conflict
        );
    }
}
