//! Linux Qt AnimationDriver 实现
//!
//! 坐标填充、帧驱动、QML overlay 绘制、暂停策略收敛到此。
//! SujianEditorItem 不直接管理动画状态，只通过此适配器。
//!
//! TODO(平台交互收口): drive_animation/cancel_all_animations/finish_all_animations
//! 当前为空桩。实际动画驱动由 QML AnimationTimer + EditorAnimationOverlay 完成。
//! 迁移计划：SujianEditorItem 在收到 Core visual transaction 后通过此适配器驱动动画，
//! 而非直接发射 QML signal 让 QML timer 驱动。

use writer_core::platform_interaction::animation_driver::{
    AnimationDriveRequest, AnimationDriver, AnimationSuppressReason,
};

/// Linux Qt AnimationDriver 实现
///
/// 抑制/恢复状态管理已真实接入（should_suppress_animation / notify_animation_suppressed / resumed）。
/// 帧驱动和动画控制仍为空桩（实际由 QML AnimationTimer / requestAnimationFrame 驱动）。
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
        // TODO(平台交互收口): 接入 SujianEditorItem transaction 系统，
        // 替代直接发射 visual_transaction_json_changed signal
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
        // TODO(平台交互收口): 接入 TextAnimationState，触发 QML overlay clear
    }

    fn finish_all_animations(&mut self) {
        // TODO(平台交互收口): 接入 TextAnimationState，触发 QML overlay finish
    }
}
