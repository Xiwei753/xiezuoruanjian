use cpp::cpp;

// Issue #810 评论 5932233052 问题1: qchar_offset_to_byte_offset 和 QString 不再在此
// 模块使用（prepare_animation_visuals_from_layout 改为 raster-only，不再做 QChar→byte
// 转换，也不再从 C++ buffer 读取 cluster raw_font_fingerprint 返回 QString）。
use super::engine::{get_font_ascent, get_font_descent, prepare_paragraph_visual_snapshot};
use super::types::{CaretAffinity, CaretRect, VisualLine};

// ── Qt 文本布局模块：Canonical document visual snapshot ──

#[derive(Clone, Debug)]
pub struct CanonicalClusterSnapshot {
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub source_rect_x: f64,
    pub source_rect_y: f64,
    pub source_rect_w: f64,
    pub source_rect_h: f64,
    pub glyph_count: usize,
    pub raw_font_fingerprint: String,
    pub is_rtl: bool,
    pub first_glyph_index: u32,
    pub cluster_text: String,
}

#[derive(Clone)]
pub struct CursorXMapEntry {
    pub qchar_pos: usize,
    pub x_leading: f64,
    pub x_trailing: f64,
}

#[derive(Clone)]
pub struct CanonicalLineSnapshot {
    pub qchar_start: usize,
    pub qchar_end: usize,
    pub document_byte_start: usize,
    pub document_byte_end: usize,
    pub x_pos: f64,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub x_end_trailing: f64,
    pub image: Option<qmetaobject::QImage>,
    pub clusters: Vec<CanonicalClusterSnapshot>,
    pub cursor_x_map: Vec<CursorXMapEntry>,
    // Issue #785 评论 5857873894 修改 2a: 稳定行身份字段。
    // prepare_animation_visuals_from_layout 按 (generation, cache_slot, qtextline_idx)
    // 从 QTextLayout 提取动画视觉，但 inject_animation_visuals_into_snapshot 之前只按
    // document_byte_start 猜目标行，找不到就静默跳过。新增段落文档起点 + 段落内
    // qtextline_idx 作为稳定行身份，inject 时按同一身份精确匹配，避免行身份漂移。
    // paragraph_document_byte_start = 该行所属段落在文档中的 byte 起始偏移。
    pub paragraph_document_byte_start: usize,
    // qtextline_idx = 该视觉行在所属段落 QTextLayout 中的 line index。
    pub qtextline_idx: i32,
}

#[derive(Clone)]
pub struct CanonicalParagraphSnapshot {
    pub paragraph_text: String,
    pub paragraph_document_byte_start: usize,
    pub lines: Vec<CanonicalLineSnapshot>,
    pub index_map: crate::editor::paragraph_index_map::ParagraphIndexMap,
}

/// Issue #810 评论 5932233052 问题1: 动画 raster 视觉 — 只携带 QImage 和稳定行身份，
/// 不携带 cluster。
///
/// 从类型上断掉"动画提取复用 CanonicalLineSnapshot 顺手把 cluster 填回去"的路。
/// cluster 几何现在由基础 canonical 排版直接产出（engine.rs 中 cluster 提取始终执行），
/// `prepare_animation_visuals_from_layout` 只负责提取可延迟的 QImage/纹理，
/// `inject_animation_visuals_into_snapshot` 只注入 image，不再覆盖 cluster。
///
/// `paragraph_document_byte_start` + `qtextline_idx` 是稳定行身份，用于 inject 时
/// 精确匹配目标行（与 `CanonicalLineSnapshot` 同源）。
#[derive(Clone)]
pub struct AnimationRasterVisual {
    pub image: Option<qmetaobject::QImage>,
    pub paragraph_document_byte_start: usize,
    pub qtextline_idx: i32,
}

#[derive(Clone)]
pub struct CanonicalDocumentVisualSnapshot {
    pub text_revision: u64,
    pub font_size: f64,
    pub font_family: String,
    pub line_spacing: f64,
    pub text_indent: f64,
    pub padding: f64,
    pub width: f64,
    pub dpr: f64,
    pub paragraphs: Vec<CanonicalParagraphSnapshot>,
    pub visual_lines: Vec<VisualLine>,
}

impl CanonicalDocumentVisualSnapshot {
    pub fn cursor_rect(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        let line = self
            .visual_lines
            .iter()
            .enumerate()
            .find(|(idx, _)| {
                super::hit_test::line_contains_cursor_with_affinity(
                    &self.visual_lines,
                    *idx,
                    cursor_byte,
                    affinity,
                )
            })
            .map(|(_, line)| line)
            .or_else(|| self.visual_lines.last());

        let fallback;
        let line = match line {
            Some(line) => line,
            None => {
                fallback = VisualLine {
                    id: 0,
                    byte_start: 0,
                    byte_end: 0,
                    qchar_start: 0,
                    qchar_end: 0,
                    hard_break: true,
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: self.font_size * self.line_spacing,
                    para_text: String::new(),
                    para_start: 0,
                    qtextline_idx: 0,
                    para_qchar_start: 0,
                    para_qchar_end: 0,
                    line_wrap_width: 0.0,
                    line_indent_x: 0.0,
                    para_indent: 0.0,
                    x_end_trailing: 0.0,
                    qt_ascent: 0.0,
                    qt_descent: 0.0,
                    cache_slot: 0,
                };
                &fallback
            }
        };

        let cursor_x = self.cursor_x_from_canonical(line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) =
            super::engine::cursor_rect_for_line(line, self.font_size, &self.font_family);
        let cursor_y = cursor_y_doc - scroll_y;
        let visible = cursor_y + cursor_h > 0.0 && cursor_y < viewport_h.max(1.0);

        // Issue #712: baseline_y 从 QTextLine 的真实 ascent/descent 计算。
        let baseline_y_doc =
            super::engine::text_baseline_y(line, self.font_size, &self.font_family);
        let baseline_y = baseline_y_doc - scroll_y;

        CaretRect {
            x: cursor_x,
            y: cursor_y,
            h: cursor_h,
            visual_line_id: line.id,
            visible,
            baseline_y,
        }
    }

    /// Issue #722 评论 5748596920 问题1: canonical caret 文档坐标入口。
    ///
    /// 与 `cursor_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标（不减 scroll_y），
    /// `visible` 始终为 true。供正文事务 caret track 使用，使 caret track 与
    /// AnimatedSlice/StaticPatch 文档坐标系一致。
    pub fn cursor_rect_doc(&self, cursor_byte: usize, affinity: CaretAffinity) -> CaretRect {
        let line = self
            .visual_lines
            .iter()
            .enumerate()
            .find(|(idx, _)| {
                super::hit_test::line_contains_cursor_with_affinity(
                    &self.visual_lines,
                    *idx,
                    cursor_byte,
                    affinity,
                )
            })
            .map(|(_, line)| line)
            .or_else(|| self.visual_lines.last());

        let fallback;
        let line = match line {
            Some(line) => line,
            None => {
                fallback = VisualLine {
                    id: 0,
                    byte_start: 0,
                    byte_end: 0,
                    qchar_start: 0,
                    qchar_end: 0,
                    hard_break: true,
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: self.font_size * self.line_spacing,
                    para_text: String::new(),
                    para_start: 0,
                    qtextline_idx: 0,
                    para_qchar_start: 0,
                    para_qchar_end: 0,
                    line_wrap_width: 0.0,
                    line_indent_x: 0.0,
                    para_indent: 0.0,
                    x_end_trailing: 0.0,
                    qt_ascent: 0.0,
                    qt_descent: 0.0,
                    cache_slot: 0,
                };
                &fallback
            }
        };

        let cursor_x = self.cursor_x_from_canonical(line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) =
            super::engine::cursor_rect_for_line(line, self.font_size, &self.font_family);
        let baseline_y_doc =
            super::engine::text_baseline_y(line, self.font_size, &self.font_family);

        CaretRect {
            x: cursor_x,
            y: cursor_y_doc,
            h: cursor_h,
            visual_line_id: line.id,
            visible: true,
            baseline_y: baseline_y_doc,
        }
    }

    fn cursor_x_from_canonical(
        &self,
        line: &VisualLine,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        if line.para_text.is_empty() {
            if line.width > 0.0 && cursor_byte == line.byte_end {
                return line.x + line.width;
            }
            return line.x;
        }

        let cursor_in_para = cursor_byte.saturating_sub(line.para_start);
        let para = match self.paragraphs.iter().find(|p| {
            p.paragraph_document_byte_start <= cursor_byte
                && cursor_byte <= p.paragraph_document_byte_start + p.paragraph_text.len()
        }) {
            Some(p) => p,
            None => return line.x,
        };

        let cursor_qchar =
            super::engine::byte_offset_to_qchar_offset(&para.paragraph_text, cursor_in_para);

        // Issue #722 评论 5747719529 改法 3: 软换行边界必须使用已经选中的那条
        // QTextLine，不要用包含区间 find。软换行边界同时等于上一行 end 和下一行
        // start，包含区间 find 会先命中上一行，再把上一行行尾 X 套到下一行 Y。
        // 直接使用已选中的 line.qtextline_idx：从对应 paragraph 的
        // canonical.lines[line.qtextline_idx as usize] 取 cursor_x_map，再按
        // CaretAffinity::Leading/Trailing 取这一条实际视觉行上的 X。静态布局路径
        // 本来就是按 qtextline_idx + QTextLine::cursorToX() 算，canonical 动画路径
        // 必须和它完全一致。
        let canonical_line: Option<&CanonicalLineSnapshot> =
            if line.qtextline_idx >= 0 && (line.qtextline_idx as usize) < para.lines.len() {
                // explicit_line: 直接用已选中的视觉行的 qtextline_idx 索引 canonical lines。
                let indexed_line = &para.lines[line.qtextline_idx as usize];
                Some(indexed_line)
            } else {
                // qtextline_idx 无效时回退到按 qchar 范围匹配（半开区间，避免软换行
                // 边界同时命中两行）。只在 qtextline_idx 未被正确设置时才会走到这里。
                para.lines
                    .iter()
                    .find(|cl| cl.qchar_start <= cursor_qchar && cursor_qchar < cl.qchar_end)
            };

        match canonical_line {
            Some(cl) => {
                let use_trailing =
                    affinity == CaretAffinity::Upstream && cursor_byte == line.byte_end;
                let entry = cl.cursor_x_map.iter().find(|m| m.qchar_pos == cursor_qchar);
                let x_in_line = match entry {
                    Some(m) => {
                        if use_trailing {
                            m.x_trailing
                        } else {
                            m.x_leading
                        }
                    }
                    None => {
                        if let Some(last) = cl.cursor_x_map.last() {
                            if use_trailing {
                                last.x_trailing
                            } else {
                                last.x_leading
                            }
                        } else {
                            0.0
                        }
                    }
                };
                let x = line.x + x_in_line;

                if x <= line.x + 0.5
                    && line.byte_start != line.byte_end
                    && affinity == CaretAffinity::Upstream
                    && cursor_byte == line.byte_end
                    && line.x_end_trailing > 0.0
                {
                    return line.x + line.x_end_trailing;
                }

                x
            }
            None => line.x,
        }
    }

    pub fn to_layout_snapshot(&self) -> super::types::LayoutSnapshot {
        super::types::LayoutSnapshot {
            text_revision: self.text_revision,
            text_ptr: 0,
            text_len: 0,
            width: self.width,
            font_size: self.font_size as f32,
            font_family: self.font_family.clone(),
            line_spacing: self.line_spacing as f32,
            text_indent: self.text_indent as f32,
            padding: self.padding as f32,
            lines: self.visual_lines.clone(),
            // Issue #658 评论 5620035970 问题 2: 动画路径不直接渲染此 snapshot，
            // generation 填 0 表示不用于 rebuild_text_node_from_paragraphs。
            layout_generation: 0,
        }
    }
}

// 本文件只留 snapshot 的数据结构与其构造方法；
//   - ffi.rs：thread_local 排版缓冲的取值 shim（image 物理尺寸）
//   - prepare.rs：五条 snapshot 组装入口 + 动画纹理注入
// prepare.rs 里的公开入口在这里重导出，`layout/mod.rs` 的 re-export 不用改。
// Issue #810 评论 5932233052 问题1: prepare_animation_visuals_from_layout 改为
// raster-only 后，ffi.rs 只保留 image 物理尺寸读取 shim，cluster/cursor_x_map
// 读取 shim 已删除（被新实现替代的旧入口直接删除，AGENTS.md）。
mod ffi;
mod prepare;

pub use prepare::{
    assemble_document_visual_snapshot_from_lines, inject_animation_visuals_into_snapshot,
    prepare_affected_paragraphs_visual_snapshot, prepare_animation_visuals_from_layout,
    prepare_document_visual_snapshot, prepare_document_visual_snapshot_scoped,
};
// Issue #810 评论 5932233052 问题1: AnimationRasterVisual 定义在本模块（pub struct），
// 外部通过 canonical_snapshot::AnimationRasterVisual 访问，无需额外 re-export。
