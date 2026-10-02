use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};

/// 事务级时钟 — 只提供事务层级的时间推进/pause 语义。
///
/// Issue #815 评论 6042062633 修改 2: 事务级时钟不驱动任何具体视觉单元。
/// 协同模式下 InsertReveal/DeleteConceal 的逐帧边界来自同一笔事务的 cursor track
/// 当前帧（见 [`VisualUnitTiming::CaretTrack`]），非协同文字动画和 Reflow 来自各自
/// `VisualUnitTiming::Timed` 时间线，光标来自 `PreparedCursorVisualTrack`。
/// 本事务 timeline 只服务"事务级状态/完成判断"（例如 units 为空的 cursor-only 事务）。
///
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

/// Issue #815 评论 6042062633 修改 2: 视觉单元的计时语义分成两种驱动。
///
/// 协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立。
/// 不再是"两条互不相干的时间线，只在起点位置看起来碰巧挨着"。
///
/// - [`VisualUnitTiming::Timed`]：单元自带 started_at / duration_ms / progress，用
///   文字自己的 `ease_out_quad` 推进。覆盖：
///   - 非协同的 InsertReveal/DeleteConceal（跟随「打字动画」设置）
///   - ReflowMove / ReflowCrossFade（始终独立播放，不被 caret 轨迹接管）
/// - [`VisualUnitTiming::CaretTrack`]：单元**没有自己的时间线**。本帧的吞吐边界
///   完全来自同一笔事务 cursor track 的当前帧（`sample_coordinated_motion_frame`
///   的唯一一次采样），文字层与光标层消费同一份采样，文字不再自己算一次时间。
///   覆盖协同模式的 InsertReveal/DeleteConceal。
#[derive(Clone, Debug)]
pub(crate) enum VisualUnitTiming {
    /// 独立时间线驱动。文字用 `ease_out_quad`，光标 track 用 `ease_out_cubic`。
    Timed {
        started_at: Option<Instant>,
        duration_ms: u64,
        start_fraction: f64,
        target_fraction: f64,
    },
    /// Issue #815 评论 6042062633 修改 2: 协同模式的吞吐字。
    ///
    /// 没有 started_at / duration_ms——协同吞吐字不再拥有独立 progress。
    /// 逐帧 clip 由 `AnimatedSlice::compute_frame_by_caret_ingest` 直接用 cursor
    /// track 当前帧的 caret.x 算出，不经过 0..1 visible fraction。
    ///
    /// `retired` 表示这笔事务已经失去 caret motion ownership（epoch 切换 / layout
    /// basis 过期）。retired 后本单元立即收口到终态：`progress` 返回 1.0，让事务
    /// 的完成判断不再等一条已经不推进的 caret track。
    CaretTrack { retired: bool },
}

impl VisualUnitTiming {
    /// 从 `AnimatedSliceKind` 推断默认计时语义。
    /// 所有 kind 都返回 `Timed`（要 caret 驱动必须走
    /// [`VisualUnitTiming::default_for_kind_with_coordinated`]）。
    pub(crate) fn default_for_kind(kind: AnimatedSliceKind, duration_ms: u64) -> Self {
        let target_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 0.0,
            _ => 1.0,
        };
        let start_fraction = match kind {
            AnimatedSliceKind::DeleteConceal => 1.0,
            _ => 0.0,
        };
        VisualUnitTiming::Timed {
            started_at: None,
            duration_ms,
            start_fraction,
            target_fraction,
        }
    }

    /// Issue #815 评论 6042062633 修改 2: 按 `coordinated` 决定 InsertReveal/DeleteConceal
    /// 的计时驱动。
    ///
    /// - `coordinated=true`：InsertReveal/DeleteConceal 返回
    ///   [`VisualUnitTiming::CaretTrack`]，不再拥有自己的 `ease_out_quad + text_duration_ms`
    ///   progress，也不再调用 `current_visible_fraction(now)` 决定吞吐边界。它们与光标
    ///   共用同一笔 `cursor_visual_track` 的当前帧。
    /// - `coordinated=false`：仍是独立 `Timed`，按「打字动画」设置自己推进
    ///   （`is_caret_line`/`caret_anchor_x` 仍表达遮罩锚点）。
    /// - ReflowMove / ReflowCrossFade：**始终**独立 `Timed`，协同模式也不接管，
    ///   继续按 `text_duration_ms` 播放。
    pub(crate) fn default_for_kind_with_coordinated(
        kind: AnimatedSliceKind,
        duration_ms: u64,
        coordinated: bool,
    ) -> Self {
        if coordinated
            && matches!(
                kind,
                AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal
            )
        {
            return VisualUnitTiming::CaretTrack { retired: false };
        }
        Self::default_for_kind(kind, duration_ms)
    }

    /// 是否仍由 caret track 当前帧驱动吞吐边界（未被 retire 的 CaretTrack unit）。
    pub(crate) fn is_caret_driven(&self) -> bool {
        matches!(self, VisualUnitTiming::CaretTrack { retired: false })
    }

    /// 是否是 CaretTrack 单元（不管有没有被 retire）。
    ///
    /// 完成判断、rebase 交棒、静态守卫都用这个：retired 的 CaretTrack 单元
    /// 同样没有独立时间线，不能对它调 `rebase_from_frame` 重置 duration。
    pub(crate) fn is_caret_track(&self) -> bool {
        matches!(self, VisualUnitTiming::CaretTrack { .. })
    }

    /// Issue #815: 事务失去 caret motion ownership 时收口所有 CaretTrack 单元。
    pub(crate) fn retire_caret_motion(&mut self) {
        if let VisualUnitTiming::CaretTrack { retired } = self {
            *retired = true;
        }
    }

    /// 获取 `start_fraction`（rebase 交棒载体）。
    pub fn start_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::Timed { start_fraction, .. } => *start_fraction,
            // CaretTrack 没有自己的进度载体：retired 给终态，未 retired 给起始态。
            // 两者都不会被用来驱动逐帧 clip。
            VisualUnitTiming::CaretTrack { retired } => {
                if *retired {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// 获取 `target_fraction`。
    pub fn target_fraction(&self) -> f64 {
        match self {
            VisualUnitTiming::Timed {
                target_fraction, ..
            } => *target_fraction,
            VisualUnitTiming::CaretTrack { .. } => 1.0,
        }
    }

    /// 从自己的 `started_at` / `duration_ms` 计算当前 progress（0..1）。
    /// Issue #815: CaretTrack 没有自己的时间线；未 retired 时返回 0，retired 后返回 1。
    pub fn progress(&self, now: Instant) -> f64 {
        match self {
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
            VisualUnitTiming::CaretTrack { retired } => {
                if *retired {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// 单元在 `now` 时刻的独立可见比例（0..1）。
    ///
    /// Issue #815: 只有 `Timed` 会用这个值驱动逐帧 clip
    ///（`start_fraction + (target_fraction - start_fraction) * ease_out_quad(progress)`）。
    /// `CaretTrack` 不允许消费它——协同吞吐字的边界是 caret.x 本身。
    pub fn current_visible_fraction(&self, now: Instant) -> f64 {
        match self {
            VisualUnitTiming::Timed {
                start_fraction,
                target_fraction,
                ..
            } => {
                let eased = AnimatedSlice::ease_out_quad(self.progress(now));
                (start_fraction + (target_fraction - start_fraction) * eased).clamp(0.0, 1.0)
            }
            VisualUnitTiming::CaretTrack { retired } => {
                if *retired {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// Issue #690 评论 5683759796: 在事务进入 Rendering 时打上统一起始时间。
    /// Issue #815: 只有 `Timed` 设置 started_at；`CaretTrack` 的起点在 cursor track 上。
    pub fn mark_started(&mut self, frame_now: Instant) {
        if let VisualUnitTiming::Timed { started_at, .. } = self {
            if started_at.is_none() {
                *started_at = Some(frame_now);
            }
        }
    }

    /// Issue #690 评论 5683759796 / Issue #815: rebase 交棒时更新可见比例载体。
    ///
    /// 只有 `Timed` 更新 start_fraction + 重置时间线（started_at = None,
    /// duration = remaining）。`CaretTrack` 是 no-op：它的连续性由
    /// `RebaseCaretHandoff` 承担（先采样旧 track 当前帧，新 track 从这个当前
    /// caret 连到新的目标 caret，不退回逻辑旧 caret）。
    pub fn rebase_from_frame(&mut self, visible_fraction: f64, remaining_duration_ms: u64) {
        match self {
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
            VisualUnitTiming::CaretTrack { .. } => {}
        }
    }
}
