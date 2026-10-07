//! Issue #826 评论 38：协同模式下光标与文字前沿共享的单条 caret 运动轨迹。
//!
//! # 为什么需要这一层
//!
//! 评论 36 之前，`EditFrontierState`（正文吞吐）与 `CursorAnimationState`
//! （视觉光标）是两条完全独立的时间线：各自的 `started_at`、各自的
//! duration（typing duration vs smooth cursor duration），只在 Scene Graph
//! 帧里碰巧用了同一个 `frame_now`。于是协同开关打开时仍然出现
//! 「光标 80ms 先到、文字 160ms 还在吐」的各跑各的（评论 38 的实机反例）。
//!
//! # 协同语义（评论 38 收口定义）
//!
//! `协同模式 = 一条 caret 运动轨迹 + 文字以 caret 当前帧为吞吐边界 + Reflow 可独立`
//!
//! - 同一笔正文编辑只建**一份**当前态 [`CoordinatedCaretMotion`]（不是历史
//!   transaction）：`start = 上一帧真实 visual caret`、`target = 最新 canonical
//!   caret`、`started_at = 与前沿统一的 edit 起点`、`duration = typing duration`。
//! - 每帧只采一次：光标直接画采样位置；前沿用同一个 progress
//!   （同一 `started_at` + 同一 `duration` + 同一条 `ease_out_cubic`，progress
//!   天然相等）推进吞吐边界。Reflow 仍然独立。
//! - 连续输入只 retarget 这一份 motion（从当前屏幕位置继续），不排 per-key
//!   queue，活跃 motion 数量始终最多 1（`Option` 本身就是证明）。
//! - 非协同模式不用这一层：typing 前沿与 smooth cursor 保持各自独立 timeline。
//!
//! # 所有权
//!
//! 这一份 motion 由 [`LinuxEditorAnimationCoordinator`](super::coordinator::LinuxEditorAnimationCoordinator)
//! 持有（`active_coordinated_caret: Option<CoordinatedCaretMotion>`），与前沿同生共死：
//! 前沿换 burst / 收成 canonical / 被 suppress 时，它一起被重建或清掉。
//! `CursorController` 在协同期间不建独立 Tween（`animation == None`），
//! 每帧视觉位置由 Scene Graph 帧从这里采样后写入。

use std::time::{Duration, Instant};

use super::edit_frontier::{ease_out_cubic, FrontierRegion};

/// Issue #826 评论 38：协同模式下**唯一**的 caret 运动轨迹。
///
/// 当前态对象：没有历史队列、没有 per-key track。连续编辑只改它的
/// start/target（retarget），`started_at`/`duration_ms` 永远与当前前沿一致。
#[derive(Clone, Debug)]
pub(crate) struct CoordinatedCaretMotion {
    /// 上一帧真实 visual caret（文档坐标 x）。
    pub start_x: f64,
    /// 上一帧真实 visual caret（文档坐标 y）。
    pub start_y: f64,
    /// 最新 canonical caret（文档坐标 x）。
    pub target_x: f64,
    /// 最新 canonical caret（文档坐标 y）。
    pub target_y: f64,
    /// 与前沿统一的时间起点：协同创建/retarget 时直接取
    /// `active_edit_frontier.started_at`，不另取 `Instant::now()`。
    pub started_at: Instant,
    /// 与前沿统一的时长：永远是 typing duration，不是 smooth cursor duration。
    pub duration_ms: u64,
}

impl CoordinatedCaretMotion {
    /// 按 `now` 采样 0..1 进度（与 [`EditFrontierState::sample`](super::edit_frontier::EditFrontierState::sample)
    /// 同公式：同一 `started_at` + 同一 `duration` ⇒ 同一 progress）。
    pub(crate) fn sample_progress(&self, now: Instant) -> f64 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed_ms = now.saturating_duration_since(self.started_at).as_secs_f64()
            / Duration::from_millis(self.duration_ms).as_secs_f64();
        elapsed_ms.clamp(0.0, 1.0)
    }

    /// 给定 progress 下的 caret 位置（与 `CursorAnimationState::current_position`
    /// 同一条 `ease_out_cubic` 缓动，保证光标画的位置就是前沿吞吐边界的位置）。
    pub(crate) fn position_at(&self, progress: f64) -> (f64, f64) {
        let eased = ease_out_cubic(progress);
        (
            self.start_x + (self.target_x - self.start_x) * eased,
            self.start_y + (self.target_y - self.start_y) * eased,
        )
    }

    /// 滚动 pause / resume 用：与前沿三层一起平移起点，恢复后仍从 pause 时刻继续。
    pub(crate) fn shift_started_at(&mut self, delta: Duration) {
        self.started_at += delta;
    }
}

/// Issue #826 评论 38：协同 caret 的一帧采样结果。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoordinatedCaretSample {
    pub x: f64,
    pub y: f64,
    pub progress: f64,
    /// progress 到 1：调用方把视觉光标精确落到 target，本 motion 已清掉。
    pub finished: bool,
}

/// Issue #826 评论 38：本帧协同 caret 投影到前沿路径上的吞吐距离。
///
/// 前沿遮罩/overlay 不再用自己的 `advanced(progress)`，而用这里的距离 ——
/// caret 几何与 glyph 几何天然差几个像素（实测约 2.5px：caret 矩形与 cluster
/// 矩形的定位基准不同），只共享 progress 会让「光标已到、边界还差一点」。
/// 投影保证同一帧 `boundary_after(distance) == caret`（浮点舍入内精确）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoordinatedBoundary {
    /// 吐字侧总距离（`distance_start` 坐标系，与 `advanced()` 同空间）。
    pub reveal_distance: f64,
    /// 吞字侧总距离。
    pub conceal_distance: f64,
}

/// 把本帧协同 caret 位置投影到一侧前沿路径上，返回总距离。
///
/// 按 caret 的 y 找同一视觉行的段（`seg.y <= y < seg.y + seg.h`），再按 x 在
/// 段内的比例算距离；x 超出段两端时钳制到端点（caret 走到头 ⇒ 该行播完）。
/// caret 的 y 落在任何段之外（跨行补间穿过行间缝隙时）返回 None，调用方回退
/// 到 progress 时钟 —— 两帧之内 y 就会进入下一行的段。
pub(crate) fn project_onto_layer(
    regions: &[FrontierRegion],
    x: f64,
    y: f64,
) -> Option<f64> {
    for region in regions {
        let mut prefix = 0.0;
        for segment in &region.path.segments {
            if y >= segment.y && y < segment.y + segment.h {
                let span = segment.x_to - segment.x_from;
                let frac = if span.abs() <= f64::EPSILON {
                    1.0
                } else {
                    ((x - segment.x_from) / span).clamp(0.0, 1.0)
                };
                return Some(region.distance_start + prefix + frac * segment.visual_length);
            }
            prefix += segment.visual_length;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion() -> CoordinatedCaretMotion {
        CoordinatedCaretMotion {
            start_x: 10.0,
            start_y: 20.0,
            target_x: 30.0,
            target_y: 20.0,
            started_at: Instant::now(),
            duration_ms: 160,
        }
    }

    #[test]
    fn coordinated_motion_progress_follows_typing_duration() {
        let m = motion();
        let t0 = m.started_at;
        assert!((m.sample_progress(t0) - 0.0).abs() < 1e-9);
        let mid = m.sample_progress(t0 + Duration::from_millis(80));
        assert!(mid > 0.0 && mid < 1.0, "80/160ms 必须在中间，实际 {}", mid);
        assert!((m.sample_progress(t0 + Duration::from_millis(160)) - 1.0).abs() < 1e-9);
        assert!((m.sample_progress(t0 + Duration::from_millis(999)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn coordinated_motion_position_uses_same_easing_as_frontier() {
        // 前沿 `FrontierLayer::advanced` 用 `ease_out_cubic(progress)`；
        // motion 必须用同一条曲线，否则同 progress 下 caret.x 与吞吐边界 x 对不上。
        let m = motion();
        let (x0, _) = m.position_at(0.0);
        assert!((x0 - 10.0).abs() < 1e-9);
        let (x1, _) = m.position_at(1.0);
        assert!((x1 - 30.0).abs() < 1e-9);
        let (xm, _) = m.position_at(0.5);
        let expected = 10.0 + 20.0 * ease_out_cubic(0.5);
        assert!((xm - expected).abs() < 1e-9);
    }

    #[test]
    fn coordinated_motion_retarget_is_continuous_from_current_sample() {
        // 40ms 处第二笔：新 motion 的 start 必须等于旧 motion 当前采样位置，
        // 不能退回逻辑旧 caret（否则光标跳）。
        let m = motion();
        let t0 = m.started_at;
        let now = t0 + Duration::from_millis(40);
        let (cur_x, cur_y) = m.position_at(m.sample_progress(now));
        let retargeted = CoordinatedCaretMotion {
            start_x: cur_x,
            start_y: cur_y,
            target_x: 50.0,
            target_y: 20.0,
            started_at: now,
            duration_ms: 160,
        };
        let (rx0, _) = retargeted.position_at(retargeted.sample_progress(now));
        assert!(
            (rx0 - cur_x).abs() < 1e-9,
            "retarget 首帧必须等于旧 motion 当前采样，实际 {} vs {}",
            rx0,
            cur_x
        );
    }
}
