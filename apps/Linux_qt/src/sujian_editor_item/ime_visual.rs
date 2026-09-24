use super::*;

impl SujianEditorItem {
    pub(crate) fn update_preedit_visual_state(&mut self) {
        if self.pipeline.composition().preedit_text.is_empty() {
            self.pipeline.composition_mut().preedit_cursor_rect = None;
            return;
        }

        self.compute_preedit_cursor_rect();

        let old_text = self.pipeline.composition().preedit_old_text.clone();
        let new_text = self.pipeline.composition().preedit_text.clone();

        if old_text == new_text {
            self.request_static_repaint();
            return;
        }

        // Issue #735: preedit_visual_transaction 字段已删除（Core 已删除
        // PreeditVisualTransaction / VisualCoordinateMode）。
        // preedit 视觉状态由 preedit_cursor_rect + layout snapshot 直接渲染，
        // 不再构造跨平台视觉事务 DTO。
        editor_animation_debug_log(&format!(
            "[preedit] preedit_visual_state_updated old_len={} new_len={}",
            old_text.len(),
            new_text.len()
        ));
    }

    pub(crate) fn compute_preedit_cursor_rect(&mut self) {
        if self.pipeline.composition().preedit_text.is_empty() {
            self.pipeline.composition_mut().preedit_cursor_rect = None;
            return;
        }

        let width = self.bounding_width();
        let font_size = f64::from(self.current_font_pixel_size);
        let font_family = &self.current_font_family.to_string();
        let scroll_y = f64::from(self.current_scroll_y);

        let cursor_byte = self.pipeline.cursor();
        let snapshot = self.layout_snapshot(width);

        let cursor_line = snapshot
            .lines
            .iter()
            .find(|l| l.byte_end >= cursor_byte && l.byte_start <= cursor_byte);

        let Some(line) = cursor_line else {
            self.pipeline.composition_mut().preedit_cursor_rect = None;
            return;
        };

        let preedit_start_x = self.editor_layout.cursor_x_for_line(
            &snapshot,
            line,
            cursor_byte,
            self.cursor_ctrl.affinity,
        );

        let preedit_cursor = self.pipeline.composition().preedit_cursor;
        let preedit_text = self.pipeline.composition().preedit_text.clone();
        let preedit_before_cursor =
            if preedit_text.is_char_boundary(preedit_cursor.min(preedit_text.len())) {
                &preedit_text[..preedit_cursor.min(preedit_text.len())]
            } else {
                ""
            };
        let preedit_cursor_offset =
            self.editor_layout
                .text_width(preedit_before_cursor, font_size, font_family);

        let cursor_x = preedit_start_x + preedit_cursor_offset;
        let cursor_y = line.y - scroll_y;
        let cursor_h = line.height;

        let baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;

        self.pipeline.composition_mut().preedit_cursor_rect = Some(CursorRect {
            x: cursor_x,
            top: cursor_y,
            bottom: cursor_y + cursor_h,
            baseline_y,
        });
    }

    pub(crate) fn update_ime_cursor_for_preedit(&mut self) {
        if self.pipeline.composition().preedit_cursor_rect.is_some() {
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                });
            }
        }
    }
}
