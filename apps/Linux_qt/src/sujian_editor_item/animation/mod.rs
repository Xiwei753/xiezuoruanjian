//! Issue #904：正文视觉过渡由 `text_motion` 的单一局部动画计划完成。
//!
//! 本模块只保留动画模式解析。旧的 `VisualEditState` / `LinuxEditorAnimationCoordinator` /
//! `VisualFrame` / `RenderOwnershipPlan` / `EditVisualTimeline` / `render_plan_builder`，以及
//! Issue #901 的 `PresentedScene` / `CanvasController` / `CanvasDrawPlan` / `RevisionChain` /
//! `SceneTransition` 状态机都已删除。

/// Issue #904 复核三：三项用户偏好在当前设置下解析出的运行策略。
///
/// 每次按当前设置现算，不另存状态机，也不改写用户已保存的独立开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EffectiveAnimation {
    /// 是否播放正文吞吐动画（吐字 / 吞字 / 位移）。
    pub text: bool,
    /// 是否播放光标平滑动画（普通点击 / 键盘导航）。
    pub cursor: bool,
    /// 正文与光标是否共用同一条局部时间线（协同模式）。
    pub shared_timeline: bool,
}

/// Issue #904 复核三：把三项用户偏好解析成一份确定性的运行策略。
///
/// - 协同模式是一个**完整的**模式：一套局部文字运动 + 现有 `CursorController`
///   共用 `text_motion` 进度，与独立开关此前开没开无关；但**不修改**用户保存的
///   `typing` / `smooth_cursor` 偏好，关闭协同后原样恢复。
/// - 非协同：两个独立开关互不影响，不共享时间线。
pub(crate) const fn resolve(
    typing: bool,
    smooth_cursor: bool,
    coordinated: bool,
) -> EffectiveAnimation {
    if coordinated {
        EffectiveAnimation {
            text: true,
            cursor: true,
            shared_timeline: true,
        }
    } else {
        EffectiveAnimation {
            text: typing,
            cursor: smooth_cursor,
            shared_timeline: false,
        }
    }
}
