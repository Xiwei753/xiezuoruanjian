use super::*;

impl SujianEditorItem {
    pub(crate) fn recalculate_content_height_and_emit(&mut self) {
        let next = self.compute_content_height();
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_dirty.set(false);
            self.content_height_changed();
        }
    }

    pub(crate) fn compute_content_height(&mut self) -> f32 {
        let width = self.bounding_width();
        let padding = self.current_padding;
        let font_size = self.current_font_pixel_size;
        let line_spacing = self.current_line_spacing;
        let lines = self.ensure_layout_cached(width);
        let height = lines
            .last()
            .map(|line| line.y + line.height + padding as f64)
            .unwrap_or((font_size * line_spacing + padding * 2.0) as f64);
        height.max(1.0) as f32
    }

    pub(crate) fn invalidate_layout_cache(&mut self) {
        self.editor_layout.invalidate();
        self.scroll_buffer = None;
    }

    pub(crate) fn layout_params(&self, width: f64) -> LayoutParams {
        LayoutParams {
            width,
            font_size: self.current_font_pixel_size,
            font_family: self.current_font_family.to_string(),
            line_spacing: self.current_line_spacing,
            text_indent: self.current_text_indent,
            padding: self.current_padding,
        }
    }

    pub(crate) fn layout_snapshot(&mut self, width: f64) -> LayoutSnapshot {
        let params = self.layout_params(width);
        self.editor_layout
            .snapshot(&self.buffer.text, params, self.text_revision)
            .clone()
    }

    pub(crate) fn layout_snapshot_for_text(&mut self, text: &str, width: f64) -> LayoutSnapshot {
        let params = self.layout_params(width);
        self.editor_layout.snapshot(text, params, 0).clone()
    }

    pub(crate) fn build_editor_layout_snapshot(&mut self, width: f64) -> EditorLayoutSnapshot {
        let layout_snap = self.layout_snapshot(width);
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;
        let caret = self.editor_layout.caret_rect(
            &layout_snap,
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
            scroll_y,
            viewport_h,
        );
        EditorLayoutSnapshot::new(
            layout_snap,
            &self.buffer.text,
            Some(caret),
            self.cursor_ctrl.affinity,
        )
    }

    pub(crate) fn build_editor_layout_snapshot_for_text(
        &mut self,
        text: &str,
        width: f64,
    ) -> EditorLayoutSnapshot {
        let layout_snap = self.layout_snapshot_for_text(text, width);
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;
        EditorLayoutSnapshot::new(
            layout_snap,
            text,
            None,
            CaretAffinity::Downstream,
        )
    }

    pub(crate) fn update_current_layout_snapshot(&mut self) {
        let width = self.bounding_width();
        let new_snapshot = self.build_editor_layout_snapshot(width);
        self.previous_layout_snapshot = self.current_layout_snapshot.take();
        self.current_layout_snapshot = Some(new_snapshot);
        self.layout_revision = layout_snapshot::LayoutRevision::new();
    }

    pub(crate) fn ensure_layout_cached(&mut self, width: f64) -> &Vec<VisualLine> {
        let params = self.layout_params(width);
        &self
            .editor_layout
            .snapshot(&self.buffer.text, params, self.text_revision)
            .lines
    }

    pub(crate) fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let cursor = self.buffer.cursor;

        let is_wrap_boundary = lines.iter().enumerate().any(|(idx, line)| {
            idx + 1 < lines.len() && line.byte_end == cursor && lines[idx + 1].byte_start == cursor
        });

        if is_wrap_boundary {
            self.cursor_ctrl.affinity = CaretAffinity::Upstream;
        } else {
            self.cursor_ctrl.affinity = CaretAffinity::Downstream;
        }
    }

    pub(crate) fn editor_layout_cursor_rect(
        &mut self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
    ) -> CursorLayoutRect {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        self.editor_layout.caret_rect(
            &snapshot,
            cursor_byte,
            affinity,
            scroll_y,
            self.current_viewport_height.max(1.0) as f64,
        )
    }

    pub(crate) fn hit_test(&mut self, x: f64, y: f64) -> (usize, CaretAffinity) {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        let scroll_y = self.current_scroll_y as f64;
        let (index, affinity) = self.editor_layout.hit_test(&snapshot, x, y, scroll_y);
        editor_debug_log(&format!(
            "hit_test: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, clamped_index={}, affinity={:?}",
            x, y, self.current_scroll_y, index, affinity
        ));
        (index, affinity)
    }

    pub(crate) fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let Some(snapshot) = self.editor_layout.cache() else {
            return line.byte_start;
        };
        self.editor_layout.index_at_line_x(snapshot, line, x)
    }

    pub(crate) fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        let Some(snapshot) = self.editor_layout.cache() else {
            return None;
        };
        debug_assert_eq!(lines.len(), snapshot.lines.len());
        self.editor_layout.cursor_line_and_x(
            snapshot,
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
        )
    }
}
