use crate::editor::layout::CaretAffinity;
use crate::editor::renderer;
use crate::editor::scene_graph;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{
    QBrush, QColor, QLineF, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QRectF, QString,
};
use std::time::Instant;

use super::buffer::EditorSnapshot;
use super::{editor_animation_debug_enabled, sujian_editor_debug_enabled, EditorBuffer, SujianEditorItem};

pub struct ScrollBuffer {
    pub image: qmetaobject::QImage,
    pub buffer_scroll_y: f64,
    pub buffer_content_h: f64,
    pub buffer_logical_h: f64,
    pub dpr: f64,
}

impl ScrollBuffer {
    pub fn contains_viewport(&self, scroll_y: f64, vp_h: f64) -> bool {
        let threshold = vp_h * 0.5;
        let near_top = scroll_y < self.buffer_scroll_y + threshold;
        let near_bottom =
            scroll_y + vp_h > self.buffer_scroll_y + self.buffer_logical_h - threshold;

        if near_top && self.buffer_scroll_y > 0.1 {
            return false;
        }
        if near_bottom
            && (self.buffer_scroll_y + self.buffer_logical_h) < self.buffer_content_h - 0.1
        {
            return false;
        }

        if scroll_y < self.buffer_scroll_y
            || scroll_y + vp_h > self.buffer_scroll_y + self.buffer_logical_h
        {
            return false;
        }

        true
    }

    pub fn clamp_source_rect(&self, scroll_y: f64, vp_h: f64) -> (f64, f64) {
        let mut src_y = scroll_y - self.buffer_scroll_y;
        let mut src_h = vp_h;

        if src_y < 0.0 {
            src_y = 0.0;
        }

        if src_y + src_h > self.buffer_logical_h {
            if src_h > self.buffer_logical_h {
                src_h = self.buffer_logical_h;
            }
            if src_y + src_h > self.buffer_logical_h {
                src_y = self.buffer_logical_h - src_h;
            }
        }

        let phys_h = (self.image.size().height as f64).max(1.0);
        let dpr = self.dpr;

        let mut phys_src_y = src_y * dpr;
        let mut phys_src_h = src_h * dpr;

        if phys_src_y < 0.0 {
            phys_src_y = 0.0;
        }
        if phys_src_y + phys_src_h > phys_h {
            if phys_src_h > phys_h {
                phys_src_h = phys_h;
            }
            if phys_src_y + phys_src_h > phys_h {
                phys_src_y = phys_h - phys_src_h;
            }
        }

        src_y = phys_src_y / dpr;
        src_h = phys_src_h / dpr;

        (src_y, src_h)
    }
}

/// 光标动画状态 — 固定时长 tween，不再用指数追赶
#[derive(Clone, Debug)]
pub struct CursorAnimationState {
    pub start_x: f64,
    pub start_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub start_time: Instant,
    pub duration_ms: u64,
}

impl CursorAnimationState {
    pub fn current_position(&self, now: Instant) -> (f64, f64) {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        let t = (elapsed_ms / self.duration_ms as f64).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        let x = self.start_x + (self.target_x - self.start_x) * eased;
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (x, y)
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }
}

#[derive(Clone, Debug)]
pub struct AnimatedGlyph {
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
    pub rect: (f64, f64, f64, f64),
    pub baseline_y: f64,
    pub line_index: usize,
}

#[derive(Clone, Debug)]
pub struct InsertAnimation {
    pub glyphs: Vec<AnimatedGlyph>,
    pub origin_cursor_rect: (f64, f64, f64, f64),
    pub start_time: Instant,
    pub duration_ms: u64,
}

impl InsertAnimation {
    pub fn progress(&self, now: Instant) -> f64 {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        (elapsed_ms / self.duration_ms as f64).min(1.0)
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }
}

#[derive(Clone, Debug)]
pub struct DeleteAnimation {
    pub glyphs: Vec<AnimatedGlyph>,
    pub target_cursor_rect: (f64, f64, f64, f64),
    pub start_time: Instant,
    pub duration_ms: u64,
}

impl DeleteAnimation {
    pub fn progress(&self, now: Instant) -> f64 {
        let elapsed_ms = now.duration_since(self.start_time).as_millis() as f64;
        (elapsed_ms / self.duration_ms as f64).min(1.0)
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        now.duration_since(self.start_time).as_millis() as u64 >= self.duration_ms
    }
}

impl SujianEditorItem {
    /// 渲染文字到 painter。buffer_scroll_y 和 buffer_h 定义缓冲区可见范围。
    pub(crate) fn paint_onto(&mut self, painter: &mut QPainter, buffer_scroll_y: f64, buffer_h: f64) {
        let paint_start = Instant::now();
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        let content_h = snapshot.content_height;
        if (self.current_content_height - content_h).abs() > 0.5 {
            self.current_content_height = content_h;
            self.content_height_dirty.set(true);
        }

        let scroll_y = buffer_scroll_y;
        let paint_offset_y = -scroll_y;

        painter.set_render_hint(QPainterRenderHint::TextAntialiasing, true);
        painter.fill_rect(
            QRectF {
                x: 0.0,
                y: 0.0,
                width,
                height: buffer_h,
            },
            QBrush::from_color(QColor::from_rgba(0, 0, 0, 0)),
        );

        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let fs = font_size as f32;
        let ff: QString = font_family.clone().into();
        cpp!(unsafe [painter as "QPainter*", fs as "float", ff as "QString"] {
            QFont f(ff);
            f.setPixelSize(static_cast<int>(fs));
            painter->setFont(f);
        });

        let lines = &snapshot.lines;
        let vis_start = lines.partition_point(|l| l.y + l.height < scroll_y);
        let vis_end = lines
            .len()
            .min(lines.partition_point(|l| l.y < scroll_y + buffer_h + font_size * 2.0) + 1);

        let selection = self.buffer.selection_range();
        let now_anim = Instant::now();
        let mut needs_animation_repaint = false;

        // ── Layer 1: Selection background ──
        for line in &lines[vis_start..vis_end] {
            if self.buffer.has_selection() && selection.1 > line.start && selection.0 < line.end {
                let sel_start = selection.0.max(line.start);
                let sel_end = selection.1.min(line.end);
                let x_start = self.editor_layout.cursor_x_for_line(
                    &snapshot,
                    line,
                    sel_start,
                    CaretAffinity::Downstream,
                );
                let x_end = self.editor_layout.cursor_x_for_line(
                    &snapshot,
                    line,
                    sel_end,
                    CaretAffinity::Upstream,
                );
                renderer::draw_rect(
                    painter,
                    x_start,
                    line.y + paint_offset_y,
                    (x_end - x_start).max(2.0),
                    line.height,
                    self.current_selection_color.clone(),
                );
            }
        }

        // ── Layer 2: Base text ──
        let active_insert: Option<&InsertAnimation> = self.insert_animation.as_ref();
        let active_delete: Option<&DeleteAnimation> = self.delete_animation.as_ref();
        let had_insert_animation = active_insert.is_some();
        let had_delete_animation = active_delete.is_some();

        for line_idx in vis_start..vis_end {
            let line = &lines[line_idx];
            let text = self.buffer.text[line.start..line.end].to_string();
            let text_y = self
                .editor_layout
                .text_baseline_y(line, font_size, &font_family)
                + paint_offset_y;

            if let Some(ref insert_anim) = active_insert {
                let insert_in_line = insert_anim.glyphs.iter().any(|g| g.line_index == line_idx);
                if insert_in_line {
                    let eased = 1.0 - (1.0 - insert_anim.progress(now_anim)).powi(3);
                    if editor_animation_debug_enabled() {
                        eprintln!(
                            "insert_animation_paint: progress={:.3}, glyph_count={}, visible_line_hit=true",
                            insert_anim.progress(now_anim),
                            insert_anim.glyphs.len(),
                        );
                    }
                    let first_glyph = insert_anim.glyphs.first().unwrap();
                    let last_glyph = insert_anim.glyphs.last().unwrap();
                    let insert_start_byte = first_glyph.byte_start;
                    let insert_end_byte = last_glyph.byte_end;

                    if insert_start_byte > line.start && insert_start_byte <= line.end {
                        let before = &self.buffer.text[line.start..insert_start_byte];
                        renderer::draw_text(
                            painter,
                            line.x,
                            text_y,
                            fs,
                            self.current_text_color.clone(),
                            before.to_string().into(),
                        );
                    }

                    let clip_origin_x = insert_anim.origin_cursor_rect.0;
                    let final_insert_w: f64 = insert_anim.glyphs.iter().map(|g| g.rect.2).sum();
                    let clip_right = clip_origin_x + final_insert_w * eased;
                    let base_color = renderer::color_from_qstring(self.current_text_color.clone());

                    for glyph in &insert_anim.glyphs {
                        let gx = glyph.rect.0;
                        let gy = glyph.baseline_y + paint_offset_y;
                        if gx + glyph.rect.2 <= clip_right + 0.5 {
                            renderer::draw_text(
                                painter,
                                gx,
                                gy,
                                fs,
                                self.current_text_color.clone(),
                                glyph.text.clone().into(),
                            );
                        } else if gx < clip_right + 0.5 {
                            let visible_frac = ((clip_right - gx) / glyph.rect.2).clamp(0.0, 1.0);
                            let alpha = (visible_frac * 255.0).round() as i32;
                            renderer::draw_text_color(
                                painter,
                                gx,
                                gy,
                                QColor::from_rgba(
                                    base_color.red(),
                                    base_color.green(),
                                    base_color.blue(),
                                    alpha,
                                ),
                                glyph.text.clone().into(),
                            );
                        }
                    }

                    if insert_end_byte < line.end {
                        let insert_w = final_insert_w;
                        let after_x = first_glyph.rect.0 + insert_w;
                        let after = &self.buffer.text[insert_end_byte..line.end];
                        renderer::draw_text(
                            painter,
                            after_x,
                            text_y,
                            fs,
                            self.current_text_color.clone(),
                            after.to_string().into(),
                        );
                    }

                    needs_animation_repaint = true;
                    continue;
                }
            }

            renderer::draw_text(
                painter,
                line.x,
                text_y,
                fs,
                self.current_text_color.clone(),
                text.into(),
            );
        }

        if let Some(ref delete_anim) = active_delete {
            let eased = 1.0 - (1.0 - delete_anim.progress(now_anim)).powi(3);
            let target_x = delete_anim.target_cursor_rect.0;
            let target_y_top = delete_anim.target_cursor_rect.1;
            let base_color = renderer::color_from_qstring(self.current_text_color.clone());
            if editor_animation_debug_enabled() {
                eprintln!(
                    "delete_animation_paint: progress={:.3}, glyph_count={}",
                    delete_anim.progress(now_anim),
                    delete_anim.glyphs.len(),
                );
            }
            for glyph in &delete_anim.glyphs {
                let (gx, gy, _gw, gh) = glyph.rect;
                if gy + gh < scroll_y || gy > scroll_y + buffer_h {
                    continue;
                }
                let offset_x = (target_x - gx) * eased;
                let offset_y = (target_y_top - gy) * eased;
                let alpha = ((1.0 - eased) * 255.0).round() as i32;
                renderer::draw_text_color(
                    painter,
                    gx + offset_x,
                    glyph.baseline_y + offset_y + paint_offset_y,
                    QColor::from_rgba(
                        base_color.red(),
                        base_color.green(),
                        base_color.blue(),
                        alpha,
                    ),
                    glyph.text.clone().into(),
                );
            }
            needs_animation_repaint = true;
        }

        // ── Layer 3: Preedit ──
        if !self.preedit_text.is_empty() {
            let pc = self.buffer.cursor;
            for (idx, line) in lines.iter().enumerate() {
                if idx < vis_start || idx >= vis_end {
                    continue;
                }
                if self.editor_layout.line_contains_cursor_with_affinity(
                    lines,
                    idx,
                    pc,
                    self.current_cursor_affinity,
                ) {
                    let x = self.editor_layout.cursor_x_for_line(
                        &snapshot,
                        line,
                        pc,
                        self.current_cursor_affinity,
                    );
                    let baseline =
                        self.editor_layout
                            .text_baseline_y(line, font_size, &font_family)
                            + paint_offset_y;
                    renderer::draw_text(
                        painter,
                        x,
                        baseline,
                        fs,
                        self.current_text_color.clone(),
                        self.preedit_text.clone().into(),
                    );
                    let preedit_w =
                        self.editor_layout
                            .text_width(&self.preedit_text, font_size, &font_family);
                    painter.set_pen(QPen::from_color(renderer::color_from_qstring(
                        self.current_text_color.clone(),
                    )));
                    let underline_y = baseline + 2.0;
                    let line_f = QLineF {
                        pt1: QPointF { x, y: underline_y },
                        pt2: QPointF {
                            x: x + preedit_w,
                            y: underline_y,
                        },
                    };
                    painter.draw_line(line_f);
                    break;
                }
            }
        }

        let now_cleanup = Instant::now();
        if let Some(ref anim) = self.insert_animation {
            if anim.is_finished(now_cleanup) {
                self.insert_animation = None;
            }
        }
        if let Some(ref anim) = self.delete_animation {
            if anim.is_finished(now_cleanup) {
                self.delete_animation = None;
            }
        }
        if needs_animation_repaint && !self.current_is_scrolling {
            self.request_repaint();
        }

        let elapsed = paint_start.elapsed();
        if elapsed.as_millis() > 4 && renderer::should_log_slow_paint(self.last_slow_paint_log, now_cleanup) {
            self.last_slow_paint_log = Some(now_cleanup);
            eprintln!(
                "sujian_paint_onto: elapsed_ms={}, vis_lines=[{}..{}]={}, insert_anim={}, delete_anim={}, scrolling={}, buffer_h={:.1}",
                elapsed.as_millis(),
                vis_start, vis_end,
                vis_end.saturating_sub(vis_start),
                had_insert_animation,
                had_delete_animation,
                self.current_is_scrolling,
                self.current_content_height,
            );
        }
    }

    pub(crate) fn render_to_image(&mut self) -> Option<(qmetaobject::QImage, f64, f64)> {
        let render_start = Instant::now();
        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };
        let width = self.bounding_width();
        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let img_w = (width as i32).max(1);
        let scroll_y = self.current_scroll_y as f64;
        let content_h = self.current_content_height as f64;

        let overscan = vp_h * 2.5;
        let min_y = (scroll_y - overscan).max(0.0);
        let max_y = (scroll_y + vp_h + overscan).min(content_h.max(vp_h));
        let buffer_h = max_y - min_y;

        let mut miss_reason = "none";
        let mut needs_render = true;

        if self.scroll_buffer.is_none() {
            miss_reason = "no_buffer";
        } else if self.editor_layout.cache().is_none() {
            miss_reason = "layout_invalidated";
        } else {
            let buf = self.scroll_buffer.as_ref().unwrap();
            let content_changed = (content_h - buf.buffer_content_h).abs() > 1.0;
            if content_changed {
                miss_reason = "content_changed";
            } else {
                let dpr_changed = (dpr - buf.dpr).abs() > 0.01;
                if dpr_changed {
                    miss_reason = "dpr_changed";
                } else {
                    let inside_buffer = buf.contains_viewport(scroll_y, vp_h);
                    if !inside_buffer {
                        miss_reason = "outside_buffer";
                    } else {
                        needs_render = false;
                    }
                }
            }
        }

        if !needs_render {
            self.render_dirty = false;
            return None;
        }

        if sujian_editor_debug_enabled() {
            eprintln!(
                "render_to_image: rebuilding buffer, miss_reason={}, scroll_y={:.1}, content_h={:.1}, vp_h={:.1}",
                miss_reason, scroll_y, content_h, vp_h
            );
        }

        let phys_w = ((img_w as f64 * dpr) as i32).max(1);
        let phys_h = ((buffer_h * dpr) as i32).max(1);
        let mut image = qmetaobject::QImage::new(
            qmetaobject::QSize {
                width: phys_w as u32,
                height: phys_h as u32,
            },
            qmetaobject::ImageFormat::ARGB32_Premultiplied,
        );
        image.fill(qmetaobject::QColor::from_rgba(0, 0, 0, 0));

        let painter_ptr = renderer::sujian_create_painter_scaled(&mut image, dpr);
        if painter_ptr.is_null() {
            return Some((image, min_y, buffer_h));
        }

        let painter: &mut QPainter = unsafe { &mut *painter_ptr };
        self.paint_onto(painter, min_y, buffer_h);

        renderer::sujian_delete_painter(painter_ptr);

        self.render_dirty = false;
        self.scroll_buffer = Some(ScrollBuffer {
            image: image.clone(),
            buffer_scroll_y: min_y,
            buffer_content_h: content_h,
            buffer_logical_h: buffer_h,
            dpr,
        });

        let render_elapsed = render_start.elapsed();
        if renderer::should_log_slow_paint(self.last_slow_paint_log, render_start) {
            self.last_slow_paint_log = Some(render_start);
            let vis_lines = self
                .editor_layout
                .cache()
                .map(|c| {
                    let start = c.lines.partition_point(|l| l.y + l.height < min_y);
                    let end = c.lines.len().min(
                        c.lines.partition_point(|l| {
                            l.y < min_y + buffer_h + self.current_font_pixel_size as f64 * 2.0
                        }) + 1,
                    );
                    end.saturating_sub(start)
                })
                .unwrap_or(0);
            eprintln!(
                "sujian_render_to_image: elapsed_ms={}, img={}x{}(phys {}x{}, dpr={}), vis_lines={}, scroll_y={:.1}, buf_scroll={:.1}, buf_h={:.1}",
                render_elapsed.as_millis(),
                img_w, (buffer_h as i32), phys_w, phys_h, dpr,
                vis_lines,
                scroll_y, min_y, buffer_h,
            );
        }

        Some((image, min_y, buffer_h))
    }

    pub(crate) fn update_cursor_visual_position(&mut self) {
        let scroll_y = self.current_scroll_y as f64;
        let layout_res = self.editor_layout_cursor_rect(
            self.buffer.cursor,
            self.current_cursor_affinity,
            scroll_y,
        );

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        let old_visual_line_id = self.current_visual_line_id;
        self.current_visual_line_id = Some(visual_line_id);

        let scroll_changed = (self.last_cursor_scroll_y - scroll_y).abs() > 0.01;
        self.last_cursor_scroll_y = scroll_y;

        let old_x = self.target_cursor_x;
        let old_y = self.target_cursor_y;
        let old_visible = self.cursor_visible;

        self.target_cursor_x = cursor_x;
        self.target_cursor_y = cursor_y;
        self.ime_cursor_rect_h = cursor_h;
        self.cursor_visual_h = cursor_h;

        if (old_x - cursor_x).abs() > 0.01 || (old_y - cursor_y).abs() > 0.01 {
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                if (obj_ptr) {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                }
            });
        }

        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < vp_h;
        let new_visible = self.current_editor_enabled
            && !self.buffer.has_selection()
            && in_viewport
            && !self.current_is_scrolling;
        self.cursor_visible = new_visible;

        if self.cursor_visible {
            debug_assert!(
                self.target_cursor_y + cursor_h > 0.0 && self.target_cursor_y < vp_h,
                "Debug assert failed: cursor is visible but target_cursor_y ({:.2}) is outside viewport [0, {:.2}]",
                self.target_cursor_y, vp_h
            );
        }

        if !new_visible {
            self.cursor_animation = None;
            self.cursor_visual_x = cursor_x;
            self.cursor_visual_y = cursor_y;
            if old_visible {
                self.cursor_dirty = true;
                let item = self as &dyn QQuickItem;
                item.update();
            }
            return;
        }

        let now = Instant::now();
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.preedit_text.is_empty();

        let line_changed =
            old_visual_line_id.is_none() || old_visual_line_id != Some(visual_line_id);
        let half_line = cursor_h * 0.5;
        let target_y_changed_more_than_half_line = (old_y - cursor_y).abs() > half_line;
        let x_diff = (old_x - cursor_x).abs();
        let is_small_x_change = x_diff <= 150.0;

        let should_snap = self.current_is_scrolling
            || is_selecting
            || is_preediting
            || !old_visible
            || self.force_snap_next_cursor
            || scroll_changed
            || line_changed
            || target_y_changed_more_than_half_line
            || !is_small_x_change;

        let (visual_x, visual_y) = if let Some(ref anim) = self.cursor_animation {
            if anim.is_finished(now) {
                (anim.target_x, anim.target_y)
            } else {
                anim.current_position(now)
            }
        } else {
            (old_x, old_y)
        };

        let (final_x, final_y, new_animation) = if should_snap
            || !self.current_smooth_cursor_enabled
        {
            (cursor_x, cursor_y, None)
        } else if let Some(ref anim) = self.cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01 || (anim.target_y - cursor_y).abs() > 0.01 {
                let (cur_x, cur_y) = anim.current_position(now);
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: cur_x,
                    start_y: cur_y,
                    target_x: cursor_x,
                    target_y: cursor_y,
                    start_time: now,
                    duration_ms: duration,
                };
                (cur_x, cur_y, Some(new_anim))
            } else if anim.is_finished(now) {
                (anim.target_x, anim.target_y, None)
            } else {
                let (cur_x, cur_y) = anim.current_position(now);
                (cur_x, cur_y, Some(anim.clone()))
            }
        } else {
            if (visual_x - cursor_x).abs() > 0.01 || (visual_y - cursor_y).abs() > 0.01 {
                let duration = self.current_cursor_animation_duration_ms.max(30) as u64;
                let new_anim = CursorAnimationState {
                    start_x: visual_x,
                    start_y: visual_y,
                    target_x: cursor_x,
                    target_y: cursor_y,
                    start_time: now,
                    duration_ms: duration,
                };
                (visual_x, visual_y, Some(new_anim))
            } else {
                (cursor_x, cursor_y, None)
            }
        };

        self.cursor_animation = new_animation;
        self.cursor_visual_x = final_x;
        self.cursor_visual_y = final_y;
        self.force_snap_next_cursor = false;

        let pos_changed =
            (final_x - old_x).abs() > 0.01 || (final_y - old_y).abs() > 0.01 || !old_visible;
        if pos_changed {
            self.cursor_dirty = true;
        }

        if sujian_editor_debug_enabled() {
            let mut line_info = String::new();
            if let Some(snapshot) = self.editor_layout.cache() {
                if let Some(line) = snapshot.lines.iter().find(|l| l.id == visual_line_id) {
                    let font_size = snapshot.font_size as f64;
                    let font_family = &snapshot.font_family;
                    let ascent = if line.qt_ascent > 0.0 { line.qt_ascent } else { crate::editor::layout::get_font_ascent(font_family, snapshot.font_size) };
                    let descent = if line.qt_descent > 0.0 { line.qt_descent } else { crate::editor::layout::get_font_descent(font_family, snapshot.font_size) };
                    let baseline = crate::editor::layout::text_baseline_y(line, font_size, font_family);
                    let cursor_top_doc = cursor_y + scroll_y;
                    let cursor_top_to_baseline = baseline - cursor_top_doc;
                    let cursor_bottom_to_baseline = cursor_top_doc + cursor_h - baseline;
                    line_info = format!(
                        ", line.y={:.1}, line.height={:.1}, visual_line_id={}, font_ascent={:.1}, font_descent={:.1}, text_baseline_y={:.1}, cursor_top_to_baseline={:.1}, cursor_bottom_to_baseline={:.1}, cursor_h={:.1}, qt_ascent={:.1}, qt_descent={:.1}",
                        line.y, line.height, line.id, ascent, descent, baseline,
                        cursor_top_to_baseline, cursor_bottom_to_baseline, cursor_h,
                        line.qt_ascent, line.qt_descent
                    );
                }
            }
            eprintln!(
                "update_cursor_visual_position: cursor={}, target_x={:.1}, target_y={:.1}, visual_x={:.1}, visual_y={:.1}, is_animating={}, scroll_y={:.1}{}",
                self.buffer.cursor, self.target_cursor_x, self.target_cursor_y, self.cursor_visual_x, self.cursor_visual_y, self.cursor_animation.is_some(), scroll_y, line_info
            );
        }

        if let Some(ref anim) = self.cursor_animation {
            if !anim.is_finished(now) {
                self.cursor_dirty = true;
                let item = self as &dyn QQuickItem;
                item.update();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qmetaobject::QImage;

    #[test]
    fn test_scroll_y_cache_and_buffer_hit() {
        let vp_h: f64 = 1000.0;
        let content_h: f64 = 5000.0;

        let scroll_buffer = ScrollBuffer {
            image: QImage::new(
                qmetaobject::QSize {
                    width: 10,
                    height: 3500,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            ),
            buffer_scroll_y: 0.0,
            buffer_content_h: content_h,
            buffer_logical_h: 3500.0,
            dpr: 1.0,
        };

        assert!(
            scroll_buffer.contains_viewport(100.0, vp_h),
            "Should contain viewport at document top"
        );

        assert!(
            scroll_buffer.contains_viewport(2000.0, vp_h),
            "Should contain viewport in the middle of buffer"
        );

        assert!(
            !scroll_buffer.contains_viewport(3100.0, vp_h),
            "Should not contain viewport if outside buffer"
        );

        let (src_y, src_h) = scroll_buffer.clamp_source_rect(2000.0, vp_h);
        assert_eq!(src_y, 2000.0);
        assert_eq!(src_h, 1000.0);

        let (src_y_neg, src_h_neg) = scroll_buffer.clamp_source_rect(-100.0, vp_h);
        assert_eq!(src_y_neg, 0.0);
        assert!(src_h_neg > 0.0);

        let (src_y_clamp, src_h_clamp) = scroll_buffer.clamp_source_rect(3000.0, vp_h);
        assert!(src_y_clamp + src_h_clamp <= 3500.0);
    }
}
