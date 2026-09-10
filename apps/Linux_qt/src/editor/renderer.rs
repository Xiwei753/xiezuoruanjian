use cpp::cpp;

cpp! {{
    #include <QtGui/QPainter>
    #include <QtGui/QFont>
}}

/// 获取 QQuickItem 所在窗口的设备像素比（DPR）。必须在 GUI 线程调用。
pub fn sujian_item_dpr(item_ptr: *mut std::ffi::c_void) -> f64 {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [item_ptr as "QQuickItem*"] -> f64 as "double" {
        if (!item_ptr || !item_ptr->window()) return 1.0;
        return item_ptr->window()->devicePixelRatio();
    })
}
