//! Linux Qt 文字动画的平台视觉快照层。
//!
//! 本模块把一次 Qt 排版结果固化为不可变行视觉资源和 cluster 几何信息。
//! 动画阶段只能裁剪、移动和混合这些资源，不得再次调用文字排版生成第二套视觉结果。

pub(crate) use super::layout_revision::LayoutRevision;
pub(crate) use super::snapshot_id::LineSnapshotId;
use crate::editor::layout::{CaretAffinity, CaretRect, LayoutSnapshot};
use cpp::cpp;
use qmetaobject::QImage;

// Issue #724 评论 5751573705 问题1: C++ 侧头文件 + 外部函数声明。
// 每个 cpp! 块独立编译，此处必须在本文件内 include 头文件和声明 extern。
// get_glyph_range_rect 在 qt_text_node.rs 的 cpp! 块中定义，此处 extern 声明。
cpp! {{
    #include <QtGui/QTextLayout>
    extern QTextLayout* get_paragraph_layout(uint64_t gen, int slot);
    bool get_glyph_range_rect(uint64_t generation, int cacheSlot, int qtextlineIdx,
                              int qcharStart, int qcharEnd,
                              double* outX, double* outY, double* outW, double* outH);
}}

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
        Self {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
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
    pub dpr: f64,
    pub byte_start: usize,
    pub byte_end: usize,
    pub visual_x: f64,
    /// Issue #722 评论 5749572808 问题1: 该行在全文视觉行中的身份，
    /// 原样从源 `VisualLine.id` 带下来。动画切片的 `visual_line_id`
    /// 必须用这个全文行号，不能用 `line_snapshots` 的局部数组下标——
    /// 视口裁剪后局部下标和全文行号不一致，跨行裁切会判断错。
    pub visual_line_id: usize,
    /// Issue #722 评论 5750218208: 真实视觉行 top（= `VisualLine.y`，文档坐标）。
    /// 行几何判断直接用此字段，不再从 cluster ink bounds 猜行高。
    pub visual_line_top: f64,
    /// Issue #722 评论 5750218208: 真实视觉行 bottom（= `VisualLine.y + VisualLine.height`，
    /// 文档坐标）。空行也有正确高度，不依赖 cluster。
    pub visual_line_bottom: f64,
    /// Issue #724 评论 5751573705 问题1: 该视觉行所属段落在 C++ 缓存中的槽位，
    /// 用于从 QTextLayout 获取精确 glyph 几何。
    pub cache_slot: i32,
    /// Issue #724 评论 5751573705 问题1: 该视觉行在 QTextLayout 中的行索引，
    /// 配合 cache_slot 定位 QTextLine。
    pub qtextline_idx: i32,
}

/// Issue #724 评论 5751268664 缺口1: cluster 相对 inserted 子范围的位置分类。
///
/// `Inside`：cluster 完全落在 inserted 范围内，整个 cluster 都是新插入文字，
/// 进入 InsertReveal 动画 + 静态层隐藏。
///
/// `Partial`：cluster 部分落在 inserted 范围内（ligature/cluster 跨越 inserted 边界），
/// 只把属于 inserted 的子片段交给 InsertReveal，旧邻字部分保持原样不进入静态层隐藏。
/// `clipped_byte_start/end` 是属于 inserted 的子片段字节范围。
/// `clipped_source_rect` 不在 Rust 侧用 UTF-8 byte 比例猜测——字节长度不是 glyph 宽度，
/// 中文 UTF-8 3 字节/拉丁 1 字节/ligature/组合字符/fallback font/比例字体/RTL
/// 都不能按 byte ratio 对应到 source rect 的 x/w。
/// Partial 的精确 source rect 由调用方通过 `get_precise_glyph_rect_for_byte_range`
/// 从 QTextLayout 侧获取（支持 split ligature 的实际 glyph 几何）。
#[derive(Clone, Debug)]
pub(crate) enum ClusterInsertRelation {
    /// cluster 完全在 inserted 范围内。
    Inside,
    /// cluster 部分在 inserted 范围内。`clipped_byte_start/end` 是属于 inserted 的
    /// 子片段字节范围。精确 `clipped_source_rect` 由调用方从 QTextLayout 获取。
    Partial {
        clipped_byte_start: usize,
        clipped_byte_end: usize,
    },
}

impl LineClusterSnapshot {
    /// Issue #724 评论 5751268664 缺口1: 判断 cluster 相对 inserted 子范围的位置。
    ///
    /// 返回 `Inside`（完全在 inserted 内）或 `Partial`（部分重叠）。
    /// `Partial` 只携带 `clipped_byte_start/end` 字节范围，**不**再自己计算
    /// `clipped_source_rect`——字节长度不是 glyph 宽度，中文 UTF-8 3 字节、
    /// 拉丁 1 字节、ligature、组合字符、fallback font、比例字体、RTL 都不能按
    /// byte ratio 对应到 source rect 的 x/w。
    /// 精确的 source rect 由调用方通过 `get_precise_glyph_rect_for_byte_range`
    /// 从 QTextLayout 侧获取（支持 split ligature 的实际 glyph 几何）。
    ///
    /// cluster_len 为 0 时退化为 Inside（保护性 fallback）。
    pub(crate) fn relate_to_inserted_range(
        &self,
        ins_start: usize,
        ins_end: usize,
    ) -> Option<ClusterInsertRelation> {
        // 不相交：完全在 inserted 范围外，不参与 InsertReveal。
        if self.byte_end <= ins_start || self.byte_start >= ins_end {
            return None;
        }
        let cluster_len = self.byte_end.saturating_sub(self.byte_start);
        // 完全包含：整个 cluster 都是新插入文字。
        if self.byte_start >= ins_start && self.byte_end <= ins_end || cluster_len == 0 {
            return Some(ClusterInsertRelation::Inside);
        }
        // 部分重叠：只返回字节范围，精确 source rect 由调用方从 QTextLayout 获取。
        let clip_start = self.byte_start.max(ins_start);
        let clip_end = self.byte_end.min(ins_end);
        if clip_end <= clip_start {
            return None;
        }
        Some(ClusterInsertRelation::Partial {
            clipped_byte_start: clip_start,
            clipped_byte_end: clip_end,
        })
    }
}

impl PreparedLineSnapshot {
    /// 聚合与 `byte_start..byte_end` 相交的所有 cluster 的 `source_rect`。
    /// 相交语义：cluster 的 byte range 与查询 range 有重叠即纳入。
    pub fn source_rect_for_byte_range(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Option<SourceRect> {
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

    /// Issue #724 评论 5751573705 问题1: 从 QTextLayout 获取精确 glyph 几何。
    ///
    /// 给定文档字节范围 `[byte_start, byte_end)`，通过 C++/Qt QTextLayout
    /// 获取该子范围的精确视觉矩形（支持 split ligature / 实际 glyph 几何），
    /// 不再用 UTF-8 byte 比例猜视觉宽度。
    ///
    /// 参数 `generation` 来自 `EditorLayoutSnapshot.revision.0`，
    /// `full_text` 用于把 UTF-8 byte 偏移转换为 QChar 偏移。
    /// 返回行视觉资源局部坐标的 source rect（已乘 DPR），失败时返回 `None`。
    pub(crate) fn get_precise_glyph_rect_for_byte_range(
        &self,
        generation: u64,
        full_text: &str,
        byte_start: usize,
        byte_end: usize,
    ) -> Option<SourceRect> {
        use crate::editor::paragraph_index_map::utf8_byte_to_utf16_code_unit;
        // 字节范围必须在本文本行内；超出时不查 QTextLayout。
        if byte_start < self.byte_start || byte_end > self.byte_end || byte_end <= byte_start {
            return None;
        }
        let qchar_start = utf8_byte_to_utf16_code_unit(full_text, byte_start);
        let qchar_end = utf8_byte_to_utf16_code_unit(full_text, byte_end);
        if qchar_end <= qchar_start {
            return None;
        }
        let cache_slot = self.cache_slot;
        let qtextline_idx = self.qtextline_idx;
        // 调用 C++ 侧：从 g_paragraph_layout_cache 取 QTextLayout，
        // 用 QTextLine::cursorToX 取精确左/右边缘，写到四个 f64 输出参数。
        let mut rx = 0.0f64;
        let mut ry = 0.0f64;
        let mut rw = 0.0f64;
        let mut rh = 0.0f64;
        cpp!(unsafe [
            generation as "uint64_t",
            cache_slot as "int",
            qtextline_idx as "int",
            qchar_start as "int64_t",
            qchar_end as "int64_t",
            mut rx as "double*",
            mut ry as "double*",
            mut rw as "double*",
            mut rh as "double*"
        ] {
            QTextLayout* layout = get_paragraph_layout(generation, cache_slot);
            if (!layout || qtextline_idx < 0 || qtextline_idx >= layout->lineCount())
                return;
            QTextLine line = layout->lineAt(qtextline_idx);
            if (qchar_start < line.textStart() || qchar_end > line.textStart() + line.textLength())
                return;
            qreal left = line.cursorToX(qchar_start);
            qreal right = line.cursorToX(qchar_end);
            if (right <= left) return;
            *rx = line.x() + left;
            *ry = line.y();
            *rw = right - left;
            *rh = line.height();
        });
        if rw <= 0.0 || rh <= 0.0 {
            return None;
        }
        Some(SourceRect {
            x: rx,
            y: ry,
            w: rw,
            h: rh,
        })
    }

    /// 将行局部物理像素 source_rect 转换为文档逻辑坐标。
    ///
    /// source_rect 来自 cluster 快照，使用行视觉资源局部坐标（已乘 DPR）。
    /// 渲染管线的 `from_document_rect`/`to_document_rect` 要求文档逻辑坐标，
    /// 此方法完成坐标和单位转换。
    pub fn source_rect_to_document_rect(&self, source_rect: &SourceRect) -> SourceRect {
        let dpr = self.dpr.max(0.001);
        SourceRect {
            x: source_rect.x / dpr + self.visual_x,
            y: self.document_origin_y + source_rect.y / dpr,
            w: source_rect.w / dpr,
            h: source_rect.h / dpr,
        }
    }

    pub fn clusters_in_byte_range(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<&LineClusterSnapshot> {
        self.clusters
            .iter()
            .filter(|c| c.byte_end > byte_start && c.byte_start < byte_end)
            .collect()
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
    pub line_snapshots: Vec<PreparedLineSnapshot>,
    /// Viewport 坐标的 caret（y/baseline_y 已减 scroll_y），给平台/IME query 使用。
    pub caret_rect: Option<CaretRect>,
    /// Issue #722 评论 5749791161: 文档坐标的 caret（y/baseline_y 不减 scroll_y，
    /// visible 始终为 true），给 VisualTransaction / caret track 使用。
    pub caret_rect_doc: Option<CaretRect>,
    pub caret_affinity: CaretAffinity,
    pub virtual_text: String,
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
        _layout_snapshot: LayoutSnapshot,
        line_snapshots: Vec<PreparedLineSnapshot>,
        caret_rect: Option<CaretRect>,
        caret_rect_doc: Option<CaretRect>,
        caret_affinity: CaretAffinity,
    ) -> Self {
        let revision = LayoutRevision::next();
        EditorLayoutSnapshot {
            revision,
            line_snapshots,
            caret_rect,
            caret_rect_doc,
            caret_affinity,
            virtual_text: String::new(),
        }
    }

    #[cfg(test)]
    pub fn with_virtual_text(mut self, virtual_text: String) -> Self {
        self.virtual_text = virtual_text;
        self
    }

    pub fn lines_in_byte_range(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<&PreparedLineSnapshot> {
        self.line_snapshots
            .iter()
            .filter(|l| l.byte_end > byte_start && l.byte_start < byte_end)
            .collect()
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

    /// Issue #724 评论 5751268664 缺口1: cluster 相对 inserted 子范围分类 + clipped source rect。
    fn make_cluster(byte_start: usize, byte_end: usize, x: f64, w: f64) -> LineClusterSnapshot {
        LineClusterSnapshot {
            byte_start,
            byte_end,
            source_rect: SourceRect { x, y: 0.0, w, h: 20.0 },
            shaping_identity: ShapingIdentity {
                text_content_hash: 0,
                raw_font_fingerprint: String::new(),
                glyph_indexes_hash: 0,
                cluster_glyph_count: 0,
                direction_rtl: false,
                format_fingerprint: 0,
            },
        }
    }

    #[test]
    fn test_relate_to_inserted_range_disjoint() {
        let c = make_cluster(0, 3, 0.0, 30.0);
        assert!(c.relate_to_inserted_range(5, 8).is_none());
    }

    #[test]
    fn test_relate_to_inserted_range_inside() {
        let c = make_cluster(5, 8, 50.0, 30.0);
        match c.relate_to_inserted_range(0, 10) {
            Some(ClusterInsertRelation::Inside) => {}
            other => panic!("expected Inside, got {:?}", other),
        }
    }

    #[test]
    fn test_relate_to_inserted_range_partial_left_clip() {
        // cluster [0, 10), inserted [5, 15) → 交集 [5, 10)，左半被裁掉
        let c = make_cluster(0, 10, 0.0, 100.0);
        match c.relate_to_inserted_range(5, 15) {
            Some(ClusterInsertRelation::Partial {
                clipped_byte_start,
                clipped_byte_end,
            }) => {
                assert_eq!(clipped_byte_start, 5);
                assert_eq!(clipped_byte_end, 10);
            }
            other => panic!("expected Partial, got {:?}", other),
        }
    }

    #[test]
    fn test_relate_to_inserted_range_partial_right_clip() {
        // cluster [5, 15), inserted [0, 10) → 交集 [5, 10)，右半被裁掉
        let c = make_cluster(5, 15, 50.0, 100.0);
        match c.relate_to_inserted_range(0, 10) {
            Some(ClusterInsertRelation::Partial {
                clipped_byte_start,
                clipped_byte_end,
            }) => {
                assert_eq!(clipped_byte_start, 5);
                assert_eq!(clipped_byte_end, 10);
            }
            other => panic!("expected Partial, got {:?}", other),
        }
    }

    #[test]
    fn test_relate_to_inserted_range_partial_middle_clip() {
        // cluster [0, 20), inserted [5, 15) → 交集 [5, 15)，左右各裁掉 1/4
        let c = make_cluster(0, 20, 0.0, 100.0);
        match c.relate_to_inserted_range(5, 15) {
            Some(ClusterInsertRelation::Partial {
                clipped_byte_start,
                clipped_byte_end,
            }) => {
                assert_eq!(clipped_byte_start, 5);
                assert_eq!(clipped_byte_end, 15);
            }
            other => panic!("expected Partial, got {:?}", other),
        }
    }
}
