use super::*;

impl SujianEditorItem {
    pub(crate) fn update_preedit_visual_state(&mut self) {
        if self.preedit_text.is_empty() {
            self.preedit_visual_transaction = None;
            self.preedit_cursor_rect = None;
            self.last_preedit_visual_transaction_json = "".into();
            return;
        }

        self.compute_preedit_cursor_rect();

        let old_text = self.preedit_old_text.clone();
        let new_text = self.preedit_text.clone();

        if old_text == new_text {
            self.preedit_visual_transaction_changed();
            self.request_static_repaint();
            return;
        }

        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;

        let cursor_byte = self.buffer.cursor;
        let snapshot = self.layout_snapshot(width);

        let cursor_line = snapshot
            .lines
            .iter()
            .find(|l| l.byte_end >= cursor_byte && l.byte_start <= cursor_byte);

        let (preedit_start_x, line_y, line_h, line_baseline_y) = if let Some(line) = cursor_line {
            let start_x = self.editor_layout.cursor_x_for_line(
                &snapshot,
                line,
                cursor_byte,
                self.cursor_ctrl.affinity,
            );
            let baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;
            (start_x, line.y - scroll_y, line.height, baseline_y)
        } else {
            (0.0, 0.0, font_size, font_size * 0.8)
        };

        let preedit_glyph_rects = {
            let mut rects = Vec::new();
            let mut cum_x = preedit_start_x;
            let mut byte_offset = 0usize;
            for ch in new_text.chars() {
                if is_complex_grapheme(ch) {
                    let ch_str = ch.to_string();
                    let ch_w = self
                        .editor_layout
                        .text_width(&ch_str, font_size, font_family);
                    cum_x += ch_w;
                    byte_offset += ch.len_utf8();
                    continue;
                }
                let ch_str = ch.to_string();
                let ch_w = self
                    .editor_layout
                    .text_width(&ch_str, font_size, font_family);
                let bs = byte_offset;
                let be = byte_offset + ch.len_utf8();
                rects.push(GlyphRect {
                    x: cum_x,
                    y: line_y,
                    w: ch_w,
                    h: line_h,
                    char_: ch_str,
                    baseline_y: line_baseline_y,
                    byte_start: bs,
                    byte_end: be,
                });
                cum_x += ch_w;
                byte_offset = be;
            }
            rects
        };

        let old_chars: Vec<char> = old_text.chars().collect();
        let new_chars: Vec<char> = new_text.chars().collect();
        let prefix_len = old_chars
            .iter()
            .zip(new_chars.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let suffix_len = old_chars[prefix_len..]
            .iter()
            .rev()
            .zip(new_chars[prefix_len..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();

        let inserted_preedit_glyph_rects = {
            let mut rects = Vec::new();
            let mut cum_x = preedit_start_x;
            let mut byte_offset = 0usize;
            for ch in new_chars[..prefix_len].iter() {
                let ch_str = ch.to_string();
                byte_offset += ch.len_utf8();
                cum_x += self
                    .editor_layout
                    .text_width(&ch_str, font_size, font_family);
            }
            for ch in new_chars[prefix_len..new_chars.len().saturating_sub(suffix_len)].iter() {
                if is_complex_grapheme(*ch) {
                    let ch_str = ch.to_string();
                    let ch_w = self
                        .editor_layout
                        .text_width(&ch_str, font_size, font_family);
                    cum_x += ch_w;
                    byte_offset += ch.len_utf8();
                    continue;
                }
                let ch_str = ch.to_string();
                let ch_w = self
                    .editor_layout
                    .text_width(&ch_str, font_size, font_family);
                let bs = byte_offset;
                let be = byte_offset + ch.len_utf8();
                rects.push(GlyphRect {
                    x: cum_x,
                    y: line_y,
                    w: ch_w,
                    h: line_h,
                    char_: ch_str,
                    baseline_y: line_baseline_y,
                    byte_start: bs,
                    byte_end: be,
                });
                cum_x += ch_w;
                byte_offset = be;
            }
            if rects.is_empty() {
                None
            } else {
                Some(rects)
            }
        };

        let deleted_preedit_glyph_rects = {
            let mut rects = Vec::new();
            let mut cum_x = preedit_start_x;
            let mut byte_offset = 0usize;
            for ch in old_chars[..prefix_len].iter() {
                let ch_str = ch.to_string();
                byte_offset += ch.len_utf8();
                cum_x += self
                    .editor_layout
                    .text_width(&ch_str, font_size, font_family);
            }
            for ch in old_chars[prefix_len..old_chars.len().saturating_sub(suffix_len)].iter() {
                if is_complex_grapheme(*ch) {
                    let ch_str = ch.to_string();
                    let ch_w = self
                        .editor_layout
                        .text_width(&ch_str, font_size, font_family);
                    cum_x += ch_w;
                    byte_offset += ch.len_utf8();
                    continue;
                }
                let ch_str = ch.to_string();
                let ch_w = self
                    .editor_layout
                    .text_width(&ch_str, font_size, font_family);
                let bs = byte_offset;
                let be = byte_offset + ch.len_utf8();
                rects.push(GlyphRect {
                    x: cum_x,
                    y: line_y,
                    w: ch_w,
                    h: line_h,
                    char_: ch_str,
                    baseline_y: line_baseline_y,
                    byte_start: bs,
                    byte_end: be,
                });
                cum_x += ch_w;
                byte_offset = be;
            }
            if rects.is_empty() {
                None
            } else {
                Some(rects)
            }
        };

        let vt = PreeditVisualTransaction {
            id: 0,
            old_preedit_text: old_text.clone(),
            new_preedit_text: new_text.clone(),
            old_preedit_cursor_rect: None,
            new_preedit_cursor_rect: self.preedit_cursor_rect.clone(),
            preedit_glyph_rects: Some(preedit_glyph_rects),
            deleted_preedit_glyph_rects,
            inserted_preedit_glyph_rects,
            preedit_cursor_rect: self.preedit_cursor_rect.clone(),
            duration_ms: self.current_typing_animation_duration_ms as u64,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };

        self.preedit_visual_transaction = Some(vt);
        editor_animation_debug_log(&format!(
            "[preedit] preedit_visual_transaction_created old_len={} new_len={}",
            old_text.len(),
            new_text.len()
        ));

        if let Some(ref vt) = self.preedit_visual_transaction {
            match serde_json::to_string(vt) {
                Ok(json) => {
                    self.last_preedit_visual_transaction_json = json.into();
                }
                Err(e) => {
                    editor_animation_debug_log(&format!(
                        "update_preedit_visual_state: failed to serialize preedit visual transaction: {}",
                        e
                    ));
                    self.last_preedit_visual_transaction_json = "{}".into();
                }
            }
            self.preedit_visual_transaction_changed();
        }
    }

    pub(crate) fn compute_preedit_cursor_rect(&mut self) {
        if self.preedit_text.is_empty() {
            self.preedit_cursor_rect = None;
            return;
        }

        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;

        let cursor_byte = self.buffer.cursor;
        let snapshot = self.layout_snapshot(width);

        let cursor_line = snapshot
            .lines
            .iter()
            .find(|l| l.byte_end >= cursor_byte && l.byte_start <= cursor_byte);

        let Some(line) = cursor_line else {
            self.preedit_cursor_rect = None;
            return;
        };

        let preedit_start_x = self.editor_layout.cursor_x_for_line(
            &snapshot,
            line,
            cursor_byte,
            self.cursor_ctrl.affinity,
        );

        let preedit_before_cursor =
            &self.preedit_text[..self.preedit_cursor.min(self.preedit_text.len())];
        let preedit_cursor_offset =
            self.editor_layout
                .text_width(preedit_before_cursor, font_size, font_family);

        let cursor_x = preedit_start_x + preedit_cursor_offset;
        let cursor_y = line.y - scroll_y;
        let cursor_h = line.height;

        let baseline_y = text_baseline_y(line, font_size, font_family) - scroll_y;

        self.preedit_cursor_rect = Some(CursorRect {
            x: cursor_x,
            top: cursor_y,
            bottom: cursor_y + cursor_h,
            baseline_y,
        });
    }

    pub(crate) fn update_ime_cursor_for_preedit(&mut self) {
        if self.preedit_cursor_rect.is_some() {
            self.cursor_rect_changed();
            let obj_ptr = self.get_cpp_object();
            if !obj_ptr.is_null() {
                cpp!(unsafe [obj_ptr as "QQuickItem*"] {
                    QGuiApplication::inputMethod()->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle);
                });
            }
        }
    }
}
