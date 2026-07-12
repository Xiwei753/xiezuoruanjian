//! 从 CanonicalDocumentVisualSnapshot 构建 EditorLayoutSnapshot。
//!
//! 输入是 CanonicalDocumentVisualSnapshot（由 layout 模块完成 QTextLayout 排版后产出）。
//! PreparedLineSnapshot 的 image、cluster、byte range、visual line 和 shaping identity
//! 均来自同一批 canonical 排版结果，动画阶段只消费快照，不再次排版。
//!
//! source_rect 坐标单位为像素，已乘 DPR；消费端直接用于 QImage 裁剪。
//!
//! canonical 排版使用 QChar/UTF-16 offset（QTextLayout 原生坐标），
//! 此处通过 CanonicalClusterSnapshot 的 document_byte_start/document_byte_end
//! 将 UTF-16 范围转换为 UTF-8 document byte range，供跨平台事务使用。
//!
//! 职责边界：
//! - build_from_canonical_document()：从整个文档快照构建 EditorLayoutSnapshot，
//!   包含视口裁剪、段落 ID 分配、baseline 计算和空行处理。
//! - build_clusters_from_canonical()：从单条 canonical line 的 cluster 列表
//!   构建 LineClusterSnapshot，负责 source_rect 裁剪和 shaping identity 哈希。

use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, PreparedLineSnapshot,
    ShapingIdentity, SourceRect,
};
use super::line_snapshot::LineTextureStore;
use crate::editor::layout::{
    CanonicalDocumentVisualSnapshot, CanonicalLineSnapshot, VisualLine,
};
use crate::editor::layout;
use crate::editor::paragraph_index_map::ParagraphIndexMap;

pub(crate) struct LineSnapshotBuilder;

impl LineSnapshotBuilder {
    pub fn build_from_canonical_document(
        revision: LayoutRevision,
        doc_snapshot: &CanonicalDocumentVisualSnapshot,
        scroll_y: f64,
        viewport_h: f64,
    ) -> EditorLayoutSnapshot {
        let mut line_snapshots = Vec::new();
        let mut paragraph_id: u64 = 0;
        let mut prev_para_start: Option<usize> = None;
        let mut visual_line_ordinal: u32 = 0;

        for line in &doc_snapshot.visual_lines {
            if line.para_text.is_empty() {
                paragraph_id = paragraph_id.wrapping_add(1);
                visual_line_ordinal = 0;
                prev_para_start = Some(line.para_start);

                let line_top = line.y - scroll_y;
                let line_bottom = line_top + line.height;
                if line_bottom < -line.height || line_top > viewport_h + line.height {
                    visual_line_ordinal += 1;
                    continue;
                }

                let baseline_y = layout::text_baseline_y(line, doc_snapshot.font_size, &doc_snapshot.font_family);
                let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);

                line_snapshots.push(PreparedLineSnapshot {
                    id,
                    image: None,
                    clusters: Vec::new(),
                    document_origin_y: line.y,
                    baseline_y,
                    dpr: doc_snapshot.dpr,
                    line_height: line.height,
                    line_width: line.width,
                    byte_start: line.byte_start,
                    byte_end: line.byte_end,
                    para_text: String::new(),
                    para_start: line.para_start,
                    qtextline_idx: line.qtextline_idx,
                    paragraph_wrap_w: line.line_wrap_width + line.line_indent_x,
                    para_indent: line.para_indent,
                    visual_x: line.x,
                    scroll_y,
                });

                visual_line_ordinal += 1;
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

            let canonical_para = doc_snapshot.paragraphs.iter().find(|p| {
                p.paragraph_document_byte_start == line.para_start
            });

            let (image, clusters) = if let Some(canonical) = canonical_para {
                let canonical_line = canonical.lines.get(line.qtextline_idx as usize);
                let img = canonical_line.and_then(|cl| cl.image.clone());
                let cls = canonical_line
                    .map(|cl| Self::build_clusters_from_canonical(cl, line, &canonical.index_map))
                    .unwrap_or_default();
                (img, cls)
            } else {
                (None, Vec::new())
            };

            let baseline_y = layout::text_baseline_y(line, doc_snapshot.font_size, &doc_snapshot.font_family);
            let wrap_w = line.line_wrap_width + line.line_indent_x;
            let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);

            line_snapshots.push(PreparedLineSnapshot {
                id,
                image,
                clusters,
                document_origin_y: line.y,
                baseline_y,
                dpr: doc_snapshot.dpr,
                line_height: line.height,
                line_width: line.width,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                para_text: line.para_text.clone(),
                para_start: line.para_start,
                qtextline_idx: line.qtextline_idx,
                paragraph_wrap_w: wrap_w,
                para_indent: line.para_indent,
                visual_x: line.x,
                scroll_y,
            });

            visual_line_ordinal += 1;
        }

        EditorLayoutSnapshot {
            revision,
            layout_snapshot: doc_snapshot.to_layout_snapshot(),
            line_snapshots,
            caret_rect: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Downstream,
        }
    }

    pub fn build_old_new_from_canonical(
        old_doc: &CanonicalDocumentVisualSnapshot,
        new_doc: &CanonicalDocumentVisualSnapshot,
        old_revision: LayoutRevision,
        new_revision: LayoutRevision,
        scroll_y: f64,
        viewport_h: f64,
    ) -> (EditorLayoutSnapshot, EditorLayoutSnapshot) {
        let old_layout = Self::build_from_canonical_document(
            old_revision, old_doc, scroll_y, viewport_h,
        );
        let new_layout = Self::build_from_canonical_document(
            new_revision, new_doc, scroll_y, viewport_h,
        );
        (old_layout, new_layout)
    }

    fn build_clusters_from_canonical(
        canonical_line: &CanonicalLineSnapshot,
        line: &VisualLine,
        _index_map: &ParagraphIndexMap,
    ) -> Vec<LineClusterSnapshot> {
        canonical_line
            .clusters
            .iter()
            .map(|cc| {
                let shaping_identity = ShapingIdentity {
                    text_content_hash: Self::hash_u32(&[
                        cc.qchar_start as u32,
                        cc.qchar_end as u32,
                    ]),
                    raw_font_fingerprint: cc.raw_font_fingerprint.clone(),
                    glyph_indexes_hash: Self::hash_u32(&[cc.first_glyph_index]),
                    cluster_glyph_count: cc.glyph_count,
                    direction_rtl: cc.is_rtl,
                    format_fingerprint: 0,
                };

                LineClusterSnapshot {
                    byte_start: cc.document_byte_start,
                    byte_end: cc.document_byte_end,
                    source_rect: SourceRect {
                        x: cc.source_rect_x,
                        y: cc.source_rect_y,
                        w: cc.source_rect_w,
                        h: cc.source_rect_h,
                    },
                    shaping_identity,
                    visual_line_id: line.id,
                }
            })
            .collect()
    }

    fn hash_u32(data: &[u32]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
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
