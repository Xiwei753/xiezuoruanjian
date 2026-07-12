use super::layout_revision::LayoutRevision;
use super::line_snapshot::{EditorLayoutSnapshot, LineSnapshotId, PreparedLineSnapshot};
use super::shaped_visual_run::{ReflowVisualSnapshot, ShapedVisualRun};
use crate::editor::layout::extract_shaped_runs_on_line;
use crate::editor::layout::LayoutSnapshot;

pub(crate) struct LineSnapshotBuilder;

impl LineSnapshotBuilder {
    pub fn build_from_layout(
        revision: LayoutRevision,
        snapshot: &LayoutSnapshot,
        font_size: f64,
        font_family: &str,
        scroll_y: f64,
        viewport_h: f64,
        text_indent: f64,
    ) -> EditorLayoutSnapshot {
        let mut lines = Vec::new();

        for (line_idx, line) in snapshot.lines.iter().enumerate() {
            if line.para_text.is_empty() {
                continue;
            }
            let line_top = line.y - scroll_y;
            let line_bottom = line_top + line.height;
            if line_bottom < 0.0 || line_top > viewport_h {
                continue;
            }

            let wrap_w = line.line_wrap_width + line.line_indent_x;
            let indent = text_indent;
            let baseline_y = crate::editor::layout::text_baseline_y(line, font_size, font_family);

            let shaped_run_data = extract_shaped_runs_on_line(
                &line.para_text,
                line.byte_start,
                line.byte_end,
                line.byte_start,
                font_size,
                font_family,
                wrap_w,
                indent,
                line.qtextline_idx,
            );

            let shaped_runs: Vec<ShapedVisualRun> = shaped_run_data
                .iter()
                .map(|srd| Self::convert_shaped_run_data(srd, &line.para_text, line.qtextline_idx, wrap_w, indent))
                .collect();

            lines.push(PreparedLineSnapshot {
                id: LineSnapshotId::new(revision, line_idx),
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                visual_x: line.x,
                visual_y: line.y,
                visual_w: line.width,
                visual_h: line.height,
                baseline_y,
                shaped_runs,
                para_text: line.para_text.clone(),
                qtextline_idx: line.qtextline_idx,
                paragraph_wrap_w: wrap_w,
                para_indent: indent,
            });
        }

        EditorLayoutSnapshot { revision, lines }
    }

    pub fn build_old_new_snapshots(
        old_snapshot: &LayoutSnapshot,
        new_snapshot: &LayoutSnapshot,
        old_revision: LayoutRevision,
        new_revision: LayoutRevision,
        font_size: f64,
        font_family: &str,
        scroll_y: f64,
        viewport_h: f64,
        text_indent: f64,
    ) -> (EditorLayoutSnapshot, EditorLayoutSnapshot) {
        let old_layout = Self::build_from_layout(
            old_revision, old_snapshot, font_size, font_family, scroll_y, viewport_h, text_indent,
        );
        let new_layout = Self::build_from_layout(
            new_revision, new_snapshot, font_size, font_family, scroll_y, viewport_h, text_indent,
        );
        (old_layout, new_layout)
    }

    fn convert_shaped_run_data(
        srd: &crate::editor::layout::ShapedRunData,
        para_text: &str,
        qtextline_idx: i32,
        wrap_w: f64,
        indent: f64,
    ) -> ShapedVisualRun {
        use super::shaped_visual_run::{RawFontCacheKey, RunFlags, ShapedCluster, ShapedGlyph, derive_clusters_from_glyphs};

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

    pub fn build_reflow_snapshot(
        old_lines: &[PreparedLineSnapshot],
        new_lines: &[PreparedLineSnapshot],
        affected_byte_start: usize,
        affected_byte_end: usize,
    ) -> ReflowVisualSnapshot {
        let mut old_runs: Vec<ShapedVisualRun> = Vec::new();
        let mut new_runs: Vec<ShapedVisualRun> = Vec::new();

        for line in old_lines {
            if line.intersects_byte_range(affected_byte_start, affected_byte_end) {
                old_runs.extend(line.shaped_runs.iter().cloned());
            }
        }

        for line in new_lines {
            if line.intersects_byte_range(affected_byte_start, affected_byte_end) {
                new_runs.extend(line.shaped_runs.iter().cloned());
            }
        }

        ReflowVisualSnapshot::new(old_runs, new_runs)
    }
}
