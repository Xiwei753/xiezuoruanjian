//! Linux Qt AnimationDriver 实现
//!
//! 坐标填充、帧驱动、QML overlay 绘制、暂停策略收敛到此。
//! SujianEditorItem 不直接管理动画状态，只通过此适配器。

use writer_core::platform_interaction::animation_driver::{
    AnimationDriveRequest, AnimationDriver, AnimationSuppressReason,
};

/// Linux Qt AnimationDriver 实现
///
/// 坐标填充由 SujianEditorItem::fill_visual_transaction_coords() 完成，
/// 帧驱动由 QML AnimationTimer / requestAnimationFrame 完成，
/// 绘制由 QML EditorAnimationOverlay 完成。
pub struct LinuxQtAnimationDriver {
    suppressed: bool,
    suppress_reason: Option<AnimationSuppressReason>,
}

impl LinuxQtAnimationDriver {
    pub fn new() -> Self {
        Self {
            suppressed: false,
            suppress_reason: None,
        }
    }
}

impl Default for LinuxQtAnimationDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationDriver for LinuxQtAnimationDriver {
    fn drive_animation(&mut self, _request: AnimationDriveRequest) {
        // 实际的动画驱动委托给 SujianEditorItem 的 transaction 系统
        // 此处只提供接口边界
    }

    fn should_suppress_animation(&self) -> bool {
        self.suppressed
    }

    fn current_suppress_reason(&self) -> Option<AnimationSuppressReason> {
        self.suppress_reason
    }

    fn notify_animation_suppressed(&mut self, reason: AnimationSuppressReason) {
        self.suppressed = true;
        self.suppress_reason = Some(reason);
    }

    fn notify_animation_resumed(&mut self) {
        self.suppressed = false;
        self.suppress_reason = None;
    }

    fn cancel_all_animations(&mut self) {
        // 委托给 TextAnimationState::cancel_all()
    }

    fn finish_all_animations(&mut self) {
        // 委托给 TextAnimationState::finish_all()
    }
}
