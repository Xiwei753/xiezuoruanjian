use cpp::cpp;
use qmetaobject::{QColor, QPainter, QPointF, QRectF, QString};
use std::time::{Duration, Instant};

cpp! {{
    #include <QtGui/QPainter>
    #include <QtGui/QFont>
}}

pub fn should_log_slow_paint(last_log: Option<Instant>, now: Instant) -> bool {
    last_log.is_none_or(|last| now.duration_since(last) >= Duration::from_millis(500))
}

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

pub fn color_from_qstring(color: QString) -> QColor {
    QColor::from_name(&color.to_string())
}

pub fn sujian_item_dpr(item_ptr: *mut std::ffi::c_void) -> f64 {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [item_ptr as "QQuickItem*"] -> f64 as "double" {
        if (!item_ptr || !item_ptr->window()) return 1.0;
        return item_ptr->window()->devicePixelRatio();
    })
}

pub fn sujian_create_painter_scaled(image: &mut qmetaobject::QImage, dpr: f64) -> *mut QPainter {
    let img_ptr = image as *mut qmetaobject::QImage;
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [img_ptr as "QImage*", dpr as "double"] -> *mut QPainter as "QPainter*" {
        auto *p = new QPainter(img_ptr);
        p->scale(dpr, dpr);
        return p;
    })
}

pub fn sujian_delete_painter(painter: *mut QPainter) {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [painter as "QPainter*"] { delete painter; })
}
