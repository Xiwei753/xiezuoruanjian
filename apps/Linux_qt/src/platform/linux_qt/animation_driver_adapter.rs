//! Linux Qt AnimationDriver 实现
//!
//! 坐标填充、帧驱动、Scene Graph 绘制、暂停策略收敛到此。
//! SujianEditorItem 不直接管理动画状态，只通过此适配器。
//!
//! 安全约束：
//! - 本适配器持有 QQuickItem* 裸指针，仅限 GUI 线程使用。
//! - 使用 Rc<Cell<>> 而非 Mutex，因为 GUI 单线程不需要跨线程同步；
//!   Rc 不是 Send/Sync，编译器会阻止跨线程传播。

use cpp::cpp;
use std::cell::Cell;
use std::rc::Rc;
use writer_core::platform_interaction::animation_driver::{
    AnimationDriveRequest, AnimationDriver, AnimationSuppressReason,
};

cpp! {{
    #include <QtQuick/QQuickItem>
}}

pub struct LinuxQtAnimationDriver {
    suppressed: bool,
    suppress_reason: Option<AnimationSuppressReason>,
    item_ptr: Rc<Cell<*mut std::ffi::c_void>>,
}

impl LinuxQtAnimationDriver {
    pub fn new() -> Self {
        Self {
            suppressed: false,
            suppress_reason: None,
            item_ptr: Rc::new(Cell::new(std::ptr::null_mut())),
        }
    }

    pub fn set_item_ptr(&self, ptr: *mut std::ffi::c_void) {
        self.item_ptr.set(ptr);
    }
}

impl Default for LinuxQtAnimationDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationDriver for LinuxQtAnimationDriver {
    fn drive_animation(&mut self, request: AnimationDriveRequest) {
        if self.suppressed {
            return;
        }
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            let _ = request;
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                item_ptr->update();
            });
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
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                QMetaObject::invokeMethod(item_ptr, "explicit_clear_requested");
            });
        }
    }

    fn finish_all_animations(&mut self) {
        let item_ptr = self.item_ptr.get();
        if !item_ptr.is_null() {
            cpp!(unsafe [item_ptr as "QQuickItem*"] {
                QMetaObject::invokeMethod(item_ptr, "explicit_clear_requested");
            });
        }
    }
}
