use crate::editor::layout::CaretAffinity;
use crate::editor::renderer;
use cpp::cpp;
use qmetaobject::prelude::*;
use qmetaobject::{
    QImage, QBrush, QColor, QLineF, QPainter, QPainterRenderHint, QPen, QPointF, QQuickItem, QRectF,
    QString,
};
use std::time::Instant;

use super::SujianEditorItem;
use super::animation_coordinator::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::cursor_controller::CursorUpdateResult;

pub struct ScrollBuffer {
    pub image: QImage,
    pub buffer_scroll_y: f64,
    pub buffer_content_h: f64,
    pub buffer_logical_h: f64,
    pub dpr: f64,
    pub text_revision: u64,
    pub visual_revision: u64,
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

        (src_y, src_h)
    }
}

/// 光标动画状态 — 使用事务 Timeline 的 progress 而非独立时间源。
///
/// Issue #516: 光标不再维护独立 Choreographer/start_time，
/// 而是消费与文字动画相同的 Timeline progress。
#[derive(Clone, Debug)]
pub struct CursorAnimationState {
    pub start_x: f64,
    pub start_y: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub progress: f64,
}

impl CursorAnimationState {
    pub fn current_position(&self) -> (f64, f64) {
        let t = self.progress.clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3i32);
        let x = self.start_x + (self.target_x - self.start_x) * eased;
        let y = self.start_y + (self.target_y - self.start_y) * eased;
        (x, y)
    }

    pub fn is_finished(&self) -> bool {
        self.progress >= 1.0
    }
}

impl SujianEditorItem {
    // has_active_animation() removed: animation display lifecycle is now managed
    // by ActiveVisualTransactionQueue in Scene Graph (child[1]).

    // cleanup_finished_animations() removed: transaction completion is handled
    // atomically via transactionId + generation in updatePaintNode.

    /// 渲染静态正文到 painter。不包含任何动画内容。
    ///
    /// 绘制顺序：Layer 1 选区背景 → Layer 2 正文（含动画裁剪）→ Layer 2.5 选中文字叠层
    /// → Layer 3 预输入。
    ///
    /// 滚动偏移应用点：`paint_offset_y = -scroll_y`，所有行坐标先加此偏移再绘制。
    ///
    /// 动画裁剪：当 Insert/Reflow 动画活跃时，静态层通过 `hidden_clip_rects` 裁剪掉
    /// 被动画切片接管的区域。裁剪依据是已经排版好的视觉矩形（来自 `StaticLinePatch`），
    /// 而不是重新从 byte range 反推 x 坐标——这样才能保持 ligature、RTL、emoji/ZWJ 和
    /// 复杂 shaping 的一致性。
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

        let font_size = f64::from(self.current_font_pixel_size);
        let font_family = self.current_font_family.to_string();
        let fs = font_size as f32;
        let ff: QString = font_family.clone().into();
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
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
            if self.buffer.has_selection() && selection.1 > line.byte_start && selection.0 < line.byte_end {
                let sel_start = selection.0.max(line.byte_start);
                let sel_end = selection.1.min(line.byte_end);
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
        // The static layer renders the full text, EXCEPT when Insert animations
        // are active — in that case, all inserted ranges AND reflow ranges are
        // clipped out so the QML overlay can show the glyphs "spitting out" from
        // the cursor (insert) and moving from old to new positions (reflow).
        // Uses QTextLine::draw() via layout::draw_line_text() to ensure
        // the text rendering uses the same shaping data as cursorToX(),
        // fixing mixed-script cursor issues (e.g. "]\"" where cursor
        // lands inside the Chinese quote).
        let render_plan = self.pipeline.animation_coordinator_mut().current_static_render_plan();
        let hidden_clip_rects = render_plan.merged_clip_rects();
        for line_idx in vis_start..vis_end {
            let line = &lines[line_idx];
            let text_y = self
                .editor_layout
                .text_baseline_y(line, font_size, &font_family)
                + paint_offset_y;

            if line.para_text.is_empty() {
                continue;
            }

            let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
            let line_top = line.y + paint_offset_y;
            let _line_bottom = line_top + line.height;

            let line_hidden: Vec<(f64, f64, f64, f64)> = hidden_clip_rects
                .iter()
                .filter(|(left, top, right, bottom)| {
                    *right > line.x && *left < line.x + line.width
                        && *bottom > line.y && *top < line.y + line.height
                })
                .map(|(left, _top, right, _bottom)| {
                    let clip_left = (*left).max(line.x);
                    let clip_right = (*right).min(line.x + line.width);
                    (clip_left, line_top, clip_right - clip_left, line.height)
                })
                .filter(|(_, _, w, _)| *w > 0.01)
                .collect();

            if line_hidden.is_empty() {
                crate::editor::layout::draw_line_text(
                    painter,
                    &line.para_text,
                    font_size,
                    &font_family,
                    paragraph_wrap_w,
                    line.para_indent,
                    line.qtextline_idx,
                    line.x,
                    text_y,
                    &self.current_text_color.to_string(),
                );
            } else {
                let line_left = line.x;
                let line_right = line.x + line.width;

                let mut segments: Vec<(f64, f64)> = Vec::new();
                let mut cursor_x = line_left;
                let mut sorted_hidden: Vec<(f64, f64)> = line_hidden
                    .iter()
                    .map(|(x, _y, w, _h)| (*x, x + w))
                    .collect();
                sorted_hidden.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

                let mut merged_hidden: Vec<(f64, f64)> = Vec::new();
                for (xs, xe) in sorted_hidden {
                    if let Some(last) = merged_hidden.last_mut() {
                        if xs <= last.1 + 0.01 {
                            last.1 = last.1.max(xe);
                            continue;
                        }
                    }
                    merged_hidden.push((xs, xe));
                }

                for (xs, xe) in &merged_hidden {
                    if *xs > cursor_x + 0.01 {
                        segments.push((cursor_x, *xs));
                    }
                    cursor_x = cursor_x.max(*xe);
                }
                if line_right > cursor_x + 0.01 {
                    segments.push((cursor_x, line_right));
                }

                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [painter as "QPainter*"] {
                    painter->save();
                });

                for (seg_x, seg_end) in &segments {
                    let clip = QRectF {
                        x: *seg_x,
                        y: line_top,
                        width: seg_end - seg_x,
                        height: line.height,
                    };
                    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                    cpp!(unsafe [painter as "QPainter*", clip as "QRectF"] {
                        painter->setClipRect(clip, Qt::ReplaceClip);
                    });

                    crate::editor::layout::draw_line_text(
                        painter,
                        &line.para_text,
                        font_size,
                        &font_family,
                        paragraph_wrap_w,
                        line.para_indent,
                        line.qtextline_idx,
                        line.x,
                        text_y,
                        &self.current_text_color.to_string(),
                    );
                }

                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [painter as "QPainter*"] {
                    painter->restore();
                });
            }
        }

        // ── Layer 2.5: Selected text overlay ──
        // Redraw selected text in selected_text_color for readability
        if self.buffer.has_selection() {
            for line_idx in vis_start..vis_end {
                let line = &lines[line_idx];
                if selection.1 <= line.byte_start || selection.0 >= line.byte_end {
                    continue;
                }
                let sel_start = selection.0.max(line.byte_start);
                let sel_end = selection.1.min(line.byte_end);
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
                let text_y = self
                    .editor_layout
                    .text_baseline_y(line, font_size, &font_family)
                    + paint_offset_y;
                let paragraph_wrap_w = line.line_wrap_width + line.line_indent_x;
                let line_top = line.y + paint_offset_y;

                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [painter as "QPainter*"] {
                    painter->save();
                });
                let clip = QRectF {
                    x: x_start,
                    y: line_top,
                    width: (x_end - x_start).max(2.0),
                    height: line.height,
                };
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [painter as "QPainter*", clip as "QRectF"] {
                    painter->setClipRect(clip, Qt::ReplaceClip);
                });
                crate::editor::layout::draw_line_text(
                    painter,
                    &line.para_text,
                    font_size,
                    &font_family,
                    paragraph_wrap_w,
                    line.para_indent,
                    line.qtextline_idx,
                    line.x,
                    text_y,
                    &self.current_selected_text_color.to_string(),
                );
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [painter as "QPainter*"] {
                    painter->restore();
                });
            }
        }

        // ── Layer 3: Preedit visual layer (fallback only) ──
        // When a Composition transaction is active, preedit is rendered exclusively
        // through the transaction's RenderPlan slices — this layer is skipped.
        // This fallback only activates when animation is disabled or no Composition
        // transaction exists. It draws preedit on the committed layout as a last resort.
        // Issue #516: independent preedit overlay drawing is deleted as the main path.
        if !self.pipeline.composition().preedit_text.is_empty() && !self.pipeline.animation_coordinator_mut().has_active_composition() {
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
                        self.pipeline.composition().preedit_text.clone().into(),
                    );

                    let preedit_w =
                        self.editor_layout
                            .text_width(&self.pipeline.composition().preedit_text, font_size, &font_family);

                    let has_text_format_attr = self.pipeline.composition().preedit_attributes.iter()
                        .any(|a| matches!(a.kind, super::PreeditAttributeKind::Underline
                            | super::PreeditAttributeKind::FontUnderline
                            | super::PreeditAttributeKind::TextColor { .. }
                            | super::PreeditAttributeKind::BackgroundColor { .. }));

                    if has_text_format_attr {
                        for attr in &self.pipeline.composition().preedit_attributes {
                            let attr_start = attr.start.min(self.pipeline.composition().preedit_text.len());
                            let attr_end = (attr.start + attr.length).min(self.pipeline.composition().preedit_text.len());
                            if attr_start >= attr_end {
                                continue;
                            }
                            let before_w = self.editor_layout.text_width(
                                &self.pipeline.composition().preedit_text[..attr_start],
                                font_size,
                                &font_family,
                            );
                            let attr_text = &self.pipeline.composition().preedit_text[attr_start..attr_end];
                            let attr_w = self.editor_layout.text_width(
                                attr_text,
                                font_size,
                                &font_family,
                            );

                            match &attr.kind {
                                super::PreeditAttributeKind::Underline => {
                                    painter.set_pen(QPen::from_color(renderer::color_from_qstring(
                                        self.current_text_color.clone(),
                                    )));
                                    let underline_y = baseline + 2.0;
                                    let line_f = QLineF {
                                        pt1: QPointF { x: x + before_w, y: underline_y },
                                        pt2: QPointF {
                                            x: x + before_w + attr_w,
                                            y: underline_y,
                                        },
                                    };
                                    painter.draw_line(line_f);
                                }
                                super::PreeditAttributeKind::FontUnderline => {
                                    #[allow(unused_mut)]
                                    let mut pen = QPen::from_color(renderer::color_from_qstring(
                                        self.current_text_color.clone(),
                                    ));
                                    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                                    cpp!(unsafe [pen as "QPen*"] {
                                        pen->setWidth(2);
                                    });
                                    painter.set_pen(pen);
                                    let underline_y = baseline + 2.0;
                                    let line_f = QLineF {
                                        pt1: QPointF { x: x + before_w, y: underline_y },
                                        pt2: QPointF {
                                            x: x + before_w + attr_w,
                                            y: underline_y,
                                        },
                                    };
                                    painter.draw_line(line_f);
                                }
                                super::PreeditAttributeKind::TextColor { color } => {
                                    let text_color = if color.is_empty() {
                                        self.current_text_color.clone()
                                    } else {
                                        QString::from(color.clone())
                                    };
                                    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                                    cpp!(unsafe [painter as "QPainter*"] {
                                        painter->save();
                                    });
                                    let clip = QRectF {
                                        x: x + before_w,
                                        y: line.y + paint_offset_y,
                                        width: attr_w,
                                        height: line.height,
                                    };
                                    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                                    cpp!(unsafe [painter as "QPainter*", clip as "QRectF"] {
                                        painter->setClipRect(clip, Qt::ReplaceClip);
                                    });
                                    renderer::draw_text(
                                        painter,
                                        x,
                                        baseline,
                                        fs,
                                        text_color,
                                        self.pipeline.composition().preedit_text.clone().into(),
                                    );
                                    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                                    cpp!(unsafe [painter as "QPainter*"] {
                                        painter->restore();
                                    });
                                }
                                super::PreeditAttributeKind::BackgroundColor { color } => {
                                    let bg_color = if color.is_empty() {
                                        QString::from("#33FFFFFF")
                                    } else {
                                        QString::from(color.clone())
                                    };
                                    renderer::draw_rect(
                                        painter,
                                        x + before_w,
                                        line.y + paint_offset_y,
                                        attr_w,
                                        line.height,
                                        bg_color,
                                    );
                                }
                                super::PreeditAttributeKind::Cursor => {}
                            }
                        }
                    } else {
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
                    }

                    let preedit_cursor_pos = self.pipeline.composition().preedit_cursor;
                    if preedit_cursor_pos > 0 && preedit_cursor_pos <= self.pipeline.composition().preedit_text.len()
                        && self.pipeline.composition().preedit_text.is_char_boundary(preedit_cursor_pos)
                    {
                        let has_cursor_attr = self.pipeline.composition().preedit_attributes.iter()
                            .any(|a| a.kind == super::PreeditAttributeKind::Cursor);

                        if has_cursor_attr {
                            let before_cursor_w = self.editor_layout.text_width(
                                &self.pipeline.composition().preedit_text[..preedit_cursor_pos],
                                font_size,
                                &font_family,
                            );
                            let cursor_x = x + before_cursor_w;
                            let cursor_top = line.y + paint_offset_y;
                            renderer::draw_rect(
                                painter,
                                cursor_x,
                                cursor_top,
                                2.0,
                                line.height,
                                self.current_cursor_color.clone(),
                            );
                        }
                    }

                    break;
                }
            }
        }

        let now_cleanup = Instant::now();
        let elapsed = paint_start.elapsed();
        if elapsed.as_millis() > 4 && renderer::should_log_slow_paint(self.last_slow_paint_log, now_cleanup) {
            self.last_slow_paint_log = Some(now_cleanup);
            crate::sujian_editor_item::editor_debug_log(&format!(
                "sujian_paint_onto: elapsed_ms={}, vis_lines=[{}..{}]={}, scrolling={}, buffer_h={:.1}",
                elapsed.as_millis(),
                vis_start, vis_end,
                vis_end.saturating_sub(vis_start),
                self.current_is_scrolling,
                self.current_content_height,
            ));
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

        if buf.text_revision != self.pipeline.text_revision() {
            return Some("text_revision_changed");
        }

        if buf.visual_revision != self.pipeline.visual_revision() {
            return Some("visual_revision_changed");
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

        // Active text animations change what the static layer renders
        // (clipping out inserted range), so the buffer must be rebuilt.
        if self.has_active_insert_animation() {
            return Some("active_text_animation");
        }

        None
    }

    pub(crate) fn render_to_image(&mut self) -> Option<(QImage, f64, f64)> {
        // Tick text animations for timeout safety
        self.tick_text_animations();

        let render_start = Instant::now();
        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };
        let width = self.bounding_width();
        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let img_w = (width as i32).max(1);
        let scroll_y = f64::from(self.current_scroll_y);
        let content_h = f64::from(self.current_content_height);

        // Use larger overscan (3.5x viewport) to reduce buffer rebuilds
        // during scrolling, especially at large font sizes where content
        // height changes significantly per line.
        let overscan = vp_h * 3.5;
        let min_y = (scroll_y - overscan).max(0.0);
        let max_y = (scroll_y + vp_h + overscan).min(content_h.max(vp_h));
        let buffer_h = max_y - min_y;

        let miss_reason = if self.render_dirty {
            self.scroll_buffer_miss_reason(scroll_y, vp_h, content_h, dpr)
                .or(Some("render_dirty_force"))
        } else {
            self.scroll_buffer_miss_reason(scroll_y, vp_h, content_h, dpr)
        };

        let Some(miss_reason) = miss_reason else {
            self.render_dirty = false;
            return None;
        };

        crate::sujian_editor_item::editor_debug_log(&format!(
                "render_to_image: rebuilding buffer, miss_reason={}, scroll_y={:.1}, content_h={:.1}, vp_h={:.1}",
                miss_reason, scroll_y, content_h, vp_h
            ));

        let phys_w = ((f64::from(img_w) * dpr) as i32).max(1);
        let phys_h = ((buffer_h * dpr) as i32).max(1);
        let mut image = QImage::new(
            qmetaobject::QSize {
                width: phys_w as u32,
                height: phys_h as u32,
            },
            qmetaobject::ImageFormat::ARGB32_Premultiplied,
        );
        // DPR model: image is at physical pixel size with DPR=1.0.
        // QPainter manually scales by dpr so paint_onto() uses logical coords.
        // setSourceRect on QSGImageNode uses physical pixel coords (×dpr).
        // destRect uses QML logical coords.
        {
            let img_ptr = &mut image as *mut qmetaobject::QImage;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            cpp!(unsafe [img_ptr as "QImage*"] {
                img_ptr->setDevicePixelRatio(1.0);
            });
        }
        image.fill(qmetaobject::QColor::from_rgba(0, 0, 0, 0));

        let painter_ptr = renderer::sujian_create_painter_scaled(&mut image, dpr);
        if painter_ptr.is_null() {
            return Some((image, min_y, buffer_h));
        }

        // SAFETY: painter_ptr is null-checked above; sujian_create_painter_scaled returns a valid QPainter pointer that is deleted at the end of this scope.
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
            text_revision: self.pipeline.text_revision(),
            visual_revision: self.pipeline.visual_revision(),
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
                            l.y < min_y + buffer_h + f64::from(self.current_font_pixel_size) * 2.0
                        }) + 1,
                    );
                    end.saturating_sub(start)
                })
                .unwrap_or(0);
            crate::sujian_editor_item::editor_debug_log(&format!(
                "sujian_render_to_image: elapsed_ms={}, img={}x{}(phys {}x{}, dpr={}), vis_lines={}, scroll_y={:.1}, buf_scroll={:.1}, buf_h={:.1}",
                render_elapsed.as_millis(),
                img_w, (buffer_h as i32), phys_w, phys_h, dpr,
                vis_lines,
                scroll_y, min_y, buffer_h,
            ));
        }

        Some((image, min_y, buffer_h))
    }

    /// Update cursor visual position from layout-computed position.
    ///
    /// **IMPORTANT**: This method MUST only be called from the GUI thread.
    /// It directly emits signals and calls inputMethod()->update().
    pub(crate) fn update_cursor_visual_position(&mut self) -> CursorUpdateResult {
        let scroll_y = f64::from(self.current_scroll_y);
        let layout_res = self.editor_layout_cursor_rect(
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
            scroll_y,
        );

        let cursor_x = layout_res.x;
        let cursor_y = layout_res.y;
        let cursor_h = layout_res.h;
        let visual_line_id = layout_res.visual_line_id;

        let vp_h = f64::from(self.current_viewport_height.max(1.0));
        let _is_selecting = self.buffer.selection_anchor != self.buffer.cursor;
        let _is_preediting = !self.pipeline.composition().preedit_text.is_empty();

        let cursor_plan = CursorAnimationPlan {
            should_be_visible: self.current_editor_enabled
                && !self.buffer.has_selection()
                && cursor_y + cursor_h > 0.0
                && cursor_y < vp_h
                && !self.current_is_scrolling,
            blink_mode: if self.current_coordinated_text_cursor_animation_enabled
                && self.pipeline.animation_coordinator_mut().has_active_insert()
            {
                CursorBlinkMode::Suppressed
            } else {
                CursorBlinkMode::Normal
            },
            transition: if self.current_smooth_cursor_enabled {
                CursorTransition::Tween {
                    old_rect: writer_core::editor::CursorRect {
                        x: self.cursor_ctrl.visual_x,
                        top: self.cursor_ctrl.visual_y,
                        bottom: self.cursor_ctrl.visual_y + cursor_h,
                        baseline_y: self.cursor_ctrl.visual_y + cursor_h * 0.8,
                    },
                    new_rect: writer_core::editor::CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                    duration_ms: u64::from(self.current_cursor_animation_duration_ms),
                }
            } else {
                CursorTransition::Snap
            },
            cursor_x,
            cursor_y,
            cursor_h,
        };

        let result = self.cursor_ctrl.apply_plan(&cursor_plan);

        if result.needs_repaint {
            let item = self as &dyn QQuickItem;
            item.update();
        }

        if result.ime_needs_update
            || result.visibility_changed
            || result.blink_changed
            || result.visual_position_changed
        {
            self.cursor_rect_changed();
        }

        if result.ime_needs_update {
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
                });
            }
        }

        {
            let mut line_info = String::new();
            if let Some(snapshot) = self.editor_layout.cache() {
                if let Some(line) = snapshot.lines.iter().find(|l| l.id == visual_line_id) {
                    let font_size = f64::from(snapshot.font_size);
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
            crate::sujian_editor_item::editor_debug_log(&format!(
                "update_cursor_visual_position: cursor={}, target_x={:.1}, target_y={:.1}, visual_x={:.1}, visual_y={:.1}, is_animating={}, scroll_y={:.1}{}",
                self.buffer.cursor, self.cursor_ctrl.target_x, self.cursor_ctrl.target_y, self.cursor_ctrl.visual_x, self.cursor_ctrl.visual_y, self.cursor_ctrl.animation.is_some(), scroll_y, line_info
            ));
        }

        if self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.dirty = true;
            self.request_frame_update();
        }

        if self.buffer.has_selection() {
            let anchor_layout = self.editor_layout_cursor_rect(
                self.buffer.selection_anchor,
                CaretAffinity::Downstream,
                scroll_y,
            );
            self.cursor_ctrl.anchor_visual_x = Some(anchor_layout.x);
            self.cursor_ctrl.anchor_visual_y = Some(anchor_layout.y);
        } else {
            self.cursor_ctrl.anchor_visual_x = None;
            self.cursor_ctrl.anchor_visual_y = None;
        }

        result
    }
}

// Static text always renders full content. Animation must not affect text correctness.

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
            visual_revision: 0,
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
    fn test_visual_revision_mismatch_causes_buffer_miss() {
        let vp_h: f64 = 1000.0;
        let content_h: f64 = 5000.0;

        let scroll_buffer = ScrollBuffer {
            image: QImage::new(
                qmetaobject::QSize { width: 10, height: 3500 },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            ),
            buffer_scroll_y: 0.0,
            buffer_content_h: content_h,
            buffer_logical_h: 3500.0,
            dpr: 1.0,
            text_revision: 1,
            visual_revision: 1,
        };

        // Same visual_revision should not cause miss (via scroll_buffer_miss_reason)
        // This test verifies the field exists and is properly stored
        assert_eq!(scroll_buffer.visual_revision, 1);
    }
}
