use super::*;
use crate::editor::layout;

impl SujianEditorItem {
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) -> Option<EditorVisualTransaction> {
        let ctx = pipeline::VisualTransactionContext {
            typing_animation_enabled: self.current_typing_animation_enabled,
            is_scrolling: self.current_is_scrolling,
            is_loading: self.current_is_loading,
            is_applying_format: self.current_is_applying_format,
            is_applying_settings: self.current_is_applying_settings,
            bounding_width: self.bounding_width(),
            font_pixel_size: f64::from(self.current_font_pixel_size),
            font_family: self.current_font_family.to_string(),
            scroll_y: f64::from(self.current_scroll_y),
            viewport_height: f64::from(self.current_viewport_height.max(1.0)),
            text_indent: f64::from(self.current_text_indent),
            line_spacing: f64::from(self.current_line_spacing),
            padding: f64::from(self.current_padding),
            text_color: self.current_text_color.to_string(),
            dpr: {
                let item_ptr = self.get_cpp_object();
                if !item_ptr.is_null() {
                    crate::editor::renderer::sujian_item_dpr(item_ptr)
                } else {
                    1.0
                }
            },
        };

        let transaction = self.pipeline.engine().create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        let mut vt = self.pipeline.engine_mut().visual_transaction(&transaction);

        if self.current_typing_animation_enabled && vt.is_some() && !self.current_is_scrolling {
            vt = self.pipeline.record_visual_transaction(&ctx, &old, &new, cause);
        } else {
            if let Some(ref mut vt) = vt {
                self.fill_visual_transaction_coords_legacy(vt, &new.text, &old.text);
            }
        }

        self.last_event_count = if vt.is_some() { 1 } else { 0 };
        self.last_summary = format!(
            "cause={:?};changes={};vt={};animate={}",
            transaction.cause,
            transaction.changes.len(),
            vt.is_some(),
            transaction.should_animate
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_transaction: cause={:?}, changes={}, vt={}, animate={}, typing_anim_enabled={}, is_scrolling={}",
            transaction.cause,
            transaction.changes.len(),
            vt.is_some(),
            transaction.should_animate,
            self.current_typing_animation_enabled,
            self.current_is_scrolling,
        ));
        if emit {
            self.transaction_created();
        }
        vt
    }

    pub(crate) fn fill_visual_transaction_coords_legacy(
        &mut self,
        vt: &mut EditorVisualTransaction,
        text: &str,
        old_text: &str,
    ) {
        let width = self.bounding_width();
        let _font_size = f64::from(self.current_font_pixel_size);
        let font_family = &self.current_font_family.to_string();
        let scroll_y = f64::from(self.current_scroll_y);
        let viewport_h = f64::from(self.current_viewport_height.max(1.0));

        match vt.kind {
            EditorAnimationKind::Insert => {
                let insert_snapshot = self.layout_snapshot_for_text(text, width);

                if vt.inserted_range.is_some() {
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);
                    let old_caret = self.editor_layout.caret_rect(
                        &old_snapshot,
                        vt.old_selection.head.index.value(),
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    vt.old_cursor_rect = Some(make_cursor_rect_from_caret(
                        &old_caret,
                        &old_snapshot,
                        font_family,
                        scroll_y,
                    ));

                    let new_caret = self.editor_layout.caret_rect(
                        &insert_snapshot,
                        vt.new_selection.head.index.value(),
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    vt.new_cursor_rect = Some(make_cursor_rect_from_caret(
                        &new_caret,
                        &insert_snapshot,
                        font_family,
                        scroll_y,
                    ));
                }

                vt.insert_glyph_rects = Some(Vec::new());
                vt.reflow_glyph_rects = None;
            }
            EditorAnimationKind::Delete => {
                let delete_snapshot = self.layout_snapshot_for_text(old_text, width);

                let old_caret = self.editor_layout.caret_rect(
                    &delete_snapshot,
                    vt.old_selection.head.index.value(),
                    CaretAffinity::Downstream,
                    scroll_y,
                    viewport_h,
                );
                vt.old_cursor_rect = Some(make_cursor_rect_from_caret(
                    &old_caret,
                    &delete_snapshot,
                    font_family,
                    scroll_y,
                ));

                let new_snapshot = self.layout_snapshot_for_text(text, width);
                let new_caret = self.editor_layout.caret_rect(
                    &new_snapshot,
                    vt.new_selection.head.index.value(),
                    CaretAffinity::Downstream,
                    scroll_y,
                    viewport_h,
                );
                vt.new_cursor_rect = Some(make_cursor_rect_from_caret(
                    &new_caret,
                    &new_snapshot,
                    font_family,
                    scroll_y,
                ));

                vt.deleted_glyph_rects = None;
            }
            EditorAnimationKind::Cursor => {}
        }
    }

    pub(crate) fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        self.pipeline.prepare_transaction_textures(key);
        self.render_dirty = true;
    }
}

fn make_cursor_rect_from_caret(
    caret: &CursorLayoutRect,
    snapshot: &LayoutSnapshot,
    font_family: &str,
    scroll_y: f64,
) -> CursorRect {
    let line = snapshot.lines.iter().find(|l| l.id == caret.visual_line_id);
    let baseline_y = match line {
        Some(l) => layout::text_baseline_y(l, f64::from(snapshot.font_size), font_family) - scroll_y,
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}
