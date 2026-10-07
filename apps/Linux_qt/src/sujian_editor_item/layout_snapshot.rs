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
// Issue #724 评论 5752140048 问题 4 额外要求: 文件顶部原声明了
// get_glyph_range_rect(...) 但实际从未调用，qt_text_node.rs 里也没有此函数定义。
// 删除该 extern 声明，代码和说明保持一致——精确 glyph 几何由本文件内联的
// cpp! 块直接调用 QTextLine::cursorToX 完成，不经过 get_glyph_range_rect。
cpp! {{
    #include <QtGui/QTextLayout>
    #include <QtGui/QGlyphRun>
    #include <limits>
    extern QTextLayout* get_paragraph_layout(uint64_t gen, int slot);
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
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
    /// Issue #722 评论 5750218208: 真实视觉行 top（= `VisualLine.y`，文档坐标）。
    /// 行几何判断直接用此字段，不再从 cluster ink bounds 猜行高。
    pub visual_line_top: f64,
    /// Issue #722 评论 5750218208: 真实视觉行 bottom（= `VisualLine.y + VisualLine.height`，
    /// 文档坐标）。空行也有正确高度，不依赖 cluster。
    pub visual_line_bottom: f64,
    /// Issue #826 评论 42: 本视觉行**真实 drawn caret** 的文档坐标 top。
    ///
    /// `visual_line_top` 是 `VisualLine.y`（QTextLine 行顶），但 caret 并不贴行顶：
    /// `cursor_rect_for_line()` 里 `top = line.y + top_padding`，`top_padding =
    /// (line.height - (ascent + descent)) / 2`，项目默认字号/行距下不为 0。
    /// 跨行协同的 Boundary 段 y 必须用这个值，否则光标跨行会先上跳几像素、
    /// 沿文字走、再下跳回正常高度。
    pub caret_top: f64,
    /// Issue #826 评论 42: 本视觉行真实 caret 高度（与 `caret_top` 同源）。
    pub caret_height: f64,
}

impl PreparedLineSnapshot {
    /// Issue #826: 测试用最小构造器。
    ///
    /// `visual_line_top` / `visual_line_bottom` 由调用方给的 `top` + 固定行高推出，
    /// 供布局和 cluster 视觉几何判断使用。
    #[cfg(test)]
    pub(crate) fn stub_for_tests(
        visual_line_id: usize,
        top: f64,
        byte_start: usize,
        clusters: Vec<LineClusterSnapshot>,
    ) -> Self {
        let byte_end = clusters
            .iter()
            .map(|c| c.byte_end)
            .max()
            .unwrap_or(byte_start);
        let visual_x = clusters
            .iter()
            .map(|c| c.source_rect.x)
            .fold(f64::MAX, f64::min);
        Self {
            id: LineSnapshotId::new(0, 0, visual_line_id as u32),
            image: None,
            clusters,
            document_origin_y: top,
            dpr: 1.0,
            byte_start,
            byte_end,
            visual_x: if visual_x.is_finite() { visual_x } else { 0.0 },
            visual_line_top: top,
            visual_line_bottom: top + 20.0,
            // 测试 stub 没有真实字体度量：caret_top 退化为行顶（与旧行为一致，
            // 现有 synthetic 测试不受影响）。
            caret_top: top,
            caret_height: 20.0,
        }
    }

    /// Issue #826 评论 16：给测试行挂一张真实行图。
    ///
    /// `stub_for_tests` 固定写 `image: None`，纹理类测试拿不到可验证的 QImage ——
    /// 那样只能验证「source_lines 里带了正确的 snapshot_id」，验证不了
    /// `prepare_visual_edit_textures` 真的把它插进了 `TextureCache`。
    #[cfg(test)]
    pub(crate) fn with_test_image(mut self, image: qmetaobject::QImage) -> Self {
        self.image = Some(image);
        self
    }
}

impl PreparedLineSnapshot {
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

    /// Issue #826 评论 24：与 `byte_start..byte_end` 有**重叠**的 cluster。
    ///
    /// 这只是「哪些 cluster 碰到了这段逻辑范围」的查询，**不能**直接当成
    /// 「这些 cluster 属于这段范围」—— Qt shaping 的 cluster 本来就可能覆盖多个
    /// 字符（fi 连字、e + 组合音标、emoji ZWJ），重叠查询会把整块 cluster 一起
    /// 带出来。动画层要按 cluster 归属时必须用下面两个语义更明确的入口。
    pub fn clusters_overlapping_range(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<&LineClusterSnapshot> {
        self.clusters
            .iter()
            .filter(|c| c.byte_end > byte_start && c.byte_start < byte_end)
            .collect()
    }

    /// Issue #826 评论 24：被 `byte_start..byte_end` **完整包含**的 cluster。
    ///
    /// 视觉 owner 只认这种。若逻辑改动只覆盖某个 cluster 的一部分，cluster 仍是
    /// 不可拆的绘制单元，整块由 `VisualEditState` 做视觉交接，不能按 byte 比例裁切。
    pub fn clusters_contained_in_range(
        &self,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<&LineClusterSnapshot> {
        self.clusters
            .iter()
            .filter(|c| c.byte_start >= byte_start && c.byte_end <= byte_end)
            .collect()
    }

    /// Issue #826 评论 24：**恰好**等于 `range` 的那一个 cluster。
    ///
    /// 找不到就返回 `None`。调用方绝不能拿一块「看起来能用」的更大 cluster 去代表
    /// 一个子范围 —— 那正是评论 24 的根因：`f` 已经被最新 shaping 合进 `fi`，
    /// 却还拿整块 `fi` 当 `f` 的视觉资源，于是同一个 cluster 同时被 carry 和
    /// scalar Reveal 控制。
    pub fn cluster_exact_for_range(&self, range: (usize, usize)) -> Option<&LineClusterSnapshot> {
        self.clusters
            .iter()
            .find(|c| (c.byte_start, c.byte_end) == range)
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
        }
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
mod tests;
