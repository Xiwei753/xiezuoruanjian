//! Linux Qt 文字动画的平台视觉快照层。
//!
//! 本模块把一次 Qt 排版结果固化为不可变行视觉资源和 cluster 几何信息。
//! 动画阶段只能裁剪、移动和混合这些资源，不得再次调用文字排版生成第二套视觉结果。

use qmetaobject::QImage;
use crate::editor::layout::{CaretAffinity, CaretRect, LayoutSnapshot, VisualLine};
pub(crate) use super::layout_revision::LayoutRevision;
pub(crate) use super::snapshot_id::LineSnapshotId;

/// 一次平台排版后的不可变 glyph cluster 视觉快照。
///
/// `byte_start`/`byte_end` 是 UTF-8 文档范围；`source_rect` 是行视觉资源内的局部裁剪区域；
/// `shaping_identity` 用于判断旧视觉是否能直接移动复用（相同则 Move，不同则 CrossFade）。
#[derive(Clone, Debug)]
pub(crate) struct LineClusterSnapshot {
    pub byte_start: usize,
    pub byte_end: usize,
    pub source_rect: SourceRect,
    pub shaping_identity: ShapingIdentity,
    pub visual_line_id: usize,
}

/// 通用矩形载体，具体坐标空间由字段契约决定。
///
/// 在 cluster 快照中 `source_rect` 使用行视觉资源局部坐标（已乘 DPR）；
/// 在动画切片中 `from_document_rect`/`to_document_rect` 使用文档坐标（不含滚动偏移）。
/// 不得仅凭此类型假设是文档坐标。
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

/// 平台 shaping 结果是否可视为同一视觉对象的指纹。
///
/// 这不是文字逻辑 identity，而是"当前平台 shaping 结果是否可视为同一视觉对象"的判断依据。
/// 字体、glyph 索引、方向、格式任一变化都应视为不同 shaping，此时旧视觉不能直接移动，
/// 必须走 CrossFade（旧视觉淡出 + 新视觉淡入）。
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

/// 一次平台视觉事务持有的不可变行快照。
///
/// `image`/视觉资源、clusters、文档 byte range、visual line、DPR、段落上下文共同构成
/// 一次排版的完整视觉记录。事务进入 `Completed` 或 `Cancelled` 前，所有被 slice 引用的
/// 视觉资源必须保持有效。
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
    /// 聚合与 `byte_start..byte_end` 相交的所有 cluster 的 `source_rect`。
    /// 相交语义：cluster 的 byte range 与查询 range 有重叠即纳入。
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

/// 一次完整排版的不可变快照集合。
///
/// `revision` 标识同一批布局视觉结果，不是正文版本号的替代品。
/// old/new snapshot 的 revision 不同，动画切片只能引用创建时对应的 revision，
/// 不得混用。
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
