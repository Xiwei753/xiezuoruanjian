use qmetaobject::QImage;
use crate::editor::layout::{CaretAffinity, CaretRect, LayoutParams, LayoutSnapshot, VisualLine};
pub(crate) use super::layout_revision::LayoutRevision;

#[derive(Clone, Debug)]
pub(crate) struct ParagraphIndexMap {
    entries: Vec<ParagraphEntry>,
}

#[derive(Clone, Debug)]
struct ParagraphEntry {
    para_start_byte: usize,
    para_end_byte: usize,
    para_start_qchar: usize,
    para_end_qchar: usize,
}

impl ParagraphIndexMap {
    pub fn from_text(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut byte_pos = 0usize;
        let mut qchar_pos = 0usize;

        for line in text.split('\n') {
            let line_bytes = line.len();
            let line_qchars = line.chars().count();

            entries.push(ParagraphEntry {
                para_start_byte: byte_pos,
                para_end_byte: byte_pos + line_bytes,
                para_start_qchar: qchar_pos,
                para_end_qchar: qchar_pos + line_qchars,
            });

            byte_pos += line_bytes;
            qchar_pos += line_qchars;

            if byte_pos < text.len() {
                byte_pos += 1;
                qchar_pos += 1;
            }
        }

        ParagraphIndexMap { entries }
    }

    pub fn byte_to_qchar(&self, byte_offset: usize) -> usize {
        for entry in &self.entries {
            if byte_offset >= entry.para_start_byte && byte_offset <= entry.para_end_byte {
                let within_para_byte = byte_offset - entry.para_start_byte;
                return entry.para_start_qchar + within_para_byte;
            }
        }
        byte_offset
    }

    pub fn qchar_to_byte(&self, qchar_offset: usize) -> usize {
        for entry in &self.entries {
            if qchar_offset >= entry.para_start_qchar && qchar_offset <= entry.para_end_qchar {
                let within_para_qchar = qchar_offset - entry.para_start_qchar;
                return entry.para_start_byte + within_para_qchar;
            }
        }
        qchar_offset
    }

    pub fn paragraph_for_byte(&self, byte_offset: usize) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| byte_offset >= e.para_start_byte && byte_offset <= e.para_end_byte)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineSnapshotId {
    pub revision: LayoutRevision,
    pub visual_line_id: usize,
}

impl LineSnapshotId {
    pub fn new(revision: LayoutRevision, visual_line_id: usize) -> Self {
        Self { revision, visual_line_id }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LineClusterSnapshot {
    pub byte_start: usize,
    pub byte_end: usize,
    pub source_rect: SourceRect,
    pub shaping_identity: ShapingIdentity,
    pub visual_line_id: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SourceRect {
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }
    }

    pub fn intersects(&self, other: &SourceRect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapingIdentity {
    pub text_content_hash: u64,
    pub raw_font_fingerprint: String,
    pub glyph_indexes_hash: u64,
    pub cluster_glyph_count: usize,
    pub direction_rtl: bool,
    pub format_fingerprint: u64,
}

impl ShapingIdentity {
    pub fn is_same_shaping(&self, other: &ShapingIdentity) -> bool {
        self.text_content_hash == other.text_content_hash
            && self.raw_font_fingerprint == other.raw_font_fingerprint
            && self.glyph_indexes_hash == other.glyph_indexes_hash
            && self.cluster_glyph_count == other.cluster_glyph_count
            && self.direction_rtl == other.direction_rtl
            && self.format_fingerprint == other.format_fingerprint
    }
}

#[derive(Clone)]
pub(crate) struct PreparedLineSnapshot {
    pub id: LineSnapshotId,
    pub image: Option<QImage>,
    pub clusters: Vec<LineClusterSnapshot>,
    pub document_origin_y: f64,
    pub baseline_y: f64,
    pub dpr: f64,
    pub line_height: f64,
    pub line_width: f64,
    pub byte_start: usize,
    pub byte_end: usize,
    pub para_text: String,
    pub para_start: usize,
    pub qtextline_idx: i32,
    pub paragraph_wrap_w: f64,
    pub para_indent: f64,
}

impl PreparedLineSnapshot {
    pub fn source_rect_for_byte_range(&self, byte_start: usize, byte_end: usize) -> Option<SourceRect> {
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_right = f64::MIN;
        let mut max_bottom = f64::MIN;

        for cluster in &self.clusters {
            if cluster.byte_end <= byte_start || cluster.byte_start >= byte_end {
                continue;
            }
            let sr = &cluster.source_rect;
            min_x = min_x.min(sr.x);
            min_y = min_y.min(sr.y);
            max_right = max_right.max(sr.x + sr.w);
            max_bottom = max_bottom.max(sr.y + sr.h);
        }

        if min_x < f64::MAX && max_right > f64::MIN {
            Some(SourceRect {
                x: min_x,
                y: min_y,
                w: max_right - min_x,
                h: max_bottom - min_y,
            })
        } else {
            None
        }
    }

    pub fn clusters_in_byte_range(&self, byte_start: usize, byte_end: usize) -> Vec<&LineClusterSnapshot> {
        self.clusters
            .iter()
            .filter(|c| c.byte_end > byte_start && c.byte_start < byte_end)
            .collect()
    }
}

#[derive(Clone)]
pub(crate) struct EditorLayoutSnapshot {
    pub revision: LayoutRevision,
    pub layout_snapshot: LayoutSnapshot,
    pub paragraph_index_map: ParagraphIndexMap,
    pub line_snapshots: Vec<PreparedLineSnapshot>,
    pub caret_rect: Option<CaretRect>,
    pub caret_affinity: CaretAffinity,
}

impl EditorLayoutSnapshot {
    pub fn new(
        layout_snapshot: LayoutSnapshot,
        text: &str,
        caret_rect: Option<CaretRect>,
        caret_affinity: CaretAffinity,
    ) -> Self {
        let revision = LayoutRevision::next();
        let paragraph_index_map = ParagraphIndexMap::from_text(text);

        let line_snapshots = layout_snapshot.lines.iter().map(|line| {
            PreparedLineSnapshot {
                id: LineSnapshotId::new(revision, line.id),
                image: None,
                clusters: Vec::new(),
                document_origin_y: line.y,
                baseline_y: 0.0,
                dpr: 1.0,
                line_height: line.height,
                line_width: line.width,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                para_text: line.para_text.clone(),
                para_start: line.para_start,
                qtextline_idx: line.qtextline_idx,
                paragraph_wrap_w: line.line_wrap_width + line.line_indent_x,
                para_indent: line.para_indent,
            }
        }).collect();

        EditorLayoutSnapshot {
            revision,
            layout_snapshot,
            paragraph_index_map,
            line_snapshots,
            caret_rect,
            caret_affinity,
        }
    }

    pub fn line_for_byte(&self, byte_offset: usize) -> Option<&PreparedLineSnapshot> {
        self.line_snapshots
            .iter()
            .find(|l| l.byte_end >= byte_offset && l.byte_start <= byte_offset)
    }

    pub fn lines_in_byte_range(&self, byte_start: usize, byte_end: usize) -> Vec<&PreparedLineSnapshot> {
        self.line_snapshots
            .iter()
            .filter(|l| l.byte_end > byte_start && l.byte_start < byte_end)
            .collect()
    }

    pub fn content_height(&self) -> f32 {
        self.layout_snapshot.content_height
    }

    pub fn visual_lines(&self) -> &[VisualLine] {
        &self.layout_snapshot.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_revision_monotonic() {
        let r1 = LayoutRevision::next();
        let r2 = LayoutRevision::next();
        assert!(r2 > r1);
    }

    #[test]
    fn test_paragraph_index_map_simple() {
        let map = ParagraphIndexMap::from_text("hello\nworld");
        assert_eq!(map.paragraph_for_byte(0), Some(0));
        assert_eq!(map.paragraph_for_byte(6), Some(1));
    }

    #[test]
    fn test_source_rect_zero() {
        let sr = SourceRect::zero();
        assert_eq!(sr.x, 0.0);
        assert_eq!(sr.w, 0.0);
    }

    #[test]
    fn test_shaping_identity_same() {
        let a = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "Arial:w50:s16".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 2,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let b = a.clone();
        assert!(a.is_same_shaping(&b));
    }

    #[test]
    fn test_shaping_identity_different() {
        let a = ShapingIdentity {
            text_content_hash: 42,
            raw_font_fingerprint: "Arial:w50:s16".into(),
            glyph_indexes_hash: 100,
            cluster_glyph_count: 2,
            direction_rtl: false,
            format_fingerprint: 0,
        };
        let mut b = a.clone();
        b.glyph_indexes_hash = 200;
        assert!(!a.is_same_shaping(&b));
    }
}
