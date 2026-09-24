// ── Qt 文本布局模块：纯数据类型 ──
//
// 坐标空间约定：
// - Qt 层（本模块）：QChar index（UTF-16 code unit），与 QTextLayout/QTextLine API 一致
// - Core 层：UTF-8 byte offset
// - 转换入口：`sujian_editor_item` 中的 `utf8_to_utf16` / `utf16_to_utf8`
//   在调用本模块函数前完成坐标转换

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

/// 视觉行 — 排版后的单行文本，同时持有 UTF-8 byte offset 和 QChar (UTF-16) offset。
///
/// 坐标空间：x/y/width/height 为文档坐标系（不含 scroll offset）。
/// byte_start/byte_end 为半开区间 [byte_start, byte_end)（UTF-8 byte offset，文档级）。
/// qchar_start/qchar_end 为半开区间 [qchar_start, qchar_end)（QChar index，文档级）。
///
/// 段落相关字段（para_text, para_start, para_qchar_start/end, qtextline_idx,
/// line_wrap_width, line_indent_x, para_indent, x_end_trailing）用于
/// 重新调用 QTextLayout API 做精确光标定位和 hit test。
#[derive(Clone, Debug, PartialEq)]
pub struct VisualLine {
    pub id: usize,
    /// Byte offset of the line start in the full document text (UTF-8).
    pub byte_start: usize,
    /// Byte offset of the line end in the full document text (UTF-8).
    pub byte_end: usize,
    /// QChar (UTF-16 code unit) offset of the line start in the full document text.
    pub qchar_start: usize,
    /// QChar (UTF-16 code unit) offset of the line end in the full document text.
    pub qchar_end: usize,
    pub hard_break: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub para_text: String,
    pub para_start: usize,
    pub qtextline_idx: i32,
    pub para_qchar_start: usize,
    pub para_qchar_end: usize,
    pub line_wrap_width: f64,
    pub line_indent_x: f64,
    pub para_indent: f64,
    pub x_end_trailing: f64,
    pub qt_ascent: f64,
    pub qt_descent: f64,
    /// Issue #658: 该视觉行所属段落在 g_paragraph_layout_cache 中的稳定 slot 索引。
    /// 按段落在文档中出现的顺序（含空段落）从 0 递增分配，与 cache slot 一一对应。
    /// scene_graph_renderer 直接消费此字段，不再自行计数 cache_idx。
    pub cache_slot: i32,
}

/// 光标矩形 — 文档坐标系（不含 scroll offset）。
/// visible=false 表示光标在可视区域外，平台端不应绘制。
/// baseline_y 是文字基线 Y 坐标（文档坐标系），从 QTextLine 的真实 ascent/descent
/// 计算，不使用 `top + h * 0.8` 估算。
#[derive(Clone, Debug, PartialEq)]
pub struct CaretRect {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub visual_line_id: usize,
    pub visible: bool,
    pub baseline_y: f64,
}

pub type CursorLayoutRect = CaretRect;

#[derive(Clone, Debug)]
pub struct LayoutParams {
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
}

/// 布局快照 — 某次排版结果的完整快照，与特定 text_revision 绑定。
/// text_ptr/text_len 用于快速判断文本缓冲区是否变更（指针+长度双重校验）。
/// 缓存失效条件：revision、指针、长度、宽度、字号、字体、行距、缩进或内边距任一变化。
#[derive(Clone, Debug)]
pub struct LayoutSnapshot {
    pub text_revision: u64,
    pub text_ptr: usize,
    pub text_len: usize,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    pub lines: Vec<VisualLine>,
    /// Issue #658 评论 5620035970 问题 2: 布局 generation — 标识本 snapshot
    /// 对应的 g_layout_generations 中的代。渲染时用 (generation, cache_slot) 查找 layout。
    pub layout_generation: u64,
}

/// Issue #658 评论 5622829886 问题 1: 已排好的 prepared layout 提升为 EditorLayout current。
///
/// 由 record_visual_transaction 全篇排版 new text 后构造，
/// 交给 EditorLayout::promote_prepared_layout 提升为 current，
/// 后续 EditorLayout::snapshot 发现 cache 有效直接返回，不再重新排版。
/// generation 对应的 QTextLayout 已存入 g_layout_generations[generation]，
/// 由 EditorLayout 生命周期管理（invalidate/snapshot 失效时 clear）。
/// text_revision / text_ptr / text_len 在提升时从当前 buffer.text 和
/// pipeline.text_revision() 获取，确保与 EditorLayout::snapshot 的 cache
/// 有效性检查一致。
#[derive(Clone)]
pub struct PromotedLayout {
    pub generation: u64,
    pub visual_lines: Vec<VisualLine>,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    /// Issue #658 评论 5624570557 问题 1: 旧的 generation，在 promote 完成后释放。
    /// old 动画纹理提取完成后，等 new prepared layout 真正成为 current，再释放旧 generation。
    pub old_generation: u64,
}

/// Issue #658 评论 5624570557 问题 1: 已准备布局的只读句柄。
///
/// 暴露当前有效 `layout_generation + LayoutSnapshot` 的只读句柄，
/// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
/// 句柄持有者必须保证 generation 在句柄使用期间有效（不被 clear_layout_generation 释放）。
pub struct PreparedLayoutHandle<'a> {
    pub generation: u64,
    pub lines: &'a [VisualLine],
}
