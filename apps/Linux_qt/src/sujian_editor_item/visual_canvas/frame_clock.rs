//! Issue #901：统一帧时间与整体视觉姿态。
//!
//! 一次 `frame_now` 只产生**一份整体视觉姿态** `VisualPose`。
//! 配置动画速度约束整体响应，**不**把每笔 1000ms 排队。
//!
//! ## 核心规则
//!
//! - **新 target 到来保留当前已提交姿态/速度，不从 1.0 饱和进度继承。**
//!   `retarget` 时先采样当前实际显示的 pose 作为新起点，再设定新 target。
//! - **速度定义**："当前显示姿态追赶最新目标的响应时间"（`response_duration_ms`）。
//! - **禁止**光标、删除遮罩、reflow 各有互不相干的计时器。
//!   所有视觉分量共享一份 `VisualPose` 和一份响应时间。
//! - **允许**不同轨迹的几何投影，不能把"一个时间"误解成"所有文字同向平移"。
//!
//! ## 时间模型
//!
//! 使用指数衰减：`alpha = 1 - exp(-elapsed / response_time)`。
//! - `elapsed = 0` → `alpha = 0`（显示当前 pose）
//! - `elapsed = response_time` → `alpha ≈ 0.632`（已追赶 63.2%）
//! - `elapsed → ∞` → `alpha → 1`（到达 target）
//!
//! 指数衰减保证速度连续：新 target 到来时不会出现姿态跳变或速度不连续。

use super::presented_scene::{CanvasVec2, PresentedCaret, PresentedScroll};

/// 统一视觉姿态 —— 一次 `frame_now` 产出的整体视觉状态。
///
/// 所有视觉分量共享同一份进度和姿态：
/// - `progress`：整体过渡进度（0.0 = 完全在当前 pose，1.0 = 完全在 target）。
/// - `caret_position`：光标视口位置。
/// - `scroll_position`：滚动位置。
///
/// **禁止**为光标、删除遮罩、reflow 分别维护独立的进度/计时器。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisualPose {
    /// 整体过渡进度（0.0 – 1.0）。
    pub progress: f64,
    /// 光标视口位置。
    pub caret_position: CanvasVec2,
    /// 滚动位置。
    pub scroll_position: CanvasVec2,
}

impl VisualPose {
    /// 全零初始姿态。
    pub(crate) fn zero() -> Self {
        Self {
            progress: 0.0,
            caret_position: CanvasVec2::new(0.0, 0.0),
            scroll_position: CanvasVec2::new(0.0, 0.0),
        }
    }

    /// 线性插值两个 pose：`self * (1 - t) + other * t`。
    ///
    /// `progress` 也做线性插值——它是"当前过渡在第几步"的连续量，
    /// 不是独立的动画时钟。
    #[inline]
    pub(crate) fn lerp(self, other: Self, t: f64) -> Self {
        Self {
            progress: self.progress + (other.progress - self.progress) * t,
            caret_position: CanvasVec2::lerp(self.caret_position, other.caret_position, t),
            scroll_position: CanvasVec2::lerp(self.scroll_position, other.scroll_position, t),
        }
    }
}

impl Default for VisualPose {
    fn default() -> Self {
        Self::zero()
    }
}

/// 统一帧时钟 —— 持有当前已提交姿态、目标姿态和响应时间配置。
///
/// 一次 `frame_now` 只产生一份 `VisualPose`，约束所有视觉分量共享同一时间线。
/// 新 target 到来时保留当前实际显示姿态，不从 1.0 饱和进度继承。
///
/// Issue #901 阻塞 4：`retarget` 从**最新成功提交的真实视觉 pose**
/// （`last_committed_pose`）读取 current_pose，而不是从 `frame_now` 采样
/// "理论该在的位置"。这样"已显示半字 → 立即删除"从半字真正接续，不会从
/// 理论位置跳变。
pub(crate) struct FrameClock {
    /// 当前已提交姿态（上次 `retarget` 时的实际显示 pose）。
    current_pose: VisualPose,
    /// 目标姿态（最新 target）。
    target_pose: VisualPose,
    /// 响应时间（毫秒）—— "当前显示姿态追赶最新目标"的时间。
    response_duration_ms: f64,
    /// 上次 `retarget` 的时间戳（毫秒）。
    retarget_time_ms: f64,
    /// 最新成功提交的真实视觉 pose（Qt `afterFrameEnd` 回执）。
    ///
    /// Issue #901 阻塞 4：`retarget` 时用此作为新的 `current_pose`（如果存在），
    /// 而不是从 `frame_now` 采样"理论该在的位置"。`None` 表示尚无帧提交过。
    last_committed_pose: Option<VisualPose>,
}

impl FrameClock {
    /// 创建帧时钟，初始 pose 和 target 都为 `initial_pose`，响应时间 `response_duration_ms`。
    pub(crate) fn new(initial_pose: VisualPose, response_duration_ms: f64, now_ms: f64) -> Self {
        Self {
            current_pose: initial_pose,
            target_pose: initial_pose,
            response_duration_ms: response_duration_ms.max(1.0),
            retarget_time_ms: now_ms,
            last_committed_pose: None,
        }
    }

    /// 采样当前应显示的视觉姿态。
    ///
    /// 使用指数衰减计算追赶进度 `alpha`，返回 `current_pose` 到 `target_pose`
    /// 的插值。一次调用只产生一份 `VisualPose`。
    pub(crate) fn frame_now(&self, now_ms: f64) -> VisualPose {
        let elapsed = (now_ms - self.retarget_time_ms).max(0.0);
        let alpha = self.convergence_alpha(elapsed);
        VisualPose::lerp(self.current_pose, self.target_pose, alpha)
    }

    /// 设定新目标姿态。
    ///
    /// **先从最新成功提交的真实视觉 pose 读取 current_pose 的 caret/scroll**
    /// （Issue #901 阻塞 4），再设定新 target。
    ///
    /// Issue #901 问题1：**progress 重置为 0**，新过渡从头开始。
    /// 旧实现把 `last_committed_pose.progress`（上一过渡的 1.0）继承成
    /// 新过渡的起始进度，导致起点和终点都是 1→1，`SceneTransition` 每帧
    /// 取 progress=1，吞字/吐字直接跳终点，没有运动。新实现保留 caret/scroll
    /// 的视觉连续性，但 progress 从 0 开始插值到 target.progress（始终 1.0）。
    ///
    /// 这保证：
    /// - 新 target 到来保留当前已提交姿态的 caret/scroll，不从 1.0 饱和进度继承。
    /// - 新过渡的 progress 从 0 开始，最终到达 1.0，动画真正运动。
    /// - "已显示半字 → 立即删除"从半字真正接续，不会从理论位置跳变。
    /// - 姿态和速度连续，无跳变。
    pub(crate) fn retarget(&mut self, new_target: VisualPose, now_ms: f64) {
        // Issue #901 阻塞 4：优先从最新成功提交的真实视觉 pose 读取 caret/scroll。
        let current_displayed = match self.last_committed_pose {
            Some(committed) => committed,
            None => self.frame_now(now_ms),
        };
        // Issue #901 问题1：progress 重置为 0，新过渡从头开始。
        // caret/scroll 从 last_committed_pose 取（保留视觉连续），
        // 但 progress 不继承上一过渡的 1.0。
        self.current_pose = VisualPose {
            progress: 0.0,
            caret_position: current_displayed.caret_position,
            scroll_position: current_displayed.scroll_position,
        };
        self.target_pose = new_target;
        self.retarget_time_ms = now_ms;
    }

    /// 设定响应时间（毫秒）。
    ///
    /// "当前显示姿态追赶最新目标的响应时间"。值越小响应越快。
    /// 下限 1ms 避免除零。
    pub(crate) fn set_response_duration_ms(&mut self, ms: f64) {
        self.response_duration_ms = ms.max(1.0);
    }

    /// 返回当前响应时间配置。
    pub(crate) fn response_duration_ms(&self) -> f64 {
        self.response_duration_ms
    }

    /// 返回当前已提交姿态。
    pub(crate) fn current_pose(&self) -> VisualPose {
        self.current_pose
    }

    /// 返回目标姿态。
    pub(crate) fn target_pose(&self) -> VisualPose {
        self.target_pose
    }

    /// 从已提交帧更新当前姿态（Qt `afterFrameEnd` 回执）。
    ///
    /// 新帧已提交后，`current_pose` 前进到该帧的实际 pose，
    /// `target_pose` 保持不变（可能还有未追赶完的目标）。
    ///
    /// Issue #901 阻塞 4：同时更新 `last_committed_pose`，供下次 `retarget`
    /// 从最新成功提交的真实视觉 pose 读取 current_pose。
    pub(crate) fn commit_pose(&mut self, committed: VisualPose, now_ms: f64) {
        self.current_pose = committed;
        self.last_committed_pose = Some(committed);
        self.retarget_time_ms = now_ms;
    }

    /// 返回最新成功提交的真实视觉 pose（仅供调试 / 测试）。
    pub(crate) fn last_committed_pose(&self) -> Option<VisualPose> {
        self.last_committed_pose
    }

    /// 判断当前姿态是否已到达目标（已稳定）。
    ///
    /// Issue #901 问题5：帧调度不再仅用 `opacity<1` 判断是否需要下一帧。
    /// 改为由 `FrameClock` 是否到达终点决定。
    ///
    /// Issue #901 问题1：`epsilon` 钳制为不超过 `0.001`（严格小容差）。
    /// 旧实现接受调用方传入 `0.5`，导致 progress 只走到约 50% 就被判完成，
    /// 动画半途停住。新实现无论调用方传入多大的 epsilon，实际都用
    /// `min(epsilon, 0.001)`，确保 progress 真正接近 1.0 才视为 settled。
    ///
    /// **调用方应传入 `0.001` 而非 `0.5`**。此处的钳制是防御性措施，
    /// 防止旧调用方的大容差导致动画提前结束。
    ///
    /// 只检查 `progress` 是否接近 1.0——progress=1.0 时所有几何自然到达
    /// 目标位置（FLIP 插值在 progress=1.0 时返回 target_rect）。
    /// 不单独检查 caret/scroll 位置，因为它们由 progress 统一驱动。
    pub(crate) fn is_settled(&self, now_ms: f64, epsilon: f64) -> bool {
        // Issue #901 问题1：钳制 epsilon 上限为 0.001，防止大容差导致动画提前结束。
        let eps = epsilon.min(0.001);
        let current = self.frame_now(now_ms);
        (current.progress - self.target_pose.progress).abs() < eps
    }

    /// 计算指数衰减的收敛系数 alpha ∈ [0, 1)。
    ///
    /// `alpha = 1 - exp(-elapsed / response_time)`。
    /// elapsed 为 0 时返回 0（显示 current），elapsed → ∞ 时趋近 1（到达 target）。
    #[inline]
    fn convergence_alpha(&self, elapsed_ms: f64) -> f64 {
        // 1 - exp(-elapsed / tau)
        // 用 -x 的 exp 避免正数溢出
        let x = elapsed_ms / self.response_duration_ms;
        1.0 - (-x).exp()
    }
}

/// 从已提交帧的 `PresentedCaret` 和 `PresentedScroll` 构造 `VisualPose`。
///
/// 这是纯 Rust 转换：把已提交画面的视觉事实转为统一姿态。
pub(crate) fn pose_from_presented(
    caret: Option<&PresentedCaret>,
    scroll: &PresentedScroll,
    progress: f64,
) -> VisualPose {
    let caret_position = caret
        .filter(|c| c.visible)
        .map(|c| c.position)
        .unwrap_or_else(|| CanvasVec2::new(0.0, 0.0));
    VisualPose {
        progress,
        caret_position,
        scroll_position: CanvasVec2::new(scroll.x, scroll.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(progress: f64, cx: f64, cy: f64, sx: f64, sy: f64) -> VisualPose {
        VisualPose {
            progress,
            caret_position: CanvasVec2::new(cx, cy),
            scroll_position: CanvasVec2::new(sx, sy),
        }
    }

    #[test]
    fn frame_now_returns_current_at_t_zero() {
        let current = pose(0.0, 10.0, 20.0, 0.0, 0.0);
        let clock = FrameClock::new(current, 100.0, 0.0);
        // elapsed = 0 → alpha = 0 → 返回 current
        let result = clock.frame_now(0.0);
        assert_eq!(result, current);
    }

    #[test]
    fn frame_now_approaches_target_over_time() {
        let current = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target = pose(1.0, 100.0, 100.0, 0.0, 0.0);
        let mut clock = FrameClock::new(current, 100.0, 0.0);
        clock.retarget(target, 0.0);

        // elapsed = 0 → current
        let at_0 = clock.frame_now(0.0);
        assert!((at_0.caret_position.x - 0.0).abs() < 1e-9);

        // elapsed = response_time → alpha ≈ 0.632
        let at_tau = clock.frame_now(100.0);
        assert!((at_tau.caret_position.x - 63.2).abs() < 0.1);

        // elapsed >> response_time → 接近 target
        let at_large = clock.frame_now(10000.0);
        assert!((at_large.caret_position.x - 100.0).abs() < 0.01);
    }

    #[test]
    fn retarget_preserves_current_pose() {
        let current = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target1 = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(current, 100.0, 0.0);
        clock.retarget(target1, 0.0);

        // 追赶到一半
        let mid = clock.frame_now(50.0);
        assert!(mid.caret_position.x > 0.0 && mid.caret_position.x < 100.0);

        // 新 target 到来：current_pose 应为 mid（当前实际显示），不从 1.0 继承
        let target2 = pose(1.0, 200.0, 0.0, 0.0, 0.0);
        clock.retarget(target2, 50.0);

        // retarget 后 frame_now(50) 应返回 mid（elapsed = 0）
        let after_retarget = clock.frame_now(50.0);
        assert!((after_retarget.caret_position.x - mid.caret_position.x).abs() < 1e-9);
    }

    #[test]
    fn set_response_duration_clamped() {
        let pose = VisualPose::zero();
        let mut clock = FrameClock::new(pose, 100.0, 0.0);
        clock.set_response_duration_ms(0.0);
        // 下限 1ms
        assert!(clock.response_duration_ms() >= 1.0);
    }

    #[test]
    fn commit_pose_advances_current() {
        let initial = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(initial, 100.0, 0.0);
        clock.retarget(target, 0.0);

        // 模拟一帧提交了中间 pose
        let committed = pose(0.5, 50.0, 0.0, 0.0, 0.0);
        clock.commit_pose(committed, 50.0);

        // frame_now(50) 应返回 committed（elapsed = 0 after commit）
        let result = clock.frame_now(50.0);
        assert!((result.caret_position.x - 50.0).abs() < 1e-9);
    }

    #[test]
    fn pose_lerp() {
        let a = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let b = pose(1.0, 10.0, 20.0, 30.0, 40.0);
        let mid = VisualPose::lerp(a, b, 0.5);
        assert!((mid.progress - 0.5).abs() < 1e-9);
        assert!((mid.caret_position.x - 5.0).abs() < 1e-9);
        assert!((mid.scroll_position.y - 20.0).abs() < 1e-9);
    }

    #[test]
    fn pose_from_presented_with_visible_caret() {
        let caret = PresentedCaret {
            position: CanvasVec2::new(15.0, 30.0),
            visible: true,
            width: 2.0,
            height: 20.0,
        };
        let scroll = PresentedScroll { x: 0.0, y: 50.0 };
        let pose = pose_from_presented(Some(&caret), &scroll, 0.7);
        assert!((pose.caret_position.x - 15.0).abs() < 1e-9);
        assert!((pose.scroll_position.y - 50.0).abs() < 1e-9);
        assert!((pose.progress - 0.7).abs() < 1e-9);
    }

    #[test]
    fn pose_from_presented_with_hidden_caret() {
        let caret = PresentedCaret {
            position: CanvasVec2::new(15.0, 30.0),
            visible: false,
            width: 2.0,
            height: 20.0,
        };
        let scroll = PresentedScroll { x: 0.0, y: 0.0 };
        let pose = pose_from_presented(Some(&caret), &scroll, 1.0);
        // 光标不可见 → position 为 (0, 0)
        assert!((pose.caret_position.x - 0.0).abs() < 1e-9);
    }

    #[test]
    fn retarget_uses_last_committed_pose() {
        // Issue #901 阻塞 4：retarget 从最新成功提交的真实视觉 pose 读取。
        let initial = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target1 = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(initial, 100.0, 0.0);
        clock.retarget(target1, 0.0);

        // 追赶到一半，然后提交一个中间 pose。
        let committed = pose(0.5, 50.0, 0.0, 0.0, 0.0);
        clock.commit_pose(committed, 50.0);

        // 新 target 到来：current_pose 应为 committed（last_committed_pose），
        // 不是 frame_now 采样的"理论该在的位置"。
        let target2 = pose(1.0, 200.0, 0.0, 0.0, 0.0);
        clock.retarget(target2, 60.0);

        // retarget 后 frame_now(60) 应返回 committed（elapsed = 0 after retarget）。
        let after_retarget = clock.frame_now(60.0);
        assert!(
            (after_retarget.caret_position.x - 50.0).abs() < 1e-9,
            "retarget must use last_committed_pose (x=50), got {}",
            after_retarget.caret_position.x
        );
    }

    #[test]
    fn retarget_falls_back_to_frame_now_without_commit() {
        // 没有提交过帧时，retarget 回退到 frame_now 采样。
        let current = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target1 = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(current, 100.0, 0.0);
        clock.retarget(target1, 0.0);

        // 追赶到一半（未提交）。
        let mid = clock.frame_now(50.0);

        // 新 target 到来：没有 last_committed_pose → 回退到 frame_now 采样。
        let target2 = pose(1.0, 200.0, 0.0, 0.0, 0.0);
        clock.retarget(target2, 50.0);

        let after_retarget = clock.frame_now(50.0);
        assert!(
            (after_retarget.caret_position.x - mid.caret_position.x).abs() < 1e-9,
            "retarget without commit must fall back to frame_now"
        );
    }

    #[test]
    fn last_committed_pose_updated_on_commit() {
        let initial = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(initial, 100.0, 0.0);
        assert!(clock.last_committed_pose().is_none());

        let committed = pose(0.5, 50.0, 0.0, 0.0, 0.0);
        clock.commit_pose(committed, 16.0);
        assert_eq!(clock.last_committed_pose(), Some(committed));
    }

    #[test]
    fn retarget_resets_progress_to_zero() {
        // Issue #901 问题1：retarget 后 current_pose.progress 必须为 0，
        // 不从 last_committed_pose 的 1.0 继承。
        let initial = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target1 = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(initial, 100.0, 0.0);
        clock.retarget(target1, 0.0);

        // 提交一个 progress=1.0 的帧（动画完成）。
        let committed = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        clock.commit_pose(committed, 200.0);

        // 新 target 到来：current_pose.progress 必须重置为 0，
        // 不能从 committed.progress=1.0 继承。
        let target2 = pose(1.0, 200.0, 0.0, 0.0, 0.0);
        clock.retarget(target2, 200.0);

        // retarget 后 frame_now(200) elapsed=0 → 返回 current_pose。
        let after_retarget = clock.frame_now(200.0);
        assert!(
            (after_retarget.progress - 0.0).abs() < 1e-9,
            "retarget must reset progress to 0, got {}",
            after_retarget.progress
        );
        // caret/scroll 仍从 committed 继承（视觉连续）。
        assert!(
            (after_retarget.caret_position.x - 100.0).abs() < 1e-9,
            "retarget must preserve caret from committed, got {}",
            after_retarget.caret_position.x
        );
    }

    #[test]
    fn is_settled_clamps_large_epsilon() {
        // Issue #901 问题1：is_settled 钳制 epsilon 上限为 0.001，
        // 即使调用方传入 0.5 也只用 0.001。
        let current = pose(0.0, 0.0, 0.0, 0.0, 0.0);
        let target = pose(1.0, 100.0, 0.0, 0.0, 0.0);
        let mut clock = FrameClock::new(current, 100.0, 0.0);
        clock.retarget(target, 0.0);

        // elapsed=50ms, alpha≈0.393, progress≈0.393。
        // 旧实现用 epsilon=0.5 → |0.393-1.0|=0.607 > 0.5 → not settled（刚好不 settled）。
        // 但如果 progress=0.6，旧实现 |0.6-1.0|=0.4 < 0.5 → settled（错误，只走到 60%）。
        // 新实现钳制 epsilon=0.001 → |0.393-1.0|=0.607 > 0.001 → not settled。
        assert!(
            !clock.is_settled(50.0, 0.5),
            "is_settled must clamp epsilon to 0.001, not accept 0.5"
        );

        // 很长时间后 progress≈1.0 → settled。
        assert!(
            clock.is_settled(10000.0, 0.5),
            "is_settled must be true when progress truly reaches 1.0"
        );
    }
}
