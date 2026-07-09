//! Linux Qt AnimationDriver 实现
//!
//! 坐标填充、帧驱动、QML overlay 绘制、暂停策略收敛到此。
//! SujianEditorItem 不直接管理动画状态，只通过此适配器。
//!
//! 已完成迁移：
//! - should_suppress_animation / notify_animation_suppressed / resumed 已真实接入
//! - drive_animation() 通过 item_ptr 触发 QML visual_transaction_changed signal
//! - cancel_all_animations / finish_all_animations 通过 item_ptr 触发 explicit_clear_requested
//!
//! 待完成迁移：
//! - QML AnimationTimer / requestAnimationFrame 仍为独立路径，未完全收敛到 drive_animation

use cpp::cpp;
use std::sync::Mutex;
use writer_core::platform_interaction::animation_driver::{
    AnimationDriveRequest, AnimationDriver, AnimationSuppressReason,
};

cpp! {{
    #include <QtQuick/QQuickItem>
}}

pub struct LinuxQtAnimationDriver {
    suppressed: bool,
    suppress_reason: Option<AnimationSuppressReason>,
    item_ptr: Mutex<*mut std::ffi::c_void>,
}

impl LinuxQtAnimationDriver {
    pub fn new() -> Self {
        Self {
            suppressed: false,
            suppress_reason: None,
            item_ptr: Mutex::new(std::ptr::null_mut()),
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        if let Ok(mut guard) = self.item_ptr.lock() {
            *guard = ptr;
        }
    }
}

impl Default for LinuxQtAnimationDriver {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for LinuxQtAnimationDriver {}
unsafe impl Sync for LinuxQtAnimationDriver {}

impl AnimationDriver for LinuxQtAnimationDriver {
    fn drive_animation(&mut self, request: AnimationDriveRequest) {
        if self.suppressed {
            return;
        }
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                let _ = request;
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QMetaObject::invokeMethod(item_ptr, "visual_transaction_changed");
                });
            }
        }
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
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QMetaObject::invokeMethod(item_ptr, "explicit_clear_requested");
                });
            }
        }
    }

    fn finish_all_animations(&mut self) {
        if let Ok(guard) = self.item_ptr.lock() {
            let item_ptr = *guard;
            if !item_ptr.is_null() {
                cpp!(unsafe [item_ptr as "QQuickItem*"] {
                    QMetaObject::invokeMethod(item_ptr, "explicit_clear_requested");
                });
            }
        }
    }
}
