use crate::editor::layout::CaretAffinity;
use crate::editor::renderer;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{
    QImage, QBrush, QColor, QLineF, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QRectF,
    QString,
};
use std::time::Instant;

use super::{sujian_editor_debug_enabled, SujianEditorItem};
use super::cursor_controller::CursorUpdateResult;

/// A byte range that should be skipped during static text rendering
/// because an animation is currently animating those glyphs.
/// When the animation finishes, the skip range is cleared and the
/// static layer repaints with the full text.
#[derive(Clone, Debug)]
pub struct AnimatedSkipRange {
    pub byte_start: usize,
    pub byte_end: usize,
}

pub struct ScrollBuffer {
    pub image: QImage,
    pub buffer_scroll_y: f64,
    pub buffer_content_h: f64,
    pub buffer_logical_h: f64,
    pub dpr: f64,
    pub text_revision: u64,
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

// TextMaskRange and StaticTextRenderPlan removed: static layer now always
// renders the full text.  Animation overlay is purely additive — it draws
// on top of the static text, so a glyph-calculation error in the overlay
// can never cause text to disappear.

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
    pub(crate) fn has_active_animation(&self) -> bool {
        let now = Instant::now();
        self.insert_animation.as_ref().is_some_and(|a| !a.is_finished(now))
            || self.delete_animation.as_ref().is_some_and(|a| !a.is_finished(now))
    }

    // static_text_render_plan() removed: the static layer now always renders
    // the full text without masks.  The animation overlay is purely additive.

    pub(crate) fn cleanup_finished_animations(&mut self) {
        let now = Instant::now();
        let mut insert_done = false;
        let mut delete_done = false;
        if let Some(ref anim) = self.insert_animation {
            if anim.is_finished(now) {
                self.insert_animation = None;
                insert_done = true;
            }
        }
        if let Some(ref anim) = self.delete_animation {
            if anim.is_finished(now) {
                self.delete_animation = None;
                delete_done = true;
            }
        }
        // When animations finish, clear skip ranges and invalidate the
        // scroll buffer so the static layer repaints with full text.
        if (insert_done || delete_done) && !self.animated_skip_ranges.is_empty() {
            self.animated_skip_ranges.clear();
            self.scroll_buffer = None;
            self.render_dirty = true;
        }
    }

    /// 渲染静态正文到 painter。不包含任何动画内容。
    /// buffer_scroll_y 和 buffer_h 定义缓冲区可见范围。
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
        // When animated_skip_ranges is non-empty, skip rendering the glyphs
        // that are being animated by the QML ghost overlay. This makes insert
        // animations look like the characters fly from the cursor to their
        // final position, rather than appearing instantly in the static layer
        // with a ghost on top.
        let skip_ranges = &self.animated_skip_ranges;
        for line_idx in vis_start..vis_end {
            let line = &lines[line_idx];
            let text_y = self
                .editor_layout
                .text_baseline_y(line, font_size, &font_family)
                + paint_offset_y;

            if skip_ranges.is_empty() {
                // Fast path: no animations active, render the full line
                let text = self.buffer.text[line.start..line.end].to_string();
                renderer::draw_text(
                    painter,
                    line.x,
                    text_y,
                    fs,
                    self.current_text_color.clone(),
                    text.into(),
                );
            } else {
                // Slow path: skip animated byte ranges within this line
                let line_start = line.start;
                let line_end = line.end;
                let mut seg_start = line_start;

                // Collect skip ranges that overlap this line, sorted
                let mut line_skips: Vec<(usize, usize)> = skip_ranges.iter()
                    .filter_map(|r| {
                        let overlap_start = r.byte_start.max(line_start);
                        let overlap_end = r.byte_end.min(line_end);
                        if overlap_start < overlap_end {
                            Some((overlap_start, overlap_end))
                        } else {
                            None
                        }
                    })
                    .collect();
                line_skips.sort_by_key(|(s, _)| *s);

                // Render segments between skip ranges
                for (skip_s, skip_e) in &line_skips {
                    if seg_start < *skip_s {
                        let text = self.buffer.text[seg_start..*skip_s].to_string();
                        let x_offset = if seg_start > line_start {
                            self.editor_layout.text_width(
                                &self.buffer.text[line_start..seg_start],
                                font_size,
                                &font_family,
                            )
                        } else {
                            0.0
                        };
                        renderer::draw_text(
                            painter,
                            line.x + x_offset,
                            text_y,
                            fs,
                            self.current_text_color.clone(),
                            text.into(),
                        );
                    }
                    seg_start = *skip_e;
                }
                // Render remaining text after last skip
                if seg_start < line_end {
                    let text = self.buffer.text[seg_start..line_end].to_string();
                    let x_offset = if seg_start > line_start {
                        self.editor_layout.text_width(
                            &self.buffer.text[line_start..seg_start],
                            font_size,
                            &font_family,
                        )
                    } else {
                        0.0
                    };
                    renderer::draw_text(
                        painter,
                        line.x + x_offset,
                        text_y,
                        fs,
                        self.current_text_color.clone(),
                        text.into(),
                    );
                }
            }
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
                    self.cursor_ctrl.affinity,
                ) {
                    let x = self.editor_layout.cursor_x_for_line(
                        &snapshot,
                        line,
                        pc,
                        self.cursor_ctrl.affinity,
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
        let elapsed = paint_start.elapsed();
        if elapsed.as_millis() > 4 && renderer::should_log_slow_paint(self.last_slow_paint_log, now_cleanup) {
            self.last_slow_paint_log = Some(now_cleanup);
            eprintln!(
                "sujian_paint_onto: elapsed_ms={}, vis_lines=[{}..{}]={}, scrolling={}, buffer_h={:.1}",
                elapsed.as_millis(),
                vis_start, vis_end,
                vis_end.saturating_sub(vis_start),
                self.current_is_scrolling,
                self.current_content_height,
            );
        }
    }

    /// 判断当前 scroll buffer 是否可以复用。返回 None 表示可以复用，
    /// 返回 Some(reason) 表示需要重新渲染及原因。
    /// 使用早返回风格，避免多层嵌套 if/else。
    pub(crate) fn scroll_buffer_miss_reason(
        &self,
        scroll_y: f64,
        vp_h: f64,
        content_h: f64,
        dpr: f64,
    ) -> Option<&'static str> {
        let Some(buf) = self.scroll_buffer.as_ref() else {
            return Some("no_buffer");
        };

        if self.editor_layout.cache().is_none() {
            return Some("layout_invalidated");
        }

        if buf.text_revision != self.text_revision {
            return Some("text_revision_changed");
        }

        if (content_h - buf.buffer_content_h).abs() > 1.0 {
            return Some("content_changed");
        }

        if (dpr - buf.dpr).abs() > 0.01 {
            return Some("dpr_changed");
        }

        if !buf.contains_viewport(scroll_y, vp_h) {
            return Some("outside_buffer");
        }

        None
    }

    pub(crate) fn render_to_image(&mut self) -> Option<(QImage, f64, f64)> {
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

        let miss_reason = self.scroll_buffer_miss_reason(scroll_y, vp_h, content_h, dpr);

        let Some(miss_reason) = miss_reason else {
            self.render_dirty = false;
            return None;
        };

        if sujian_editor_debug_enabled() {
            eprintln!(
                "render_to_image: rebuilding buffer, miss_reason={}, scroll_y={:.1}, content_h={:.1}, vp_h={:.1}",
                miss_reason, scroll_y, content_h, vp_h
            );
        }

        let phys_w = ((img_w as f64 * dpr) as i32).max(1);
        let phys_h = ((buffer_h * dpr) as i32).max(1);
        let mut image = QImage::new(
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
            text_revision: self.text_revision,
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

    /// 渲染动画 overlay（插入/删除 ghost 字）到独立的 painter。
    /// 返回 Some(image) 如果有活跃动画，None 表示没有动画需要绘制。
    pub(crate) fn paint_animation_overlay(&mut self) -> Option<(QImage, f64, f64)> {
        let now = Instant::now();
        let has_insert = self.insert_animation.as_ref().is_some_and(|a| !a.is_finished(now));
        let has_delete = self.delete_animation.as_ref().is_some_and(|a| !a.is_finished(now));

        if !has_insert && !has_delete {
            return None;
        }

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };
        let width = self.bounding_width();
        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let scroll_y = self.current_scroll_y as f64;
        let text = self.buffer.text.clone();
        let snapshot = self.layout_snapshot_for_text(&text, width);
        let content_h = snapshot.lines.last().map(|l| l.y + l.height + self.current_padding as f64).unwrap_or(vp_h);

        let overscan = vp_h * 2.5;
        let min_y = (scroll_y - overscan).max(0.0);
        let max_y = (scroll_y + vp_h + overscan).min(content_h.max(vp_h));
        let buffer_h = max_y - min_y;

        let img_w = (width as i32).max(1);
        let phys_w = ((img_w as f64 * dpr) as i32).max(1);
        let phys_h = ((buffer_h * dpr) as i32).max(1);
        let mut image = QImage::new(
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

        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();
        let fs = font_size as f32;
        let ff: QString = font_family.clone().into();
        cpp!(unsafe [painter as "QPainter*", fs as "float", ff as "QString"] {
            QFont f(ff);
            f.setPixelSize(static_cast<int>(fs));
            painter->setFont(f);
        });

        let paint_offset_y = -min_y;
        let now_anim = Instant::now();

        // Draw insert animation ghosts — glyphs fly from cursor origin to
        // their final position. Since the static layer skips these glyphs
        // (via animated_skip_ranges), the ghost is the only rendering of
        // these characters during the animation.
        if let Some(ref insert_anim) = self.insert_animation {
            let eased = 1.0 - (1.0 - insert_anim.progress(now_anim)).powi(3);
            let origin_x = insert_anim.origin_cursor_rect.0;
            let origin_y = insert_anim.origin_cursor_rect.1;
            let base_color = renderer::color_from_qstring(self.current_text_color.clone());

            for glyph in &insert_anim.glyphs {
                let (gx, gy, _gw, gh) = glyph.rect;
                if gy + gh < scroll_y || gy > scroll_y + vp_h {
                    continue;
                }
                // Interpolate from cursor origin to final glyph position
                let current_x = origin_x + (gx - origin_x) * eased;
                let current_y = origin_y + (gy - origin_y) * eased;
                // Fade in: 0 → 255
                let alpha = (eased * 255.0).round() as i32;
                renderer::draw_text_color(
                    painter,
                    current_x,
                    glyph.baseline_y + (current_y - gy) + paint_offset_y,
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

        // Draw delete animation ghosts
        if let Some(ref delete_anim) = self.delete_animation {
            let eased = 1.0 - (1.0 - delete_anim.progress(now_anim)).powi(3);
            let target_x = delete_anim.target_cursor_rect.0;
            let target_y_top = delete_anim.target_cursor_rect.1;
            let base_color = renderer::color_from_qstring(self.current_text_color.clone());
            for glyph in &delete_anim.glyphs {
                let (gx, gy, _gw, gh) = glyph.rect;
                if gy + gh < scroll_y || gy > scroll_y + vp_h {
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
        }

        renderer::sujian_delete_painter(painter_ptr);
        Some((image, min_y, buffer_h))
    }

    /// Update cursor visual position from layout-computed position.
    ///
    /// **IMPORTANT**: This method MUST only be called from the GUI thread.
    /// It directly emits signals and calls inputMethod()->update().
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        let scroll_y = self.current_scroll_y as f64;
        let layout_res = self.editor_layout_cursor_rect(
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
            scroll_y,
        );

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let is_preediting = !self.preedit_text.is_empty();

        let result = self.cursor_ctrl.update(
            cursor_x,
            cursor_y,
            cursor_h,
            visual_line_id,
            scroll_y,
            self.current_smooth_cursor_enabled,
            self.current_cursor_animation_duration_ms,
            self.current_is_scrolling,
            is_selecting,
            is_preediting,
            self.current_editor_enabled,
            self.buffer.has_selection(),
            vp_h,
        );

        if result.needs_repaint {
            let item = self as &dyn QQuickItem;
            item.update();
        }

        // GUI 线程直接 IME 更新
        if result.ime_needs_update {
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                });
            }
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
                self.buffer.cursor, self.cursor_ctrl.target_x, self.cursor_ctrl.target_y, self.cursor_ctrl.visual_x, self.cursor_ctrl.visual_y, self.cursor_ctrl.animation.is_some(), scroll_y, line_info
            );
        }

        if self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.dirty = true;
            self.request_frame_update();
        }

        result
    }
}

// compute_visible_segments and related helpers removed — the static layer
// no longer uses masks.  All text is always rendered in full.

#[cfg(test)]
mod tests {
    use super::*;


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
            text_revision: 1,
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

    #[test]
    fn test_insert_animation_glyph_ranges() {
        let text = "hello world";
        let glyphs = vec![
            AnimatedGlyph {
                byte_start: 6,
                byte_end: 11,
                text: "world".to_string(),
                rect: (50.0, 0.0, 40.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            },
        ];
        let insert_anim = InsertAnimation {
            glyphs,
            origin_cursor_rect: (50.0, 0.0, 2.0, 20.0),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        assert!(!insert_anim.is_finished(Instant::now()));
        assert_eq!(insert_anim.glyphs[0].byte_start, 6);
        assert_eq!(insert_anim.glyphs[0].byte_end, 11);
    }

    #[test]
    fn test_delete_animation_uses_old_layout_and_new_cursor() {
        let old_text = "hello";
        let new_text = "he";
        let glyphs = vec![
            AnimatedGlyph {
                byte_start: 2,
                byte_end: 3,
                text: "l".to_string(),
                rect: (20.0, 0.0, 8.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            },
            AnimatedGlyph {
                byte_start: 3,
                byte_end: 4,
                text: "l".to_string(),
                rect: (28.0, 0.0, 8.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            },
            AnimatedGlyph {
                byte_start: 4,
                byte_end: 5,
                text: "o".to_string(),
                rect: (36.0, 0.0, 8.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            },
        ];
        let delete_anim = DeleteAnimation {
            glyphs,
            target_cursor_rect: (20.0, 0.0, 2.0, 20.0),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        assert!(!delete_anim.is_finished(Instant::now()));
        assert_eq!(delete_anim.glyphs.len(), 3);
        assert_eq!(delete_anim.glyphs[0].text, "l");
        assert_eq!(delete_anim.target_cursor_rect.0, 20.0);
    }

    #[test]
    fn test_unicode_delete_animation_no_panic() {
        let old_text = "你好🙂世界";
        let glyphs: Vec<AnimatedGlyph> = old_text
            .char_indices()
            .map(|(i, c)| AnimatedGlyph {
                byte_start: i,
                byte_end: i + c.len_utf8(),
                text: c.to_string(),
                rect: (0.0, 0.0, 10.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            })
            .collect();
        let delete_anim = DeleteAnimation {
            glyphs,
            target_cursor_rect: (0.0, 0.0, 2.0, 20.0),
            start_time: Instant::now(),
            duration_ms: 160,
        };
        for g in &delete_anim.glyphs {
            assert!(g.byte_start < old_text.len());
            assert!(g.byte_end <= old_text.len());
            assert!(old_text.is_char_boundary(g.byte_start));
            assert!(old_text.is_char_boundary(g.byte_end));
        }
    }

    #[test]
    fn test_emoji_delete_animation_no_panic() {
        let old_text = "a🙂b";
        let glyphs: Vec<AnimatedGlyph> = old_text
            .char_indices()
            .map(|(i, c)| AnimatedGlyph {
                byte_start: i,
                byte_end: i + c.len_utf8(),
                text: c.to_string(),
                rect: (0.0, 0.0, 10.0, 20.0),
                baseline_y: 16.0,
                line_index: 0,
            })
            .collect();
        assert_eq!(glyphs.len(), 3);
        assert_eq!(glyphs[0].text, "a");
        assert_eq!(glyphs[1].text, "🙂");
        assert_eq!(glyphs[2].text, "b");
    }

    #[test]
    fn test_mixed_text_insert_delete_no_panic() {
        let text = "a，b🙂c";
        for (i, c) in text.char_indices() {
            assert!(text.is_char_boundary(i));
            assert_eq!(&text[i..i + c.len_utf8()], c.to_string());
        }
    }

}
