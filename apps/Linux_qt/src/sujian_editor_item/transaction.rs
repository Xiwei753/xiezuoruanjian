use super::*;

impl SujianEditorItem {
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) -> Option<EditorVisualTransaction> {
        let transaction = self.engine.create_transaction(
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
        let mut vt = self.engine.visual_transaction(&transaction);

        if let Some(ref mut vt) = vt {
            self.fill_visual_transaction_coords(vt, &new.text, &old.text);
        }

        if self.current_typing_animation_enabled && vt.is_some() && !self.current_is_scrolling {
            if let Some(ref vt) = vt {
                let (_old_layout_runs, _new_layout_runs, insert_runs, reflow_old_runs, reflow_new_runs) =
                    self.extract_shaped_runs_for_transaction(vt);
                let key = self.animation_coordinator.process_transaction(
                    vt,
                    self.current_typing_animation_enabled,
                    self.current_is_scrolling,
                    self.current_is_loading,
                    self.current_is_applying_format,
                    self.current_is_applying_settings,
                    vt.old_cursor_rect.clone(),
                    vt.new_cursor_rect.clone(),
                    &_old_layout_runs,
                    &_new_layout_runs,
                    &insert_runs,
                    &reflow_old_runs,
                    &reflow_new_runs,
                );
                if let Some(key) = key {
                    self.prepare_transaction_textures(key);
                }
                editor_animation_debug_log(&format!(
                    "record_transaction: processed via prepared queue, kind={:?}, has_active_insert={}",
                    vt.kind,
                    self.animation_coordinator.has_active_insert()
                ));
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
        if let Some(ref vt) = vt {
            let _ = vt;
            self.last_visual_transaction_json = "{}".into();
        } else {
            self.last_visual_transaction_json = "{}".into();
        }
        if emit {
            self.transaction_created();
            if vt.is_some() {
                self.visual_transaction_changed();
            }
        }
        vt
    }

    pub(crate) fn extract_shaped_runs_for_transaction(
        &mut self,
        vt: &EditorVisualTransaction,
    ) -> (
        Vec<super::shaped_visual_run::ShapedVisualRun>,
        Vec<super::shaped_visual_run::ShapedVisualRun>,
        Vec<super::shaped_visual_run::ShapedVisualRun>,
        Vec<super::shaped_visual_run::ShapedVisualRun>,
        Vec<super::shaped_visual_run::ShapedVisualRun>,
    ) {
        use super::shaped_visual_run::{ShapedVisualRun, ShapedGlyph, RawFontCacheKey, RunFlags, derive_clusters_from_glyphs};
        use crate::editor::layout::{extract_shaped_runs_on_line, ShapedRunData};

        let width = self.bounding_width();
        let font_size = self.current_font_pixel_size as f64;
        let font_family = &self.current_font_family.to_string();
        let scroll_y = self.current_scroll_y as f64;
        let viewport_h = self.current_viewport_height.max(1.0) as f64;

        let mut old_layout_runs = Vec::new();
        let mut new_layout_runs = Vec::new();
        let mut insert_runs = Vec::new();
        let mut reflow_old_runs = Vec::new();
        let mut reflow_new_runs = Vec::new();

        fn convert_shaped_run_data(srd: &ShapedRunData, para_text: &str, qtextline_idx: i32, wrap_w: f64, indent: f64) -> ShapedVisualRun {
            let font_key = RawFontCacheKey::new(
                &srd.raw_font_family,
                &srd.raw_font_style,
                srd.raw_font_weight,
                srd.raw_font_pixel_size,
            );
            let mut flags = RunFlags::empty();
            if srd.is_rtl { flags |= RunFlags::RTL; }
            if srd.has_underline { flags |= RunFlags::UNDERLINE; }
            let glyphs: Vec<ShapedGlyph> = srd.glyphs.iter().map(|g| ShapedGlyph {
                glyph_index: g.glyph_index,
                glyph_position_x: g.position_x,
                glyph_position_y: g.position_y,
                string_index: g.string_index.max(0) as usize,
                advance_width: g.advance_width,
            }).collect();
            let clusters = derive_clusters_from_glyphs(&glyphs);
            ShapedVisualRun {
                glyphs,
                clusters,
                raw_font_key: font_key,
                flags,
                source_string_start: srd.string_start,
                source_string_end: srd.string_end,
                baseline_y: srd.baseline_y,
                visual_x: srd.visual_x,
                visual_y: srd.visual_y,
                visual_w: srd.visual_w,
                visual_h: srd.visual_h,
                texture_atlas_x: 0.0,
                texture_atlas_y: 0.0,
                texture_atlas_w: srd.visual_w,
                texture_atlas_h: srd.visual_h,
                texture_translate_x: srd.texture_translate_x,
                texture_translate_y: srd.texture_translate_y,
                qglyphrun_index: srd.run_index,
                para_text: Some(para_text.to_string()),
                qtextline_idx: Some(qtextline_idx),
                paragraph_wrap_w: Some(wrap_w),
                para_indent: Some(indent),
                line_y: srd.line_y,
            }
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let new_snapshot = self.layout_snapshot_for_text(&self.plain_text.to_string(), width);
                    let old_snapshot = self.layout_snapshot_for_text(&vt.old_text, width);

                    for line in &old_snapshot.lines {
                        if line.para_text.is_empty() { continue; }
                        let line_top = line.y - scroll_y;
                        let line_bottom = line_top + line.height;
                        if line_bottom < 0.0 || line_top > viewport_h { continue; }
                        let wrap_w = width - line.x;
                        let indent = self.current_text_indent as f64;
                        let runs = extract_shaped_runs_on_line(
                            &line.para_text, line.byte_start, line.byte_end, line.byte_start,
                            font_size, font_family, wrap_w, indent, line.qtextline_idx,
                        );
                        for srd in &runs {
                            old_layout_runs.push(convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent));
                        }
                    }

                    for line in &new_snapshot.lines {
                        if line.para_text.is_empty() { continue; }
                        let line_top = line.y - scroll_y;
                        let line_bottom = line_top + line.height;
                        if line_bottom < 0.0 || line_top > viewport_h { continue; }
                        let wrap_w = width - line.x;
                        let indent = self.current_text_indent as f64;
                        let runs = extract_shaped_runs_on_line(
                            &line.para_text, line.byte_start, line.byte_end, line.byte_start,
                            font_size, font_family, wrap_w, indent, line.qtextline_idx,
                        );
                        for srd in &runs {
                            let run = convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent);
                            let run_start = run.source_string_start;
                            let run_end = run.source_string_end;

                            if run_start >= range_start && run_end <= range_end {
                                insert_runs.push(run);
                            } else if run_end > range_start && run_start < range_end {
                                insert_runs.push(run);
                            } else {
                                new_layout_runs.push(run);
                            }
                        }
                    }

                    for line in &new_snapshot.lines {
                        if line.para_text.is_empty() { continue; }
                        let line_top = line.y - scroll_y;
                        let line_bottom = line_top + line.height;
                        if line_bottom < 0.0 || line_top > viewport_h { continue; }
                        if line.byte_end <= range_end { continue; }

                        let reflow_start = range_end.max(line.byte_start);
                        let reflow_end = line.byte_end;
                        if reflow_start >= reflow_end { continue; }

                        let wrap_w = width - line.x;
                        let indent = self.current_text_indent as f64;
                        let new_runs = extract_shaped_runs_on_line(
                            &line.para_text, reflow_start, reflow_end, line.byte_start,
                            font_size, font_family, wrap_w, indent, line.qtextline_idx,
                        );
                        for srd in &new_runs {
                            reflow_new_runs.push(convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent));
                        }

                        let old_byte_start = reflow_start.saturating_sub(range_end - range_start);
                        let old_byte_end = reflow_end.saturating_sub(range_end - range_start);
                        let old_line = old_snapshot.lines.iter().find(|l| {
                            l.byte_start <= old_byte_start && l.byte_end >= old_byte_end && !l.para_text.is_empty()
                        });
                        if let Some(ol) = old_line {
                            let old_wrap_w = width - ol.x;
                            let old_indent = self.current_text_indent as f64;
                            let old_runs = extract_shaped_runs_on_line(
                                &ol.para_text, old_byte_start, old_byte_end, ol.byte_start,
                                font_size, font_family, old_wrap_w, old_indent, ol.qtextline_idx,
                            );
                            for srd in &old_runs {
                                reflow_old_runs.push(convert_shaped_run_data(srd, &ol.para_text, ol.qtextline_idx, old_wrap_w, old_indent));
                            }
                        }
                    }
                }
            }
            EditorAnimationKind::Delete => {
                let old_snapshot = self.layout_snapshot_for_text(&vt.old_text, width);
                let new_snapshot = self.layout_snapshot_for_text(&vt.new_text, width);

                for line in &old_snapshot.lines {
                    if line.para_text.is_empty() { continue; }
                    let line_top = line.y - scroll_y;
                    let line_bottom = line_top + line.height;
                    if line_bottom < 0.0 || line_top > viewport_h { continue; }
                    let wrap_w = width - line.x;
                    let indent = self.current_text_indent as f64;
                    let runs = extract_shaped_runs_on_line(
                        &line.para_text, line.byte_start, line.byte_end, line.byte_start,
                        font_size, font_family, wrap_w, indent, line.qtextline_idx,
                    );
                    for srd in &runs {
                        old_layout_runs.push(convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent));
                    }
                }

                for line in &new_snapshot.lines {
                    if line.para_text.is_empty() { continue; }
                    let line_top = line.y - scroll_y;
                    let line_bottom = line_top + line.height;
                    if line_bottom < 0.0 || line_top > viewport_h { continue; }
                    let wrap_w = width - line.x;
                    let indent = self.current_text_indent as f64;
                    let runs = extract_shaped_runs_on_line(
                        &line.para_text, line.byte_start, line.byte_end, line.byte_start,
                        font_size, font_family, wrap_w, indent, line.qtextline_idx,
                    );
                    for srd in &runs {
                        new_layout_runs.push(convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent));
                    }
                }
            }
            EditorAnimationKind::Cursor => {}
        }

        (old_layout_runs, new_layout_runs, insert_runs, reflow_old_runs, reflow_new_runs)
    }

    pub(crate) fn fill_visual_transaction_coords(
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

        fn make_cursor_rect(
            caret: &CursorLayoutRect,
            snapshot: &LayoutSnapshot,
            font_family: &str,
            scroll_y: f64,
        ) -> CursorRect {
            let line = snapshot.lines.iter().find(|l| l.id == caret.visual_line_id);
            let baseline_y = match line {
                Some(l) => text_baseline_y(l, snapshot.font_size as f64, font_family) - scroll_y,
                None => caret.y + caret.h * 0.8,
            };
            CursorRect {
                x: caret.x,
                top: caret.y,
                bottom: caret.y + caret.h,
                baseline_y,
            }
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                let insert_snapshot = self.layout_snapshot_for_text(text, width);
                let mut glyph_rects = Vec::new();

                if let Some((range_start, range_end)) = vt.inserted_range {
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);
                    let old_caret = self.editor_layout.caret_rect(
                        &old_snapshot,
                        vt.old_selection.head.index,
                        CaretAffinity::Downstream,
                        scroll_y,
                        viewport_h,
                    );
                    vt.old_cursor_rect = Some(make_cursor_rect(
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
                    vt.new_cursor_rect = Some(make_cursor_rect(
                        &new_caret,
                        &insert_snapshot,
                        font_family,
                        scroll_y,
                    ));

                    let mut lines_with_insert: Vec<usize> = Vec::new();
                    for (line_idx, line) in insert_snapshot.lines.iter().enumerate() {
                        if line.byte_end <= range_start || line.byte_start >= range_end {
                            continue;
                        }
                        if line.para_text.is_empty() {
                            continue;
                        }
                        let line_top = line.y - scroll_y;
                        let line_bottom = line_top + line.height;
                        if line_bottom < 0.0 || line_top > viewport_h {
                            continue;
                        }
                        lines_with_insert.push(line_idx);
                    }

                    let mut complex_byte_ranges: Vec<(usize, usize)> = Vec::new();

                    for line_idx in &lines_with_insert {
                        let line = &insert_snapshot.lines[*line_idx];
                        let seg_start = range_start.max(line.byte_start);
                        let seg_end = range_end.min(line.byte_end);
                        if seg_start >= seg_end {
                            continue;
                        }

                        let glyph_data = self.editor_layout.glyph_positions_on_line(
                            line,
                            seg_start,
                            seg_end,
                            font_size,
                            font_family,
                        );
                        let line_baseline_y =
                            text_baseline_y(line, font_size, font_family) - scroll_y;
                        for (abs_byte, x_pos, ch_w, _glyph_idx, _raw_font) in glyph_data {
                            if abs_byte >= text.len() {
                                continue;
                            }
                            let ch = text
                                .get(abs_byte..)
                                .and_then(|s| s.chars().next())
                                .unwrap_or(' ');
                            let char_len = ch.len_utf8();
                            if is_complex_grapheme(ch) {
                                complex_byte_ranges.push((abs_byte, abs_byte + char_len));
                                continue;
                            }
                            glyph_rects.push(GlyphRect {
                                x: line.x + x_pos,
                                y: line.y - scroll_y,
                                w: ch_w,
                                h: line.height,
                                char_: ch.to_string(),
                                baseline_y: line_baseline_y,
                                byte_start: abs_byte,
                                byte_end: abs_byte + char_len,
                            });
                        }
                    }

                    if let Some(ref cluster_rects) = vt.cluster_rects {
                        for cluster in cluster_rects {
                            if !cluster.is_complex {
                                continue;
                            }
                            let cluster_lines: Vec<usize> = insert_snapshot
                                .lines
                                .iter()
                                .enumerate()
                                .filter(|(_, l)| {
                                    l.byte_start < cluster.byte_end
                                        && l.byte_end > cluster.byte_start
                                        && !l.para_text.is_empty()
                                })
                                .map(|(i, _)| i)
                                .collect();
                            if cluster_lines.is_empty() {
                                continue;
                            }

                            let mut min_x = f64::MAX;
                            let mut min_y = f64::MAX;
                            let mut max_right = f64::MIN;
                            let mut max_bottom = f64::MIN;
                            let mut cluster_baseline_y: f64 = 0.0;

                            for &li in &cluster_lines {
                                let line = &insert_snapshot.lines[li];
                                let seg_start = cluster.byte_start.max(line.byte_start);
                                let seg_end = cluster.byte_end.min(line.byte_end);
                                if seg_start >= seg_end {
                                    continue;
                                }
                                let glyph_data = self.editor_layout.glyph_positions_on_line(
                                    line,
                                    seg_start,
                                    seg_end,
                                    font_size,
                                    font_family,
                                );
                                let line_baseline_y =
                                    text_baseline_y(line, font_size, font_family) - scroll_y;
                                for (abs_byte, x_pos, ch_w, _glyph_idx, _raw_font) in &glyph_data {
                                    let glyph_right = line.x + x_pos + ch_w;
                                    let glyph_bottom = line.y - scroll_y + line.height;
                                    if line.x + *x_pos < min_x {
                                        min_x = line.x + *x_pos;
                                    }
                                    if line.y - scroll_y < min_y {
                                        min_y = line.y - scroll_y;
                                    }
                                    if glyph_right > max_right {
                                        max_right = glyph_right;
                                    }
                                    if glyph_bottom > max_bottom {
                                        max_bottom = glyph_bottom;
                                    }
                                    cluster_baseline_y = line_baseline_y;
                                    let _ = abs_byte;
                                }
                            }

                            if min_x < f64::MAX && max_right > f64::MIN {
                                glyph_rects.push(GlyphRect {
                                    x: min_x,
                                    y: min_y,
                                    w: max_right - min_x,
                                    h: max_bottom - min_y,
                                    char_: cluster.text.clone(),
                                    baseline_y: cluster_baseline_y,
                                    byte_start: cluster.byte_start,
                                    byte_end: cluster.byte_end,
                                });
                            }
                        }
                    }
                }

                vt.insert_glyph_rects = Some(glyph_rects);

                if let Some((range_start, range_end)) = vt.inserted_range {
                    let mut reflow_rects = Vec::new();
                    let old_snapshot = self.layout_snapshot_for_text(old_text, width);

                    let mut affected_line_indices: Vec<usize> = Vec::new();
                    for (line_idx, line) in insert_snapshot.lines.iter().enumerate() {
                        if line.byte_end <= range_start || line.para_text.is_empty() {
                            continue;
                        }
                        if line.byte_start >= range_end || line.byte_end > range_start {
                            let line_top = line.y - scroll_y;
                            let line_bottom = line_top + line.height;
                            if line_bottom < 0.0 || line_top > viewport_h {
                                continue;
                            }
                            affected_line_indices.push(line_idx);
                        }
                    }

                    for line_idx in affected_line_indices {
                        let new_line = &insert_snapshot.lines[line_idx];
                        if new_line.para_text.is_empty() {
                            continue;
                        }

                        let reflow_start = if new_line.byte_start < range_end {
                            range_end
                        } else {
                            new_line.byte_start
                        };
                        let reflow_end = new_line.byte_end;

                        if reflow_start >= reflow_end {
                            continue;
                        }

                        let new_glyph_data = self.editor_layout.glyph_positions_on_line(
                            new_line,
                            reflow_start,
                            reflow_end,
                            font_size,
                            font_family,
                        );

                        for (abs_byte, new_x_pos, ch_w, _glyph_idx, _raw_font) in &new_glyph_data {
                            if *abs_byte >= text.len() {
                                continue;
                            }
                            let ch = text
                                .get(*abs_byte..)
                                .and_then(|s| s.chars().next())
                                .unwrap_or(' ');

                            let char_len = ch.len_utf8();
                            let byte_start = *abs_byte;
                            let byte_end = byte_start + char_len;

                            let old_byte = if *abs_byte >= range_end {
                                abs_byte.saturating_sub(range_end - range_start)
                            } else {
                                *abs_byte
                            };

                            let old_line = old_snapshot.lines.iter().find(|l| {
                                l.byte_start <= old_byte
                                    && l.byte_end > old_byte
                                    && !l.para_text.is_empty()
                            });

                            let (old_x, old_y, old_baseline_y) = match old_line {
                                Some(ol) => {
                                    let old_glyph_data =
                                        self.editor_layout.glyph_positions_on_line(
                                            ol,
                                            old_byte.min(ol.byte_end),
                                            (old_byte + char_len).min(ol.byte_end),
                                            font_size,
                                            font_family,
                                        );
                                    match old_glyph_data.first() {
                                        Some((_, ox, _, _, _)) => {
                                            let old_line_baseline_y =
                                                text_baseline_y(ol, font_size, font_family)
                                                    - scroll_y;
                                            (ol.x + *ox, ol.y - scroll_y, old_line_baseline_y)
                                        }
                                        None => continue,
                                    }
                                }
                                None => continue,
                            };

                            let new_baseline_y =
                                text_baseline_y(new_line, font_size, font_family) - scroll_y;

                            reflow_rects.push(ReflowGlyphRect {
                                char_: ch.to_string(),
                                byte_start,
                                byte_end,
                                old_x: old_x,
                                old_y: old_y,
                                old_baseline_y,
                                new_x: new_line.x + *new_x_pos,
                                new_y: new_line.y - scroll_y,
                                new_baseline_y,
                                w: *ch_w,
                                h: new_line.height,
                                line_index: line_idx,
                            });
                        }
                    }

                    vt.reflow_glyph_rects = if reflow_rects.is_empty() {
                        None
                    } else {
                        Some(reflow_rects)
                    };
                } else {
                    vt.reflow_glyph_rects = None;
                }
            }
            EditorAnimationKind::Delete => {
                let delete_snapshot = self.layout_snapshot_for_text(old_text, width);
                let mut glyph_rects = Vec::new();

                let changes = writer_core::editor::diff_plain_text(old_text, text);
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete {
                        index,
                        text: deleted_text,
                    } = change
                    {
                        let range_start = *index;
                        let range_end = range_start + deleted_text.len();

                        let old_caret = self.editor_layout.caret_rect(
                            &delete_snapshot,
                            vt.old_selection.head.index,
                            CaretAffinity::Downstream,
                            scroll_y,
                            viewport_h,
                        );
                        vt.old_cursor_rect = Some(make_cursor_rect(
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
                        vt.new_cursor_rect = Some(make_cursor_rect(
                            &new_caret,
                            &new_snapshot,
                            font_family,
                            scroll_y,
                        ));

                        let mut complex_byte_ranges: Vec<(usize, usize)> = Vec::new();

                        for line in &delete_snapshot.lines {
                            if line.byte_end <= range_start || line.byte_start >= range_end {
                                continue;
                            }
                            if line.para_text.is_empty() {
                                continue;
                            }
                            let line_top = line.y - scroll_y;
                            let line_bottom = line_top + line.height;
                            if line_bottom < 0.0 || line_top > viewport_h {
                                continue;
                            }
                            let seg_start = range_start.max(line.byte_start);
                            let seg_end = range_end.min(line.byte_end);
                            if seg_start >= seg_end {
                                continue;
                            }

                            let glyph_data = self.editor_layout.glyph_positions_on_line(
                                line,
                                seg_start,
                                seg_end,
                                font_size,
                                font_family,
                            );
                            let line_baseline_y =
                                text_baseline_y(line, font_size, font_family) - scroll_y;
                            for (abs_byte, x_pos, ch_w, _glyph_idx, _raw_font) in glyph_data {
                                if abs_byte >= old_text.len() {
                                    continue;
                                }
                                let ch = old_text
                                    .get(abs_byte..)
                                    .and_then(|s| s.chars().next())
                                    .unwrap_or(' ');
                                let char_len = ch.len_utf8();
                                if is_complex_grapheme(ch) {
                                    complex_byte_ranges.push((abs_byte, abs_byte + char_len));
                                    continue;
                                }
                                glyph_rects.push(GlyphRect {
                                    x: line.x + x_pos,
                                    y: line.y - scroll_y,
                                    w: ch_w,
                                    h: line.height,
                                    char_: ch.to_string(),
                                    baseline_y: line_baseline_y,
                                    byte_start: abs_byte,
                                    byte_end: abs_byte + char_len,
                                });
                            }
                        }

                        if let Some(ref cluster_rects) = vt.cluster_rects {
                            for cluster in cluster_rects {
                                if !cluster.is_complex {
                                    continue;
                                }
                                let cluster_lines: Vec<usize> = delete_snapshot
                                    .lines
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, l)| {
                                        l.byte_start < cluster.byte_end
                                            && l.byte_end > cluster.byte_start
                                            && !l.para_text.is_empty()
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                if cluster_lines.is_empty() {
                                    continue;
                                }

                                let mut min_x = f64::MAX;
                                let mut min_y = f64::MAX;
                                let mut max_right = f64::MIN;
                                let mut max_bottom = f64::MIN;
                                let mut cluster_baseline_y: f64 = 0.0;

                                for &li in &cluster_lines {
                                    let line = &delete_snapshot.lines[li];
                                    let seg_start = cluster.byte_start.max(line.byte_start);
                                    let seg_end = cluster.byte_end.min(line.byte_end);
                                    if seg_start >= seg_end {
                                        continue;
                                    }
                                    let glyph_data = self.editor_layout.glyph_positions_on_line(
                                        line,
                                        seg_start,
                                        seg_end,
                                        font_size,
                                        font_family,
                                    );
                                    let line_baseline_y =
                                        text_baseline_y(line, font_size, font_family) - scroll_y;
                                    for (abs_byte, x_pos, ch_w, _glyph_idx, _raw_font) in &glyph_data {
                                        let glyph_right = line.x + x_pos + ch_w;
                                        let glyph_bottom = line.y - scroll_y + line.height;
                                        if line.x + *x_pos < min_x {
                                            min_x = line.x + *x_pos;
                                        }
                                        if line.y - scroll_y < min_y {
                                            min_y = line.y - scroll_y;
                                        }
                                        if glyph_right > max_right {
                                            max_right = glyph_right;
                                        }
                                        if glyph_bottom > max_bottom {
                                            max_bottom = glyph_bottom;
                                        }
                                        cluster_baseline_y = line_baseline_y;
                                        let _ = abs_byte;
                                    }
                                }

                                if min_x < f64::MAX && max_right > f64::MIN {
                                    glyph_rects.push(GlyphRect {
                                        x: min_x,
                                        y: min_y,
                                        w: max_right - min_x,
                                        h: max_bottom - min_y,
                                        char_: cluster.text.clone(),
                                        baseline_y: cluster_baseline_y,
                                        byte_start: cluster.byte_start,
                                        byte_end: cluster.byte_end,
                                    });
                                }
                            }
                        }
                    }
                }

                vt.deleted_glyph_rects = Some(glyph_rects);
            }
            EditorAnimationKind::Cursor => {
            }
        }
    }
}
