use std::time::Instant;

use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};

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
    pub(crate) fn default_for_kind(kind: AnimatedSliceKind, duration_ms: u64) -> Self {
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
