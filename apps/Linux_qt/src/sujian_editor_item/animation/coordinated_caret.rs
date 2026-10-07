//! Issue #826 评论 38/39：协同模式下光标与文字前沿共享的单条 caret 运动轨迹。
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
//!   transaction）：时钟（`started_at` / `duration_ms`）与前沿统一，
//!   `duration` 永远是 typing duration。
//! - 每帧先由同一 progress 算出 caret 在分段轨迹上的位置（沿行段走，跨行
//!   切段，绝不对起点终点拉斜线），Reveal/Conceal 边界再由该位置投影到前沿
//!   路径 —— 真正"一条轨迹"，而不是 x/y 斜线 + progress fallback
//!   （评论 39 BLOCKER 2）。
//! - 连续输入只 retarget 这一份 motion（按当前前沿重建路径、首点取旧 motion
//!   当前位置，从当前屏幕位置继续），不排 per-key queue，活跃 motion 数量
//!   始终最多 1（`Option` 本身就是证明）。
//! - 非协同模式不用这一层：typing 前沿与 smooth cursor 保持各自独立 timeline。
//!
//! # 生命周期（评论 39 BLOCKER 1）
//!
//! motion 自己就是本笔正文协同 clock：`progress == 1` 时自己结束，不依附
//! "scalar Frontier 还有没有可见 path"。Enter（Reveal path 为空、只有 Reflow
//! 在跑）或 shaping 全接管时，前沿第一帧就 finished，motion 必须继续走到
//! target。显式收口（suppress / 点击 / 独立光标接管 / finish-to-canonical）
//! 时才清掉。
//!
//! # 所有权
//!
//! 这一份 motion 由 [`LinuxEditorAnimationCoordinator`](super::coordinator::LinuxEditorAnimationCoordinator)
//! 持有（`active_coordinated_caret: Option<CoordinatedCaretMotion>`）。
//! `CursorController` 在协同期间不建独立 Tween（`animation == None`），
//! 每帧视觉位置由 Scene Graph 帧从这里采样后写入。

use std::time::{Duration, Instant};

use super::edit_frontier::{ease_out_cubic, FrontierRegion};

/// Issue #826 评论 39 BLOCKER 2：caret 运动轨迹上的一段。
///
/// 与本笔 changed visual path 同一视觉顺序：同行一段，跨行按行进方向串起来。
/// Backward（Backspace）时段序与吞字路径一致是反向的 —— 构造时直接从 conceal
/// 侧取段，不在这里再反转。
///
/// 首段起点钉死在创建/retarget 时的屏幕 caret（`x_from = visual`），末段终点
/// 钉死在最新 canonical caret（`x_to = target`）：glyph 矩形与 caret 矩形的
/// 定位基准天然差几个像素（bearings），不对齐首尾的话第一帧跳、最后一帧还
/// 得 snap 回来。钉死后途中 caret 仍在行段上走，终点精确落 canonical。
#[derive(Clone, Debug)]
pub(crate) struct CaretMotionSegment {
    /// 进入这一段时的 x（方向起点；首段恒为创建时的屏幕 caret x）。
    pub x_from: f64,
    /// 走完这一段时的 x（方向终点；末段恒为 canonical target x）。
    pub x_to: f64,
    /// 这一段所属视觉行的文档坐标上边界。
    pub y: f64,
    /// 该视觉行的高度。
    pub h: f64,
    /// 这一段的长度（钉死后重算的 `|x_to - x_from|`，caret-only 段用欧氏距离）。
    pub visual_length: f64,
}

impl CaretMotionSegment {
    /// 沿本段走过 `take` 距离后的 x。
    fn x_after(&self, take: f64) -> f64 {
        let take = take.clamp(0.0, self.visual_length);
        let span = self.x_to - self.x_from;
        if span.abs() <= f64::EPSILON || self.visual_length <= f64::EPSILON {
            self.x_to
        } else {
            self.x_from + span * (take / self.visual_length)
        }
    }
}

/// Issue #826 评论 38/39：协同模式下**唯一**的 caret 运动轨迹。
///
/// 当前态对象：没有历史队列、没有 per-key track。连续编辑只 retarget 它
/// （按当前前沿路径重建分段、首点取旧 motion 当前位置，`started_at` /
/// `duration_ms` 跟着当前前沿走）。
#[derive(Clone, Debug)]
pub(crate) struct CoordinatedCaretMotion {
    /// 与本笔 changed visual path 同顺序的分段轨迹（首尾已钉死 visual/target）；
    /// `CaretOnly` 时只有一段。
    pub segments: Vec<CaretMotionSegment>,
    /// 整条轨迹总长。
    pub total_length: f64,
    /// 与前沿统一的时间起点：协同创建/retarget 时直接取
    /// `active_edit_frontier.started_at`，不另取 `Instant::now()`。
    pub started_at: Instant,
    /// 与前沿统一的时长：永远是 typing duration，不是 smooth cursor duration。
    pub duration_ms: u64,
    /// 最新 canonical caret（文档坐标）：终点精确落点。
    ///
    /// 末段终点已钉死在这里，终点帧是自然到达不是 snap，不存在"先冲过头再
    /// 跳回来"。
    pub target_x: f64,
    /// 最新 canonical caret（文档坐标 y）。
    pub target_y: f64,
}

impl CoordinatedCaretMotion {
    /// 按 `now` 采样 0..1 进度（与 [`EditFrontierState::sample`](super::edit_frontier::EditFrontierState::sample)
    /// 同公式：同一 `started_at` + 同一 `duration` ⇒ 同一 progress）。
    pub(crate) fn sample_progress(&self, now: Instant) -> f64 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_secs_f64()
            / Duration::from_millis(self.duration_ms).as_secs_f64();
        elapsed.clamp(0.0, 1.0)
    }

    /// 本帧已走过的轨迹距离（与 `FrontierLayer::advanced` 同公式，只是起点
    /// 恒为 0 —— retarget 靠重建路径 + 首点取旧位置保证连续，不靠继承距离）。
    pub(crate) fn distance_at_progress(&self, progress: f64) -> f64 {
        self.total_length.max(0.0) * ease_out_cubic(progress)
    }

    /// 轨迹上走过 `distance` 后的 (x, y)。
    ///
    /// Issue #826 评论 39 BLOCKER 2：跨行时沿分段轨迹走（行内沿 x、前进到
    /// 段末再切下一行），绝不对 (x, y) 起点终点拉斜线 —— 斜线会穿过行间
    /// 缝隙，那里不属于任何文字行，不可能是吞吐边界。
    pub(crate) fn position_at_distance(&self, distance: f64) -> (f64, f64) {
        let mut rest = distance.clamp(0.0, self.total_length.max(0.0));
        let mut last = (self.target_x, self.target_y);
        for segment in &self.segments {
            last = (segment.x_to, segment.y);
            if rest <= segment.visual_length + 1e-9 {
                return (segment.x_after(rest), segment.y);
            }
            rest -= segment.visual_length;
        }
        // 走完（浮点余量）：落在最后一段终点；调用方终点帧会精确 snap 到 target。
        last
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

/// Issue #826 评论 38/39：本帧协同 caret 投影到两侧前沿路径上的吞吐距离。
///
/// 吐字/吞字都由本帧 caret 位置经 [`project_onto_layer`] 投影得到
/// （`CaretOnly` 时两侧 path 本来就空，无实际消费者）。投影只认视觉身份，
/// 不引入第二条时间轴。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CoordinatedBoundary {
    /// 吐字侧总距离（`distance_start` 坐标系，与 `advanced()` 同空间）。
    pub reveal_distance: f64,
    /// 吞字侧总距离。
    pub conceal_distance: f64,
}

/// 把本帧协同 caret 位置投影到一侧前沿路径上，返回总距离。
///
/// - 同一视觉行可能有多个不相邻 region（多 patch / Replace / IME batch）：
///   先收齐同 y 的候选段，优先选 x 真正落在段内的；都没有命中再按 x 间隙选
///   最近段（钳制到端点），绝不 first-y-match 就返回（评论 39 BLOCKER 3）。
/// - x 超出段两端时钳制到端点（caret 走到头 ⇒ 该行播完）。
/// - caret 的 y 落在任何段之外（跨行补间穿过行间缝隙时）返回 None，调用方回退
///   到 progress 时钟 —— 两帧之内 y 就会进入下一行的段。
pub(crate) fn project_onto_layer(
    regions: &[FrontierRegion],
    x: f64,
    y: f64,
) -> Option<f64> {
    let mut nearest: Option<(f64, f64)> = None;
    for region in regions {
        let mut prefix = 0.0;
        for segment in &region.path.segments {
            if y >= segment.y && y < segment.y + segment.h {
                let lo = segment.x_from.min(segment.x_to);
                let hi = segment.x_from.max(segment.x_to);
                let span = segment.x_to - segment.x_from;
                let frac = if span.abs() <= f64::EPSILON {
                    1.0
                } else {
                    ((x - segment.x_from) / span).clamp(0.0, 1.0)
                };
                let distance = region.distance_start + prefix + frac * segment.visual_length;
                if x >= lo && x <= hi {
                    return Some(distance);
                }
                let gap = if x < lo { lo - x } else { x - hi };
                if nearest.map(|(_, best)| gap < best).unwrap_or(true) {
                    nearest = Some((distance, gap));
                }
            }
            prefix += segment.visual_length;
        }
    }
    nearest.map(|(distance, _)| distance)
}

#[cfg(test)]
mod tests {
    use super::super::edit_frontier::{FrontierPath, PathDirection};
    use super::*;
    use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
    use crate::sujian_editor_item::animation::edit_frontier::FrontierSegment;

    fn motion() -> CoordinatedCaretMotion {
        CoordinatedCaretMotion {
            segments: vec![CaretMotionSegment {
                x_from: 10.0,
                x_to: 30.0,
                y: 20.0,
                h: 20.0,
                visual_length: 20.0,
            }],
            total_length: 20.0,
            started_at: Instant::now(),
            duration_ms: 160,
            target_x: 30.0,
            target_y: 20.0,
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
    fn coordinated_motion_position_walks_path_not_lerp() {
        // 同 progress 下 caret 必须等于轨迹上同 distance 的点（与前沿边界同源），
        // 而不是起点终点拉斜线。
        let m = motion();
        let (x0, y0) = m.position_at_distance(m.distance_at_progress(0.0));
        assert!((x0 - 10.0).abs() < 1e-9 && (y0 - 20.0).abs() < 1e-9);
        let (x1, _) = m.position_at_distance(m.distance_at_progress(1.0));
        assert!((x1 - 30.0).abs() < 1e-9);
        let d = m.distance_at_progress(0.5);
        let (xm, _) = m.position_at_distance(d);
        let expected = 10.0 + 20.0 * ease_out_cubic(0.5);
        assert!((xm - expected).abs() < 1e-9, "实际 {} 期望 {}", xm, expected);
    }

    #[test]
    fn coordinated_motion_retarget_rebuilds_path_from_current_position() {
        // 40ms 处第二笔：新 motion 首段起点必须等于旧 motion 当前位置
        // （不对回逻辑旧 caret，不跳回轨迹起点），且首帧位置连续。
        let m = motion();
        let t0 = m.started_at;
        let now = t0 + Duration::from_millis(40);
        let base = m.distance_at_progress(m.sample_progress(now));
        let (cur_x, cur_y) = m.position_at_distance(base);
        // retarget 按新前沿重建路径：首段起点取旧位置（生产里由
        // coordinated_path_from_frontier 构造，这里直接模拟结果）。
        let retargeted = CoordinatedCaretMotion {
            segments: vec![CaretMotionSegment {
                x_from: cur_x,
                x_to: 50.0,
                y: cur_y,
                h: 20.0,
                visual_length: 50.0 - cur_x,
            }],
            total_length: 50.0 - cur_x,
            started_at: now,
            duration_ms: 160,
            target_x: 50.0,
            target_y: 20.0,
        };
        let d0 = retargeted.distance_at_progress(retargeted.sample_progress(now));
        let (rx0, ry0) = retargeted.position_at_distance(d0);
        assert!(
            (rx0 - cur_x).abs() < 1e-9 && (ry0 - cur_y).abs() < 1e-9,
            "retarget 首帧必须等于旧 motion 当前位置"
        );
    }

    fn regions_two_same_line() -> Vec<FrontierRegion> {
        let seg = |x_from: f64, x_to: f64| FrontierSegment {
            line_id: LineSnapshotId::new(0, 0, 0),
            y: 0.0,
            h: 20.0,
            x_left: x_from.min(x_to),
            x_right: x_from.max(x_to),
            x_from,
            x_to,
            visual_length: (x_to - x_from).abs(),
        };
        vec![
            FrontierRegion {
                range: (0, 1),
                path: FrontierPath {
                    segments: vec![seg(10.0, 20.0)],
                    total_length: 10.0,
                    direction: PathDirection::Forward,
                },
                distance_start: 0.0,
            },
            FrontierRegion {
                range: (5, 6),
                path: FrontierPath {
                    segments: vec![seg(80.0, 90.0)],
                    total_length: 10.0,
                    direction: PathDirection::Forward,
                },
                distance_start: 10.0,
            },
        ]
    }

    #[test]
    fn coordinated_projection_selects_correct_region_when_two_regions_share_one_line() {
        // Issue #826 评论 39 BLOCKER 3：同行两 region（10..20 / 80..90），
        // caret x=85 必须落 region B，不能 first-y-match 返回 A 末端。
        let regions = regions_two_same_line();
        let distance = project_onto_layer(&regions, 85.0, 10.0)
            .expect("x=85 必须命中 region B");
        assert!(
            (distance - 15.0).abs() < 1e-9,
            "必须落 region B 中点（distance_start=10 + 5），实际 {}",
            distance
        );
        let distance_a = project_onto_layer(&regions, 15.0, 10.0)
            .expect("x=15 必须命中 region A");
        assert!(
            (distance_a - 5.0).abs() < 1e-9,
            "必须落 region A 中点，实际 {}",
            distance_a
        );
    }

    #[test]
    fn coordinated_projection_clamps_beyond_segment_ends() {
        let regions = regions_two_same_line();
        // x 超出所有同行段：按最近段钳制到端点，不返回 None。
        let distance = project_onto_layer(&regions, 200.0, 10.0)
            .expect("超出行尾必须钳制到最近段端点");
        assert!(
            (distance - 20.0).abs() < 1e-9,
            "必须钳制到 region B 末端（10 + 10），实际 {}",
            distance
        );
    }
}
