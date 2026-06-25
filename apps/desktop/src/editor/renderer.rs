use cpp::cpp;
use qmetaobject::{QColor, QPainter, QPointF, QRectF, QString};
use std::time::{Duration, Instant};

use crate::editor::layout::EditorLayout;
use crate::sujian_editor_item::{
    editor_animation_debug_enabled, AnimatedGlyph,
};

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

pub fn draw_text_color(painter: &mut QPainter, x: f64, baseline_y: f64, color: QColor, text: QString) {
    painter.set_pen(qmetaobject::QPen::from_color(color));
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

pub fn color_hex_to_rgba(hex: &QString) -> u32 {
    let c = QColor::from_name(&hex.to_string());
    let r = c.red() as u32;
    let g = c.green() as u32;
    let b = c.blue() as u32;
    let a = c.alpha() as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

pub fn sujian_item_dpr(item_ptr: *mut std::ffi::c_void) -> f64 {
    cpp!(unsafe [item_ptr as "QQuickItem*"] -> f64 as "double" {
        if (!item_ptr || !item_ptr->window()) return 1.0;
        return item_ptr->window()->devicePixelRatio();
    })
}

/// Create a QPainter for the given QImage.
///
/// The image is at physical pixel size with DPR=1.0.
/// The painter manually scales by dpr so that paint_onto() works
/// entirely in logical coordinates. setSourceRect on QSGImageNode
/// must use physical pixel coords (multiply by dpr).
pub fn sujian_create_painter_scaled(image: &mut qmetaobject::QImage, dpr: f64) -> *mut QPainter {
    let img_ptr = image as *mut qmetaobject::QImage;
    cpp!(unsafe [img_ptr as "QImage*", dpr as "double"] -> *mut QPainter as "QPainter*" {
        auto *p = new QPainter(img_ptr);
        p->scale(dpr, dpr);
        return p;
    })
}

pub fn sujian_delete_painter(painter: *mut QPainter) {
    cpp!(unsafe [painter as "QPainter*"] { delete painter; })
}

pub fn glyph_rects_for_range(
    editor_layout: &EditorLayout,
    buffer_text: &str,
    byte_start: usize,
    byte_end: usize,
    font_pixel_size: f32,
    font_family: &str,
) -> Vec<AnimatedGlyph> {
    let Some(cache) = editor_layout.cache() else {
        return Vec::new();
    };
    let lines = &cache.lines;
    if lines.is_empty() || byte_start >= byte_end {
        return Vec::new();
    }
    let font_size = font_pixel_size as f64;
    let mut result = Vec::new();

    let search_start = lines.partition_point(|l| l.byte_end <= byte_start);
    let search_end = lines
        .len()
        .min(search_start + lines[search_start..].partition_point(|l| l.byte_start < byte_end) + 1);

    for line_idx in search_start..search_end {
        let line = &lines[line_idx];
        if line.byte_end <= byte_start || line.byte_start >= byte_end {
            continue;
        }
        let seg_start = byte_start.max(line.byte_start);
        let seg_end = byte_end.min(line.byte_end);
        if seg_start >= seg_end {
            continue;
        }

        let baseline_y = editor_layout.text_baseline_y(line, font_size, font_family);
        let (top_y, h) = editor_layout.cursor_rect_for_line(line, font_size, font_family);

        if line.para_text.is_empty() {
            continue;
        }
        let glyph_data = editor_layout.glyph_positions_on_line(
            line,
            seg_start,
            seg_end,
            font_size,
            font_family,
        );
        for (abs_byte, x_pos, ch_w) in glyph_data {
            if abs_byte >= buffer_text.len() {
                continue;
            }
            let ch = buffer_text.get(abs_byte..).and_then(|s| s.chars().next()).unwrap_or(' ');
            let ch_str = ch.to_string();
            result.push(AnimatedGlyph {
                byte_start: abs_byte,
                byte_end: abs_byte + ch_str.len(),
                text: ch_str,
                rect: (line.x + x_pos, top_y, ch_w, h),
                baseline_y,
                line_index: line_idx,
            });
        }
    }
    result
}

pub fn bounding_rect_for_range(
    editor_layout: &EditorLayout,
    buffer_text: &str,
    byte_start: usize,
    byte_end: usize,
    font_pixel_size: f32,
    font_family: &str,
    target_cursor_x: f64,
    target_cursor_y: f64,
) -> (f64, f64, f64, f64) {
    let glyphs = glyph_rects_for_range(
        editor_layout,
        buffer_text,
        byte_start,
        byte_end,
        font_pixel_size,
        font_family,
    );
    if glyphs.is_empty() {
        return (
            target_cursor_x,
            target_cursor_y,
            2.0,
            editor_layout.cursor_height(font_pixel_size as f64, font_family),
        );
    }
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for g in &glyphs {
        min_x = min_x.min(g.rect.0);
        min_y = min_y.min(g.rect.1);
        max_x = max_x.max(g.rect.0 + g.rect.2);
        max_y = max_y.max(g.rect.1 + g.rect.3);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

pub fn animation_visible_hit(
    glyphs: &[AnimatedGlyph],
    scroll_y: f64,
    viewport_height: f64,
) -> bool {
    if glyphs.is_empty() {
        return false;
    }
    let top = scroll_y;
    let bottom = top + viewport_height.max(1.0);
    glyphs.iter().any(|g| {
        let glyph_top = g.rect.1;
        let glyph_bottom = g.rect.1 + g.rect.3;
        glyph_bottom >= top && glyph_top <= bottom
    })
}

pub fn log_animation_created(
    label: &str,
    offset: usize,
    glyph_count: usize,
    visible_line_hit: bool,
    current_is_scrolling: bool,
    current_typing_animation_enabled: bool,
) {
    if editor_animation_debug_enabled() {
        eprintln!(
            "{}: offset={}, glyph_count={}, visible_line_hit={}, scrolling={}, enabled={}",
            label,
            offset,
            glyph_count,
            visible_line_hit,
            current_is_scrolling,
            current_typing_animation_enabled,
        );
    }
}
