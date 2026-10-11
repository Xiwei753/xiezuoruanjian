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
pub(crate) struct FrameClock {
    /// 当前已提交姿态（上次 `retarget` 时的实际显示 pose）。
    current_pose: VisualPose,
    /// 目标姿态（最新 target）。
    target_pose: VisualPose,
    /// 响应时间（毫秒）—— "当前显示姿态追赶最新目标"的时间。
    response_duration_ms: f64,
    /// 上次 `retarget` 的时间戳（毫秒）。
    retarget_time_ms: f64,
}

impl FrameClock {
    /// 创建帧时钟，初始 pose 和 target 都为 `initial_pose`，响应时间 `response_duration_ms`。
    pub(crate) fn new(initial_pose: VisualPose, response_duration_ms: f64, now_ms: f64) -> Self {
        Self {
            current_pose: initial_pose,
            target_pose: initial_pose,
            response_duration_ms: response_duration_ms.max(1.0),
            retarget_time_ms: now_ms,
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
    /// **先采样当前实际显示的 pose 作为新 `current_pose`**，再设定新 target。
    /// 这保证：
    /// - 新 target 到来保留当前已提交姿态/速度，不从 1.0 饱和进度继承。
    /// - 姿态和速度连续，无跳变。
    pub(crate) fn retarget(&mut self, new_target: VisualPose, now_ms: f64) {
        let current_displayed = self.frame_now(now_ms);
        self.current_pose = current_displayed;
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
    pub(crate) fn commit_pose(&mut self, committed: VisualPose, now_ms: f64) {
        self.current_pose = committed;
        self.retarget_time_ms = now_ms;
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
}
