use qmetaobject::QImage;
use crate::editor::layout::{CaretAffinity, CaretRect, LayoutSnapshot, VisualLine};
pub(crate) use super::layout_revision::LayoutRevision;
pub(crate) use super::snapshot_id::LineSnapshotId;

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
    pub visual_x: f64,
    pub scroll_y: f64,
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

    pub fn intersects_byte_range(&self, start: usize, end: usize) -> bool {
        self.byte_end > start && self.byte_start < end
    }
}

#[derive(Clone)]
pub(crate) struct EditorLayoutSnapshot {
    pub revision: LayoutRevision,
    pub layout_snapshot: LayoutSnapshot,
    pub line_snapshots: Vec<PreparedLineSnapshot>,
    pub caret_rect: Option<CaretRect>,
    pub caret_affinity: CaretAffinity,
}

impl std::fmt::Debug for EditorLayoutSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorLayoutSnapshot")
            .field("revision", &self.revision)
            .field("line_count", &self.line_snapshots.len())
            .finish()
    }
}

impl EditorLayoutSnapshot {
    pub fn new(
        layout_snapshot: LayoutSnapshot,
        line_snapshots: Vec<PreparedLineSnapshot>,
        caret_rect: Option<CaretRect>,
        caret_affinity: CaretAffinity,
    ) -> Self {
        let revision = LayoutRevision::next();
        EditorLayoutSnapshot {
            revision,
            layout_snapshot,
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
