//! 行视觉快照构建器。
//!
//! 同一次 `QTextLayout`/`QTextLine` 结果用于生成行图像、cluster/source rect 和
//! shaping identity。动画阶段只消费快照，不再次排版文字。

use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot,
    ShapingIdentity, SourceRect,
};
use super::line_snapshot::LineTextureStore;
use crate::editor::layout::{LayoutSnapshot, VisualLine};
use crate::editor::layout;

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
        dpr: f64,
        text_color: &str,
        generate_images: bool,
    ) -> EditorLayoutSnapshot {
        let mut line_snapshots = Vec::new();
        let mut paragraph_id: u64 = 0;
        let mut prev_para_start: Option<usize> = None;
        let mut visual_line_ordinal: u32 = 0;

        for (line_idx, line) in snapshot.lines.iter().enumerate() {
            if line.para_text.is_empty() {
                continue;
            }

            if prev_para_start != Some(line.para_start) {
                paragraph_id = paragraph_id.wrapping_add(1);
                visual_line_ordinal = 0;
                prev_para_start = Some(line.para_start);
            }

            let line_top = line.y - scroll_y;
            let line_bottom = line_top + line.height;
            if line_bottom < -line.height || line_top > viewport_h + line.height {
                visual_line_ordinal += 1;
                continue;
            }

            let wrap_w = line.line_wrap_width + line.line_indent_x;
            let indent = text_indent;
            let baseline_y = layout::text_baseline_y(line, font_size, font_family);

            let image = if generate_images {
                layout::render_line_to_image(
                    &line.para_text,
                    font_size,
                    font_family,
                    wrap_w,
                    indent,
                    line.qtextline_idx,
                    dpr,
                    text_color,
                )
            } else {
                None
            };

            let clusters = Self::build_clusters_for_line(
                line,
                &line.para_text,
                font_size,
                font_family,
                wrap_w,
                indent,
                dpr,
            );

            let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);

            line_snapshots.push(PreparedLineSnapshot {
                id,
                image,
                clusters,
                document_origin_y: line.y,
                baseline_y,
                dpr,
                line_height: line.height,
                line_width: line.width,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                para_text: line.para_text.clone(),
                para_start: line.para_start,
                qtextline_idx: line.qtextline_idx,
                paragraph_wrap_w: wrap_w,
                para_indent: indent,
                visual_x: line.x,
                scroll_y,
            });

            visual_line_ordinal += 1;
        }

        EditorLayoutSnapshot {
            revision,
            layout_snapshot: snapshot.clone(),
            line_snapshots,
            caret_rect: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Downstream,
        }
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
        dpr: f64,
        text_color: &str,
    ) -> (EditorLayoutSnapshot, EditorLayoutSnapshot) {
        let old_layout = Self::build_from_layout(
            old_revision, old_snapshot, font_size, font_family, scroll_y, viewport_h, text_indent, dpr, text_color, true,
        );
        let new_layout = Self::build_from_layout(
            new_revision, new_snapshot, font_size, font_family, scroll_y, viewport_h, text_indent, dpr, text_color, true,
        );
        (old_layout, new_layout)
    }

    fn build_clusters_for_line(
        line: &VisualLine,
        para_text: &str,
        font_size: f64,
        font_family: &str,
        wrap_w: f64,
        indent: f64,
        dpr: f64,
    ) -> Vec<LineClusterSnapshot> {
        let shaped_run_data = layout::extract_shaped_runs_on_line(
            para_text,
            line.byte_start,
            line.byte_end,
            line.byte_start,
            font_size,
            font_family,
            wrap_w,
            indent,
            line.qtextline_idx,
        );

        let mut clusters = Vec::new();
        for srd in &shaped_run_data {
            let run_x = srd.visual_x;
            let run_h = srd.visual_h;

            if srd.glyphs.is_empty() {
                continue;
            }

            let mut glyph_clusters: Vec<(usize, usize, i32, i32)> = Vec::new();
            let mut cluster_start = 0usize;
            for i in 1..srd.glyphs.len() {
                if srd.glyphs[i].string_index != srd.glyphs[i - 1].string_index {
                    glyph_clusters.push((
                        cluster_start,
                        i,
                        srd.glyphs[cluster_start].string_index,
                        srd.glyphs[i - 1].string_index,
                    ));
                    cluster_start = i;
                }
            }
            glyph_clusters.push((
                cluster_start,
                srd.glyphs.len(),
                srd.glyphs[cluster_start].string_index,
                srd.glyphs.last().unwrap().string_index,
            ));

            for (glyph_start, glyph_end, str_start, str_end) in &glyph_clusters {
                let mut min_x = f64::MAX;
                let mut max_x = f64::MIN;
                for g in &srd.glyphs[*glyph_start..*glyph_end] {
                    min_x = min_x.min(g.position_x);
                    max_x = max_x.max(g.position_x + g.advance_width);
                }

                if min_x >= max_x {
                    continue;
                }

                let local_x = (min_x - run_x).max(0.0);
                let local_w = max_x - min_x;

                // UTF-8 byte offset 与 Qt UTF-16/QChar offset 的转换边界：
                // Qt glyph 的 string_index 是 QChar offset（UTF-16），需加 line.byte_start
                // 转为 UTF-8 文档 byte offset。
                let byte_start = line.byte_start + (*str_start as usize);
                let byte_end = line.byte_start + (*str_end as usize) + 1;

                // glyph run/cluster 边界由 Qt shaping 决定，不能按 Rust `char` 或
                // 单个 code point 推断：一个 cluster 可能包含多个 glyph（ligature），
                // 也可能一个 glyph 覆盖多个 code point（ZWJ emoji）。
                // source_rect 使用行视觉资源局部坐标，已乘 DPR；
                // local_x 是从 run 起始位置偏移后的逻辑坐标，乘 DPR 后对应 QImage 像素坐标。
                let source_rect = SourceRect {
                    x: local_x * dpr,
                    y: 0.0,
                    w: local_w * dpr,
                    h: run_h * dpr,
                };

                let shaping_identity = ShapingIdentity {
                    text_content_hash: Self::hash_u32(&[*str_start as u32, *str_end as u32]),
                    raw_font_fingerprint: format!("{}:w{}:s{}", srd.raw_font_family, srd.raw_font_weight, srd.raw_font_pixel_size),
                    glyph_indexes_hash: Self::hash_glyph_indexes(&srd.glyphs[*glyph_start..*glyph_end]),
                    cluster_glyph_count: glyph_end - glyph_start,
                    direction_rtl: srd.is_rtl,
                    format_fingerprint: 0,
                };

                clusters.push(LineClusterSnapshot {
                    byte_start,
                    byte_end,
                    source_rect,
                    shaping_identity,
                    visual_line_id: line.id,
                });
            }
        }

        clusters
    }

    fn hash_u32(data: &[u32]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_glyph_indexes(glyphs: &[layout::RunGlyphData]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        for g in glyphs {
            g.glyph_index.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn prepare_line_textures(
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        texture_store: &mut LineTextureStore,
    ) {
        for line in &old_snapshot.line_snapshots {
            if let Some(ref image) = line.image {
                if !texture_store.contains(&line.id) {
                    texture_store.insert(line.id, image.clone());
                }
            }
        }
        for line in &new_snapshot.line_snapshots {
            if let Some(ref image) = line.image {
                if !texture_store.contains(&line.id) {
                    texture_store.insert(line.id, image.clone());
                }
            }
        }
    }
}
