use super::*;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::line_snapshot_builder::LineSnapshotBuilder;
use crate::editor::layout;

impl SujianEditorItem {
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) -> Option<EditorVisualTransaction> {
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
            if let Some(ref mut vt) = vt {
                let width = self.bounding_width();
                let font_size = self.current_font_pixel_size as f64;
                let font_family = &self.current_font_family.to_string();
                let scroll_y = self.current_scroll_y as f64;
                let viewport_h = self.current_viewport_height.max(1.0) as f64;
                let text_indent = self.current_text_indent as f64;
                let line_spacing = self.current_line_spacing as f64;
                let padding = self.current_padding as f64;
                let dpr = {
                    let item_ptr = self.get_cpp_object();
                    if !item_ptr.is_null() {
                        crate::editor::renderer::sujian_item_dpr(item_ptr)
                    } else {
                        1.0
                    }
                };
                let text_color = &self.current_text_color.to_string();

                let (affected_byte_start, affected_byte_end) = vt.inserted_range
                    .or(vt.deleted_range)
                    .unwrap_or_else(|| {
                        let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                        let mut min_b = usize::MAX;
                        let mut max_b = 0usize;
                        for change in &changes {
                            match change {
                                writer_core::editor::EditorChange::Insert { index, text } => {
                                    min_b = min_b.min(*index);
                                    max_b = (*index + text.len()).max(max_b);
                                }
                                writer_core::editor::EditorChange::Delete { index, text } => {
                                    min_b = min_b.min(*index);
                                    max_b = (*index + text.len()).max(max_b);
                                }
                                _ => {}
                            }
                        }
                        (min_b.min(max_b), max_b)
                    });

                let prev_new_snapshot = self.previous_canonical_snapshot.as_ref();

                let old_doc_snapshot = layout::prepare_affected_paragraphs_visual_snapshot(
                    &vt.old_text,
                    0,
                    font_size,
                    font_family,
                    line_spacing,
                    padding,
                    text_indent,
                    width,
                    dpr,
                    text_color,
                    affected_byte_start,
                    affected_byte_end,
                    prev_new_snapshot,
                );
                let new_doc_snapshot = layout::prepare_affected_paragraphs_visual_snapshot(
                    &new.text,
                    0,
                    font_size,
                    font_family,
                    line_spacing,
                    padding,
                    text_indent,
                    width,
                    dpr,
                    text_color,
                    affected_byte_start,
                    affected_byte_end,
                    prev_new_snapshot,
                );

                let old_caret = old_doc_snapshot.cursor_rect(
                    vt.old_selection.head.index,
                    layout::CaretAffinity::Downstream,
                    scroll_y,
                    viewport_h,
                );
                let new_caret = new_doc_snapshot.cursor_rect(
                    vt.new_selection.head.index,
                    layout::CaretAffinity::Downstream,
                    scroll_y,
                    viewport_h,
                );

                vt.old_cursor_rect = Some(make_cursor_rect_from_caret_doc(&old_caret, &old_doc_snapshot, font_family, scroll_y));
                vt.new_cursor_rect = Some(make_cursor_rect_from_caret_doc(&new_caret, &new_doc_snapshot, font_family, scroll_y));

                match vt.kind {
                    EditorAnimationKind::Insert => {
                        vt.insert_glyph_rects = Some(Vec::new());
                        vt.reflow_glyph_rects = None;
                    }
                    EditorAnimationKind::Delete => {
                        vt.deleted_glyph_rects = None;
                    }
                    EditorAnimationKind::Cursor => {}
                }

                let old_revision = self.layout_revision;
                let new_revision = layout_revision::LayoutRevision::next();

                let (old_snap, new_snap) = LineSnapshotBuilder::build_old_new_from_canonical(
                    &old_doc_snapshot,
                    &new_doc_snapshot,
                    old_revision,
                    new_revision,
                    scroll_y,
                    viewport_h,
                );

                let key = self.pipeline.animation_coordinator_mut().process_transaction(
                    vt,
                    self.current_typing_animation_enabled,
                    self.current_is_scrolling,
                    self.current_is_loading,
                    self.current_is_applying_format,
                    self.current_is_applying_settings,
                    vt.old_cursor_rect.clone(),
                    vt.new_cursor_rect.clone(),
                    &old_snap,
                    &new_snap,
                );
                if let Some(key) = key {
                    self.prepare_transaction_textures(key);
                    self.layout_revision = new_revision;
                }

                self.previous_layout_snapshot = Some(self.current_layout_snapshot.clone().unwrap_or_else(|| {
                    EditorLayoutSnapshot::new(old_doc_snapshot.to_layout_snapshot(), Vec::new(), None, crate::editor::layout::CaretAffinity::Downstream)
                }));
                self.current_layout_snapshot = Some(new_snap);
                self.previous_canonical_snapshot = Some(new_doc_snapshot);

                editor_animation_debug_log(&format!(
                    "record_transaction: processed via canonical document snapshot pipeline, kind={:?}, has_active_insert={}",
                    vt.kind,
                    self.pipeline.animation_coordinator_mut().has_active_insert()
                ));
            }
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
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;

        match vt.kind {
            EditorAnimationKind::Insert => {
                let insert_snapshot = self.layout_snapshot_for_text(text, width);

                if vt.inserted_range.is_some() {
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);
                    let old_caret = self.editor_layout.caret_rect(
                        &old_snapshot,
                        vt.old_selection.head.index,
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
                        vt.new_selection.head.index,
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
                    vt.old_selection.head.index,
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
                    vt.new_selection.head.index,
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
        let tx = self.pipeline.animation_coordinator_mut().prepared_queue.active_transactions()
            .iter()
            .find(|t| t.key == key)
            .cloned();

        match tx {
            Some(t) => {
                let snapshot_ids = t.snapshot_ids();
                if snapshot_ids.is_empty() {
                    self.pipeline.animation_coordinator_mut().prepared_queue.mark_texture_prepared(key);
                    return;
                }

                let mut all_found = true;
                for id in &snapshot_ids {
                    if !self.pipeline.texture_cache().contains_line(id) {
                        all_found = false;
                        break;
                    }
                }

                if all_found {
                    self.pipeline.animation_coordinator_mut().prepared_queue.mark_texture_prepared(key);
                    return;
                }

                if let Some(ref old_snap) = t.old_snapshot {
                    for line in &old_snap.line_snapshots {
                        if let Some(ref image) = line.image {
                            self.pipeline.texture_cache_mut().insert_line(line.id, image.clone());
                        }
                    }
                }
                if let Some(ref new_snap) = t.new_snapshot {
                    for line in &new_snap.line_snapshots {
                        if let Some(ref image) = line.image {
                            self.pipeline.texture_cache_mut().insert_line(line.id, image.clone());
                        }
                    }
                }

                let mut any_missing = false;
                for id in &snapshot_ids {
                    if !self.pipeline.texture_cache().contains_line(id) {
                        any_missing = true;
                        break;
                    }
                }

                if any_missing {
                    editor_animation_debug_log(&format!(
                        "prepare_transaction_textures: some line textures missing for tid={}, cancelling",
                        key.transaction_id
                    ));
                    self.pipeline.animation_coordinator_mut().cancel_by_key(key, "texture_failed");
                    self.render_dirty = true;
                } else {
                    self.pipeline.animation_coordinator_mut().prepared_queue.mark_texture_prepared(key);
                }
            }
            None => {}
        }
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
        Some(l) => layout::text_baseline_y(l, snapshot.font_size as f64, font_family) - scroll_y,
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}

fn make_cursor_rect_from_caret_doc(
    caret: &CursorLayoutRect,
    doc_snapshot: &layout::CanonicalDocumentVisualSnapshot,
    font_family: &str,
    scroll_y: f64,
) -> CursorRect {
    let line = doc_snapshot.visual_lines.iter().find(|l| l.id == caret.visual_line_id);
    let baseline_y = match line {
        Some(l) => layout::text_baseline_y(l, doc_snapshot.font_size, font_family) - scroll_y,
        None => caret.y + caret.h * 0.8,
    };
    CursorRect {
        x: caret.x,
        top: caret.y,
        bottom: caret.y + caret.h,
        baseline_y,
    }
}
