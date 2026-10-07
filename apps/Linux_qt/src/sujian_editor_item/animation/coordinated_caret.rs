//! Issue #826 评论 38/39/40/41：协同模式下光标与文字前沿共享的单条 caret 运动轨迹。
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
//! - 轨迹由两类段组成（评论 41）：
//!   - [`CaretSegmentKind::Boundary`]：代表**文字吞吐边界**，几何取自 Frontier
//!     path，`y_from == y_to == 视觉行`，**绝不被 start/target 钉成斜线**；
//!   - [`CaretSegmentKind::Connector`]：纯光标连接段，承载「旧 caret 到第一个
//!     文字段入口」「文字段之间的换行」「最后一个文字段出口到 canonical caret」
//!     的二维位移。connector 期间文字前沿距离**冻结**（不假装它是文字边界）。
//! - 每帧由同一 progress 算出 travelled distance，再求 caret 位置与该段的
//!   文字前沿距离；光标画位置、遮罩/overlay 吃这个距离。真正"一条轨迹"。
//! - 连续输入只 retarget 这一份 motion，不排 per-key queue，活跃 motion 数量
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

use std::time::{Duration, Instant};

use super::edit_frontier::{ease_out_cubic, FrontierRegion};

/// Issue #826 评论 41：motion 段的语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretSegmentKind {
    /// 文字吞吐边界段：几何 = Frontier path 段，`y_from == y_to == 视觉行`，
    /// 代表「文字边界在哪一行」，绝不被钉成跨行斜线。
    Boundary,
    /// 纯光标连接段：承载换行 / 首尾对齐的二维位移；期间文字前沿距离冻结。
    Connector,
}

/// Issue #826 评论 39/40/41：caret 运动轨迹上的一段。
#[derive(Clone, Debug)]
pub(crate) struct CaretMotionSegment {
    pub kind: CaretSegmentKind,
    /// 进入这一段时的 x。
    pub x_from: f64,
    /// 进入这一段时的 y。
    pub y_from: f64,
    /// 走完这一段时的 x。
    pub x_to: f64,
    /// 走完这一段时的 y。
    pub y_to: f64,
    /// 该视觉行的高度（Boundary 用 Frontier 段高；Connector 取 `|dy|`）。
    pub h: f64,
    /// 这一段的长度（Boundary = `|dx|`；Connector = 欧氏距离）。
    pub visual_length: f64,
    /// Issue #826 评论 41：本段对应的**文字前沿距离**（源侧层坐标系）。
    ///
    /// - `Boundary`：进入本段前沿已走过的距离（段内再按本地 take 累加）；
    /// - `Connector`：connector 期间前沿距离**冻结**在 `Some(0)` 之后的累计值，
    ///   即「上一个文字段结束 / 下一个文字段开始」的距离；
    /// - `CaretOnly`（两侧都无 path）：`None`，此时没有文字边界要跟。
    pub frontier_distance_from: Option<f64>,
}

impl CaretMotionSegment {
    /// 沿本段走过 `take` 距离后的 (x, y)，以及本段前沿距离（若本段属于文字边界）。
    fn sample_after(&self, take: f64) -> (f64, f64, Option<f64>) {
        let take = take.clamp(0.0, self.visual_length);
        let frac = if self.visual_length <= f64::EPSILON {
            1.0
        } else {
            take / self.visual_length
        };
        let (x, y) = (
            self.x_from + (self.x_to - self.x_from) * frac,
            self.y_from + (self.y_to - self.y_from) * frac,
        );
        let frontier = self.frontier_distance_from.map(|base| match self.kind {
            CaretSegmentKind::Boundary => base + take,
            // connector 期间文字边界不推进：冻结在连接段起点。
            CaretSegmentKind::Connector => base,
        });
        (x, y, frontier)
    }
}

/// Issue #826 评论 41：motion 路径的来源侧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretPathSource {
    /// 路径取自吐字侧（Insert / 有插入的 Replace）。
    Reveal,
    /// 路径取自吞字侧（纯 Delete，Backward）。
    Conceal,
    /// 两侧都没有可见 path（Enter / shaping 全接管 / Forward Delete 原地 caret）：
    /// 只有 connector，文字侧只剩 Reflow/Shaping/Forward clock 独立跑。
    CaretOnly,
}

/// Issue #826 评论 38/39/40：协同模式下**唯一**的 caret 运动轨迹。
#[derive(Clone, Debug)]
pub(crate) struct CoordinatedCaretMotion {
    /// 按视觉顺序排列的分段轨迹（Boundary 之间用 Connector 连接）。
    pub segments: Vec<CaretMotionSegment>,
    /// 整条轨迹总长。
    pub total_length: f64,
    /// 路径来源侧（决定 frontier 距离用在 reveal 还是 conceal）。
    pub source: CaretPathSource,
    /// 与前沿统一的时间起点：协同创建/retarget 时直接取
    /// `active_edit_frontier.started_at`，不另取 `Instant::now()`。
    pub started_at: Instant,
    /// 与前沿统一的时长：永远是 typing duration，不是 smooth cursor duration。
    pub duration_ms: u64,
    /// 最新 canonical caret（文档坐标）：终点精确落点。
    pub target_x: f64,
    /// 最新 canonical caret（文档坐标 y）。
    pub target_y: f64,
}

impl CoordinatedCaretMotion {
    /// 按 `now` 采样 0..1 进度（与前沿 sample 同公式）。
    pub(crate) fn sample_progress(&self, now: Instant) -> f64 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started_at).as_secs_f64()
            / Duration::from_millis(self.duration_ms).as_secs_f64();
        elapsed.clamp(0.0, 1.0)
    }

    /// 本帧已走过的轨迹距离（起点恒为 0；retarget 靠重建路径 + 首段起点
    /// 取旧 motion 当前位置保证连续）。
    pub(crate) fn distance_at_progress(&self, progress: f64) -> f64 {
        self.total_length.max(0.0) * ease_out_cubic(progress)
    }

    /// 轨迹上走过 `distance` 后的 (x, y, 前沿距离)。
    ///
    /// 跨行时沿分段走：Boundary 段内沿文字行水平移动，Boundary 之间由
    /// Connector 承载换行；文字前沿距离在 Connector 期间不推进。
    pub(crate) fn sample_at_distance(&self, distance: f64) -> (f64, f64, Option<f64>) {
        let mut rest = distance.clamp(0.0, self.total_length.max(0.0));
        let mut last = (self.target_x, self.target_y, None);
        for segment in &self.segments {
            last = (
                segment.x_to,
                segment.y_to,
                segment.frontier_distance_from.map(|base| match segment.kind {
                    CaretSegmentKind::Boundary => base + segment.visual_length,
                    CaretSegmentKind::Connector => base,
                }),
            );
            if rest <= segment.visual_length + 1e-9 {
                return segment.sample_after(rest);
            }
            rest -= segment.visual_length;
        }
        last
    }

    /// 轨迹上走过 `distance` 后的 (x, y)（测试 / 位置用）。
    pub(crate) fn position_at_distance(&self, distance: f64) -> (f64, f64) {
        let (x, y, _) = self.sample_at_distance(distance);
        (x, y)
    }

    /// 滚动 pause / resume 用：与前沿三层一起平移起点。
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

/// Issue #826 评论 38/39/41：本帧协同 caret 投影到两侧前沿路径上的吞吐距离。
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
/// - x 超出段两端时钳制到端点。
/// - caret 的 y 落在任何段之外返回 None，调用方回退到 progress 时钟。
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
    use crate::sujian_editor_item::animation::edit_frontier::FrontierSegment;
    use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;

    fn boundary_seg(x_from: f64, x_to: f64, y: f64, frontier_from: f64) -> CaretMotionSegment {
        CaretMotionSegment {
            kind: CaretSegmentKind::Boundary,
            x_from,
            y_from: y,
            x_to,
            y_to: y,
            h: 20.0,
            visual_length: (x_to - x_from).abs(),
            frontier_distance_from: Some(frontier_from),
        }
    }

    fn motion() -> CoordinatedCaretMotion {
        CoordinatedCaretMotion {
            segments: vec![boundary_seg(10.0, 30.0, 20.0, 0.0)],
            total_length: 20.0,
            source: CaretPathSource::Reveal,
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
    }

    #[test]
    fn coordinated_motion_position_walks_path_not_lerp() {
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
    fn boundary_segment_reports_local_frontier_distance() {
        let m = motion();
        // Boundary 段：frontier 距离 = 段起点累计 + 本地 take。
        let (_, _, f0) = m.sample_at_distance(0.0);
        assert_eq!(f0, Some(0.0));
        let (_, _, f_half) = m.sample_at_distance(m.total_length * 0.5);
        assert!((f_half.unwrap() - m.total_length * 0.5).abs() < 1e-9);
    }

    #[test]
    fn connector_freezes_frontier_distance() {
        // connector 期间前沿距离冻结在连接段起点。
        let t0 = Instant::now();
        let m = CoordinatedCaretMotion {
            segments: vec![
                CaretMotionSegment {
                    kind: CaretSegmentKind::Connector,
                    x_from: 100.0,
                    y_from: 10.0,
                    x_to: 10.0,
                    y_to: 40.0,
                    h: 30.0,
                    visual_length: (90.0f64).hypot(30.0),
                    frontier_distance_from: Some(0.0),
                },
                boundary_seg(10.0, 30.0, 40.0, 0.0),
            ],
            total_length: (90.0f64).hypot(30.0) + 20.0,
            source: CaretPathSource::Reveal,
            started_at: t0,
            duration_ms: 160,
            target_x: 30.0,
            target_y: 40.0,
        };
        let connector_len = (90.0f64).hypot(30.0);
        // connector 中点：前沿距离冻结为 0。
        let (_, _, frozen) = m.sample_at_distance(connector_len * 0.5);
        assert_eq!(frozen, Some(0.0), "connector 期间前沿距离必须冻结");
        // 进入 boundary 后：前沿距离 > 0。
        let (_, _, after) = m.sample_at_distance(connector_len + 10.0);
        assert!(after.unwrap() > 0.0, "进入文字段后前沿距离必须推进");
    }

    #[test]
    fn coordinated_motion_retarget_rebuilds_path_from_current_position() {
        let m = motion();
        let t0 = m.started_at;
        let now = t0 + Duration::from_millis(40);
        let base = m.distance_at_progress(m.sample_progress(now));
        let (cur_x, cur_y) = m.position_at_distance(base);
        let retargeted = CoordinatedCaretMotion {
            segments: vec![
                CaretMotionSegment {
                    kind: CaretSegmentKind::Connector,
                    x_from: cur_x,
                    y_from: cur_y,
                    x_to: 50.0,
                    y_to: 20.0,
                    h: 0.0,
                    visual_length: (50.0 - cur_x).abs().max(1e-6),
                    frontier_distance_from: Some(0.0),
                },
            ],
            total_length: (50.0 - cur_x).abs().max(1e-6),
            source: CaretPathSource::Reveal,
            started_at: now,
            duration_ms: 160,
            target_x: 50.0,
            target_y: 20.0,
        };
        let d0 = retargeted.distance_at_progress(retargeted.sample_progress(now));
        let (rx0, ry0) = retargeted.position_at_distance(d0);
        assert!(
            (rx0 - cur_x).abs() < 1e-6 && (ry0 - cur_y).abs() < 1e-6,
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
        let regions = regions_two_same_line();
        let distance =
            project_onto_layer(&regions, 85.0, 10.0).expect("x=85 必须命中 region B");
        assert!((distance - 15.0).abs() < 1e-9, "实际 {}", distance);
        let distance_a =
            project_onto_layer(&regions, 15.0, 10.0).expect("x=15 必须命中 region A");
        assert!((distance_a - 5.0).abs() < 1e-9);
    }

    #[test]
    fn coordinated_projection_clamps_beyond_segment_ends() {
        let regions = regions_two_same_line();
        let distance =
            project_onto_layer(&regions, 200.0, 10.0).expect("超出行尾必须钳制到最近段端点");
        assert!((distance - 20.0).abs() < 1e-9, "实际 {}", distance);
    }
}
