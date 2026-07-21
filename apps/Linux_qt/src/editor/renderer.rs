use cpp::cpp;
use qmetaobject::{QColor, QPainter, QPointF, QRectF, QString};
use std::time::{Duration, Instant};

cpp! {{
    #include <QtGui/QPainter>
    #include <QtGui/QFont>
}}

/// 慢绘制日志节流——距上次日志 ≥500ms 时才允许再次记录。
pub fn should_log_slow_paint(last_log: Option<Instant>, now: Instant) -> bool {
    last_log.is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
}

/// 绘制文本——`baseline_y` 为 Qt 文本基线 y 坐标（非顶部），坐标系为逻辑像素。
pub fn draw_text(
    painter: &mut QPainter,
    x: f64,
    baseline_y: f64,
    font_size: f32,
    color: QString,
    text: QString,
) {
    let _ = font_size;
    painter.set_pen(qmetaobject::QPen::from_color(color_from_qstring(color)));
    painter.draw_text(QPointF { x, y: baseline_y }, text);
}

/// 绘制填充矩形——坐标为逻辑像素。
pub fn draw_rect(painter: &mut QPainter, x: f64, y: f64, width: f64, height: f64, color: QString) {
    painter.fill_rect(
        QRectF {
            x,
            y,
            width,
            height,
        },
        qmetaobject::QBrush::from_color(color_from_qstring(color)),
    );
}

/// 从 QString 颜色名（如 "#FF0000" 或 "red"）创建 QColor。
pub fn color_from_qstring(color: QString) -> QColor {
    QColor::from_name(&color.to_string())
}

/// 获取 QQuickItem 所在窗口的设备像素比（DPR）。必须在 GUI 线程调用。
pub fn sujian_item_dpr(item_ptr: *mut std::ffi::c_void) -> f64 {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [item_ptr as "QQuickItem*"] -> f64 as "double" {
        if (!item_ptr || !item_ptr->window()) return 1.0;
        return item_ptr->window()->devicePixelRatio();
    })
}

/// 在 QImage 上创建缩放 QPainter——调用方负责通过 `sujian_delete_painter` 释放。
/// 返回的 QPainter 所有权转移给调用方；必须在 GUI 线程使用和删除。
pub fn sujian_create_painter_scaled(image: &mut qmetaobject::QImage, dpr: f64) -> *mut QPainter {
    let img_ptr = image as *mut qmetaobject::QImage;
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [img_ptr as "QImage*", dpr as "double"] -> *mut QPainter as "QPainter*" {
        auto *p = new QPainter(img_ptr);
        p->scale(dpr, dpr);
        return p;
    })
}

/// 释放由 `sujian_create_painter_scaled` 创建的 QPainter。必须在 GUI 线程调用。
/// 传入的指针必须来自 `sujian_create_painter_scaled`，且只能释放一次。
pub fn sujian_delete_painter(painter: *mut QPainter) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [painter as "QPainter*"] { delete painter; })
}
