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
#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
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
    /// Issue #904 复核 8：产出该行的 **稳定 ParagraphId**（`VisualLine.paragraph_id`）。
    ///
    /// 与静态 QSG 的 `ParagraphLineInfo.paragraph_id`（`line.paragraph_id.as_u64()`）
    /// 完全同源，也是动画整体段落位移 `paragraph_y_offsets` 用的身份。
    /// `LineSnapshotId.paragraph_id` 是文档中的段落序号（`logical_slot`），两者职责
    /// 不同：前者找纹理，后者才标识"同一个段落"（增删换行后不会漂移）。
    pub stable_paragraph_id: u64,
    /// Issue #904 复核 9：**真实段首的文档绝对 y**（不是本行 y，也不是视口内
    /// 第一条可见行的 y）。来源是该版 canonical 的权威 `height_index`。
    ///
    /// 两种构建路线都会按视口裁掉屏幕外的行，所以"快照里第一行的
    /// `document_origin_y`"并不等于段首；长段落从屏幕上方延伸到视口时，
    /// 拿它当段落锚点会让整段位移的起点 y 取错。
    pub paragraph_top_document_y: f64,
    /// Issue #904 复核 9：该段落完整高度（同版 canonical `height_index`）。
    /// 判断段落是否与视口相交时必须用「段首 + 段高」的矩形，而不是段首之间的短竖条。
    pub paragraph_height: f64,
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
            // 测试 stub 里没有真实 ParagraphId：与 id 的段落序号保持一致。
            stable_paragraph_id: 0,
            paragraph_top_document_y: top,
            paragraph_height: 20.0,
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

/// Issue #904 复核 10：该版本**全部段落**的几何锚点（稳定 ParagraphId → 段首 y、段高）。
///
/// 与 `line_snapshots` 分离：`line_snapshots` 只为视口内行（以及本次编辑受影响的段落
/// 行）准备 QImage 与 cluster，会被按视口裁剪；段落整体位移的候选段落却可能**两端都
/// 不在** `line_snapshots` 里（屏幕外进入/离开）。锚点不含任何 QImage，只描述该版本
/// 每个段落的真实位置，因此可以完整保留任意稳定 ParagraphId 的 `(top_y, height)`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ParagraphAnchor {
    pub paragraph_id: u64,
    pub top_y: f64,
    pub height: f64,
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
    /// Issue #904 复核 10：该版本全部段落的几何锚点（按文档顺序）。
    ///
    /// 判断"屏幕外段落是否因编辑移动进/出视口"只允许用这份索引。绝不能用
    /// `line_snapshots` 里出现过的段落反推——那是视口裁剪后的子集，屏幕外段落
    /// 会整段缺席，导致跨视口的位移被漏掉。
    pub paragraph_anchors: Vec<ParagraphAnchor>,
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
            paragraph_anchors: Vec::new(),
            caret_rect,
            caret_rect_doc,
            caret_affinity,
        }
    }

    /// Issue #904 复核 10：该版本某个**稳定 ParagraphId** 的真实 `(段首 y, 段高)`。
    ///
    /// 与视口裁剪无关：即使该段落的全部行都在屏幕外，锚点仍然完整。
    pub fn paragraph_anchor(&self, paragraph_id: u64) -> Option<(f64, f64)> {
        self.paragraph_anchors
            .iter()
            .find(|a| a.paragraph_id == paragraph_id)
            .map(|a| (a.top_y, a.height))
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
