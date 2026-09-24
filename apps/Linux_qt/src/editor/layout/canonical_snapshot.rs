use cpp::cpp;
use qmetaobject::QString;

use super::engine::{
    get_font_ascent, get_font_descent, prepare_paragraph_visual_snapshot,
    qchar_offset_to_byte_offset,
};
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
}

#[derive(Clone)]
pub struct CanonicalParagraphSnapshot {
    pub paragraph_text: String,
    pub paragraph_document_byte_start: usize,
    pub lines: Vec<CanonicalLineSnapshot>,
    pub index_map: crate::editor::paragraph_index_map::ParagraphIndexMap,
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

// Helper functions to read from C++ buffers
fn get_canonical_line_qchar_start(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
        return 0;
    })
}

fn get_canonical_line_qchar_end(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
        return 0;
    })
}

fn get_canonical_line_x_pos(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xPos;
        return 0.0;
    })
}

fn get_canonical_line_width(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].width;
        return 0.0;
    })
}

fn get_canonical_line_ascent(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].ascent;
        return 0.0;
    })
}

fn get_canonical_line_descent(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].descent;
        return 0.0;
    })
}

fn get_canonical_line_x_end_trailing(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xEndTrailing;
        return 0.0;
    })
}

fn get_canonical_line_image_phys_w(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysW;
        return 0;
    })
}

fn get_canonical_line_image_phys_h(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysH;
        return 0;
    })
}

fn get_canonical_line_cluster_start(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterStartIndex;
        return 0;
    })
}

fn get_canonical_line_cluster_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterCount;
        return 0;
    })
}

fn get_canonical_line_cursor_x_map_start(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapStart;
        return 0;
    })
}

fn get_canonical_line_cursor_x_map_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapCount;
        return 0;
    })
}

fn get_canonical_cluster_qchar_start(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharStart);
        return 0;
    })
}

fn get_canonical_cluster_qchar_end(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharEnd);
        return 0;
    })
}

fn get_canonical_cluster_src_x(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectX;
        return 0.0;
    })
}

fn get_canonical_cluster_src_y(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectY;
        return 0.0;
    })
}

fn get_canonical_cluster_src_w(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectW;
        return 0.0;
    })
}

fn get_canonical_cluster_src_h(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectH;
        return 0.0;
    })
}

fn get_canonical_cluster_glyph_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].glyphCount;
        return 0;
    })
}

fn get_canonical_cluster_raw_font(idx: i32) -> QString {
    cpp::cpp!(unsafe [idx as "int"] -> QString as "QString" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return QString::fromUtf8(g_canonical_cluster_buf[idx].rawFontFingerprint);
        return QString();
    })
}

fn get_canonical_cluster_is_rtl(idx: i32) -> bool {
    cpp::cpp!(unsafe [idx as "int"] -> bool as "bool" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].isRTL;
        return false;
    })
}

fn get_canonical_cluster_first_glyph(idx: i32) -> u32 {
    cpp::cpp!(unsafe [idx as "int"] -> u32 as "quint32" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].firstGlyphIndex;
        return 0;
    })
}

fn get_cursor_x_map_qchar(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return static_cast<qulonglong>(g_cursor_x_map_buf[idx].qcharPos);
        return 0;
    })
}

fn get_cursor_x_map_x_leading(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xLeading;
        return 0.0;
    })
}

fn get_cursor_x_map_x_trailing(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xTrailing;
        return 0.0;
    })
}

/// Issue #658 评论 5624570557 问题 1+2: 从已有 generation 的 QTextLine 提取动画视觉资源（含 clusters）。
/// 按 (generation, cache_slot, qtextline_idx) 读取现成 QTextLine，
/// 不重新排版，直接 line.draw() 到 QImage 并提取 glyphRuns/clusters。
/// 返回完整的 CanonicalLineSnapshot 列表。
///
/// Issue #658 评论 5625515748 问题 1: 移除统一的 `paragraph_text` / `paragraph_document_byte_start`
/// 参数，改为从每个 VisualLine 自身的 `para_text` / `para_start` 取段落级文本与文档起点。
/// qchar_start/qchar_end 来自各段落自己的 QTextLayout，是段落内 QChar offset，
/// 必须用对应段落的 para_text 做 QChar→byte 转换，再加该段落的 para_start 得到文档级 byte range。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
pub fn prepare_animation_visuals_from_layout(
    handle: &super::types::PreparedLayoutHandle<'_>,
    line_ids: &[usize],
    dpr: f64,
    text_color: &str,
) -> Vec<CanonicalLineSnapshot> {
    let color = qmetaobject::QColor::from_name(text_color);
    let mut snapshots = Vec::new();

    for &line_id in line_ids {
        if line_id >= handle.lines.len() {
            continue;
        }
        let line = &handle.lines[line_id];
        let slot = line.cache_slot;
        let qtextline_idx = line.qtextline_idx;
        let gen = handle.generation;
        // Issue #658 评论 5625515748 问题 1: 每行用自己的 para_text/para_start 做
        // QChar→byte 转换。qchar_start/qchar_end 是段落内 QChar offset，
        // 必须对应该段落的 para_text，再加 para_start 得到文档级 byte offset。
        let para_text: &str = &line.para_text;
        let para_start = line.para_start;

        // SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
        let success = cpp::cpp!(unsafe [
            gen as "uint64_t",
            slot as "int",
            qtextline_idx as "int",
            dpr as "double",
            color as "QColor"
        ] -> bool as "bool" {
            extract_animation_visuals_from_existing_line(gen, slot, qtextline_idx, dpr, color);
            return !g_canonical_line_buf.empty();
        });

        if !success {
            continue;
        }

        // 从 C++ buffers 中读取提取的数据
        let qchar_start = get_canonical_line_qchar_start(0);
        let qchar_end = get_canonical_line_qchar_end(0);
        let x_pos = get_canonical_line_x_pos(0);
        let width = get_canonical_line_width(0);
        let ascent = get_canonical_line_ascent(0);
        let descent = get_canonical_line_descent(0);
        let x_end_trailing = get_canonical_line_x_end_trailing(0);
        let image_phys_w = get_canonical_line_image_phys_w(0);
        let image_phys_h = get_canonical_line_image_phys_h(0);
        let cluster_start = get_canonical_line_cluster_start(0);
        let cluster_count = get_canonical_line_cluster_count(0);
        let cursor_x_map_start = get_canonical_line_cursor_x_map_start(0);
        let cursor_x_map_count = get_canonical_line_cursor_x_map_count(0);

        // 提取 image
        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize {
                    width: 1,
                    height: 1,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
            cpp::cpp!(unsafe [img_ptr as "QImage*"] {
                if (!g_canonical_line_images.empty()) {
                    *img_ptr = g_canonical_line_images[0];
                }
            });
            Some(img)
        } else {
            None
        };

        // 提取 clusters
        let mut clusters = Vec::with_capacity(cluster_count as usize);
        for ci in 0..cluster_count {
            let cidx: i32 = (cluster_start as i32) + ci as i32;
            let c_qchar_start = get_canonical_cluster_qchar_start(cidx);
            let c_qchar_end = get_canonical_cluster_qchar_end(cidx);
            let c_src_x = get_canonical_cluster_src_x(cidx);
            let c_src_y = get_canonical_cluster_src_y(cidx);
            let c_src_w = get_canonical_cluster_src_w(cidx);
            let c_src_h = get_canonical_cluster_src_h(cidx);
            let c_glyph_count = get_canonical_cluster_glyph_count(cidx);
            let c_raw_font = get_canonical_cluster_raw_font(cidx);
            let c_is_rtl = get_canonical_cluster_is_rtl(cidx);
            let c_first_glyph = get_canonical_cluster_first_glyph(cidx);

            let doc_byte_start = qchar_offset_to_byte_offset(para_text, c_qchar_start) + para_start;
            let doc_byte_end = qchar_offset_to_byte_offset(para_text, c_qchar_end) + para_start;

            let (c_byte_start, c_byte_end) = (
                qchar_offset_to_byte_offset(para_text, c_qchar_start),
                qchar_offset_to_byte_offset(para_text, c_qchar_end),
            );
            let c_cluster_text = if c_byte_start <= c_byte_end && c_byte_end <= para_text.len() {
                para_text[c_byte_start..c_byte_end].to_string()
            } else {
                String::new()
            };

            clusters.push(CanonicalClusterSnapshot {
                document_byte_start: doc_byte_start,
                document_byte_end: doc_byte_end,
                source_rect_x: c_src_x,
                source_rect_y: c_src_y,
                source_rect_w: c_src_w,
                source_rect_h: c_src_h,
                glyph_count: c_glyph_count as usize,
                raw_font_fingerprint: c_raw_font.to_string(),
                is_rtl: c_is_rtl,
                first_glyph_index: c_first_glyph,
                cluster_text: c_cluster_text,
            });
        }

        // 提取 cursor_x_map
        let mut cursor_x_map = Vec::with_capacity(cursor_x_map_count as usize);
        for mi in 0..cursor_x_map_count {
            let midx: i32 = (cursor_x_map_start as i32) + mi as i32;
            let m_qchar = get_cursor_x_map_qchar(midx);
            let m_x_leading = get_cursor_x_map_x_leading(midx);
            let m_x_trailing = get_cursor_x_map_x_trailing(midx);
            cursor_x_map.push(CursorXMapEntry {
                qchar_pos: m_qchar,
                x_leading: m_x_leading,
                x_trailing: m_x_trailing,
            });
        }

        snapshots.push(CanonicalLineSnapshot {
            qchar_start,
            qchar_end,
            document_byte_start: qchar_offset_to_byte_offset(para_text, qchar_start) + para_start,
            document_byte_end: qchar_offset_to_byte_offset(para_text, qchar_end) + para_start,
            x_pos,
            width,
            ascent,
            descent,
            x_end_trailing,
            image,
            clusters,
            cursor_x_map,
        });
    }

    snapshots
}

pub fn prepare_document_visual_snapshot(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5623746506 问题 2a: 收口到受影响范围生成动画视觉。
    // affected_byte_start >= affected_byte_end 表示全篇生成（保持原语义）。
    // Issue #688: 静态布局路径不接收颜色参数；只有 generate_animation_visuals=true 时才需要
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        None,
        generation,
        generate_animation_visuals,
        0,
        0,
    )
}

/// Issue #658 评论 5623746506 问题 2a: 按受影响字节范围生成动画视觉的排版入口。
///
/// 与 `prepare_document_visual_snapshot` 相同，但只有与 `[affected_byte_start,
/// affected_byte_end)` 有交集的段落才以 `generate_animation_visuals=true` 排版
/// （生成 QImage/glyphRuns/cluster）；其他段落只做基础排版
/// （`generate_animation_visuals=false`），保留 QTextLayout/VisualLine 供静态
/// QSGTextNode 消费。基础 canonical 排版仍一次生成完整 new text 的所有段落。
///
/// Issue #658 评论 5624570557 问题 2: 分离基础排版与动画视觉生成。
/// 本函数只做基础排版（QTextLayout + VisualLine + cursor map），不生成 QImage。
/// 调用方需随后调用 `prepare_animation_visuals_from_layout` 对受影响行提取动画资源。
pub fn prepare_document_visual_snapshot_scoped(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    text_color: Option<&str>,
    generation: u64,
    affected_byte_start: usize,
    affected_byte_end: usize,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5624570557 问题 2: 基础排版不生成动画视觉，
    // 只得到 QTextLayout/VisualLine/cursor 几何。
    // 动画视觉由 prepare_animation_visuals_from_layout 单独提取。
    // Issue #688: 静态布局路径传 None；动画路径传 Some(text_color)
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        text_color,
        generation,
        false,
        affected_byte_start,
        affected_byte_end,
    )
}

fn prepare_document_visual_snapshot_impl(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    text_color: Option<&str>,
    generation: u64,
    generate_animation_visuals: bool,
    affected_byte_start: usize,
    affected_byte_end: usize,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
    // 由调用方在批次开始前分配独立 generation，本函数用 generation 写对应代的 cache，
    // 与静态正文路径互不干扰。
    // Issue #658 评论 5623746506 问题 2a: affected_byte_start < affected_byte_end
    // 时只对受影响段落生成 QImage/glyph/cluster；>= 时全篇生成（原语义）。
    // Issue #688: 静态布局路径不接收颜色参数
    let scoped_animation = generate_animation_visuals && affected_byte_start < affected_byte_end;

    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);

    let mut paragraphs = Vec::new();
    let mut visual_lines = Vec::new();
    let mut y: f64 = padding;
    let mut paragraph_start: usize = 0;
    let mut paragraph_qchar_start: usize = 0;
    let mut line_id: usize = 0;
    let mut paragraph_idx: i32 = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #658: 空段落也调用 prepare_paragraph_visual_snapshot 占 null slot，
            // 保持 cache_slot 与文档段落一一对应。
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_start,
                font_size,
                font_family,
                available,
                indent,
                dpr,
                text_color,
                paragraph_idx,
                line_spacing,
                generation,
                generate_animation_visuals,
            );
            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: paragraph_start,
                byte_end: paragraph_start,
                qchar_start: paragraph_qchar_start,
                qchar_end: paragraph_qchar_start,
                hard_break,
                x: padding + indent,
                y,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_start,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
                para_indent: indent,
                x_end_trailing: 0.0,
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
                cache_slot: paragraph_idx,
            });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    paragraph_start,
                ),
            });

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            paragraph_idx += 1;
            continue;
        }

        let canonical = prepare_paragraph_visual_snapshot(
            paragraph_text,
            paragraph_start,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            paragraph_idx,
            line_spacing,
            generation,
            // Issue #658 评论 5623746506 问题 2a: scoped_animation 时只对受影响段落
            // 生成 QImage/glyph/cluster；其他段落只做基础排版。
            if scoped_animation {
                let para_end = paragraph_start + paragraph.len();
                paragraph_start < affected_byte_end && para_end > affected_byte_start
            } else {
                generate_animation_visuals
            },
        );
        paragraph_idx += 1;

        for (line_idx, canonical_line) in canonical.lines.iter().enumerate() {
            let qt_metrics_h = canonical_line.ascent + canonical_line.descent;
            let actual_line_h = if qt_metrics_h > 0.0 {
                line_height.max(qt_metrics_h)
            } else {
                line_height
            };

            let is_first = line_idx == 0;

            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: canonical_line.document_byte_start,
                byte_end: canonical_line.document_byte_end,
                qchar_start: canonical_line.qchar_start + paragraph_qchar_start,
                qchar_end: canonical_line.qchar_end + paragraph_qchar_start,
                hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                x: padding + canonical_line.x_pos,
                y,
                width: canonical_line.width,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start: paragraph_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: canonical_line.qchar_start,
                para_qchar_end: canonical_line.qchar_end,
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                cache_slot: paragraph_idx - 1,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        paragraphs.push(canonical);
    }

    // Issue #658 评论 5622188166 问题 2: 空文本处理，与 layout_lines 行为一致。
    // "".split_inclusive('\n') 返回空迭代器，需在此补充一个空段 VisualLine，
    // 保证 snapshot.lines 非空，让 hit_test/caret_rect 有行可操作。
    if text.is_empty() {
        let empty_ascent = get_font_ascent(font_family, font_size as f32);
        let empty_descent = get_font_descent(font_family, font_size as f32);
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            0,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            0,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
        visual_lines.push(VisualLine {
            id: line_id,
            byte_start: 0,
            byte_end: 0,
            qchar_start: 0,
            qchar_end: 0,
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: 0,
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
            para_indent: indent,
            x_end_trailing: 0.0,
            qt_ascent: empty_ascent,
            qt_descent: empty_descent,
            cache_slot: 0,
        });
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: String::new(),
            paragraph_document_byte_start: 0,
            lines: Vec::new(),
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", 0),
        });
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        // Issue #658: 尾部换行产生的空段也占 null slot。
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            text.len(),
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            paragraph_idx,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
        visual_lines.push(VisualLine {
            id: line_id,
            byte_start: text.len(),
            byte_end: text.len(),
            qchar_start: text_qchar_len,
            qchar_end: text_qchar_len,
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: text.len(),
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
            para_indent: indent,
            x_end_trailing: 0.0,
            qt_ascent: 0.0,
            qt_descent: 0.0,
            cache_slot: paragraph_idx,
        });
        // paragraph_idx 递增省略：尾部换行分支后不再使用。
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        paragraphs,
        visual_lines,
    }
}

/// Issue #658 评论 5625515748 问题 2: 只从已有 VisualLine 组装 Rust 数据构造
/// `CanonicalDocumentVisualSnapshot`，不调用任何 QTextLayout/beginLayout/createLine。
///
/// 与 `prepare_document_visual_snapshot` 的区别：本函数不重新排版，直接把 `lines` 中
/// 每个 `VisualLine` 携带的 Rust 几何数据（byte_start/byte_end/qchar/x/y/width/height
/// /ascent/descent 等）填入 `visual_lines` 和对应段落的 `CanonicalLineSnapshot`。
/// `image` / `clusters` / `cursor_x_map` 初始为空，随后由
/// `inject_animation_visuals_into_snapshot` 填充 image/clusters。
///
/// 用途：动画 old 帧从已有 prepared layout 的 VisualLine 组装 old_doc_snapshot，
/// 避免对 old text 重新排版。`cursor_rect` 依赖 `visual_lines` 的 Rust 几何数据
/// （y/height/font_size/font_family）计算 cursor_y/h 和 baseline；`cursor_x`
/// 在 `cursor_x_map` 为空时退化为 `line.x`（行首），对 old 动画起点可接受。
///
/// `padding` 用于从 `VisualLine.x`（含 padding）还原 `CanonicalLineSnapshot.x_pos`
/// （不含 padding）：`x_pos = line.x - padding`（与 `prepare_document_visual_snapshot_impl`
/// line 3320 `x = padding + canonical_line.x_pos` 对应）。
pub fn assemble_document_visual_snapshot_from_lines(
    lines: &[VisualLine],
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    text_indent: f64,
    padding: f64,
    width: f64,
    dpr: f64,
) -> CanonicalDocumentVisualSnapshot {
    let mut paragraphs: Vec<CanonicalParagraphSnapshot> = Vec::new();

    for line in lines {
        // 空段落（para_text 为空）不构造 CanonicalLineSnapshot：
        // build_from_canonical_document 对空段落直接跳过（line_snapshot_builder.rs:42-69），
        // 且 prepare_animation_visuals_from_layout 对空段落不会产生 anim_line。
        if line.para_text.is_empty() {
            continue;
        }

        // 按 para_start 找到或创建对应 CanonicalParagraphSnapshot。
        // paragraphs 顺序按首次出现的 para_start，与 prepare_document_visual_snapshot_impl
        // 按文档顺序遍历段落一致。
        let para_idx = if let Some(idx) = paragraphs
            .iter()
            .position(|p| p.paragraph_document_byte_start == line.para_start)
        {
            idx
        } else {
            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: line.para_text.clone(),
                paragraph_document_byte_start: line.para_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    &line.para_text,
                    line.para_start,
                ),
            });
            // 刚 push 成功，新索引 = len - 1，逻辑上一定 < len。
            paragraphs.len() - 1
        };

        let para = &mut paragraphs[para_idx];
        // x_pos = line.x - padding（与 prepare_document_visual_snapshot_impl
        // `x: padding + canonical_line.x_pos` 对应）。
        let x_pos = line.x - padding;
        para.lines.push(CanonicalLineSnapshot {
            // para_qchar_start/para_qchar_end 是段落内 QChar offset，
            // 与 prepare_paragraph_visual_snapshot 产生的 canonical_line.qchar_start/end 一致。
            qchar_start: line.para_qchar_start,
            qchar_end: line.para_qchar_end,
            // document_byte_start/end 直接用 VisualLine 的文档级 byte offset，
            // 与 prepare_animation_visuals_from_layout 产生的 anim_line.document_byte_start
            //（= qchar_offset_to_byte_offset(para_text, qchar_start) + para_start）一致，
            // 保证 inject_animation_visuals_into_snapshot 能按 document_byte_start 匹配注入。
            document_byte_start: line.byte_start,
            document_byte_end: line.byte_end,
            x_pos,
            width: line.width,
            ascent: line.qt_ascent,
            descent: line.qt_descent,
            x_end_trailing: line.x_end_trailing,
            // image/clusters 初始为空，由 inject_animation_visuals_into_snapshot 填充。
            image: None,
            clusters: Vec::new(),
            // cursor_x_map 为空：VisualLine 不携带 cursor_x_map（需 C++ QTextLayout 提取）。
            // cursor_x_from_canonical 在 cursor_x_map 为空时退化为 line.x（行首），
            // 对 old 动画起点可接受（动画主要看 new cursor 和 glyph rects）。
            cursor_x_map: Vec::new(),
        });
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent,
        padding,
        width,
        dpr,
        paragraphs,
        // visual_lines 直接 clone，保持与原 layout 一致的 Rust 几何数据。
        visual_lines: lines.to_vec(),
    }
}

pub fn prepare_affected_paragraphs_visual_snapshot(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    text_color: &str,
    affected_byte_start: usize,
    affected_byte_end: usize,
    previous_snapshot: Option<&CanonicalDocumentVisualSnapshot>,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalDocumentVisualSnapshot {
    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);

    let mut affected_para_indices: Vec<usize> = Vec::new();
    let paragraphs_split: Vec<&str> = text.split_inclusive('\n').collect();
    let total_paras = paragraphs_split.len();
    let mut para_start: usize = 0;
    for (para_idx, paragraph) in paragraphs_split.iter().enumerate() {
        let para_end = para_start + paragraph.len();
        if para_start < affected_byte_end && para_end > affected_byte_start {
            if !affected_para_indices.contains(&para_idx) {
                affected_para_indices.push(para_idx);
            }
            if para_idx > 0 && !affected_para_indices.contains(&(para_idx - 1)) {
                affected_para_indices.push(para_idx - 1);
            }
            if para_idx + 1 < total_paras && !affected_para_indices.contains(&(para_idx + 1)) {
                affected_para_indices.push(para_idx + 1);
            }
        }
        para_start = para_end;
    }

    if affected_para_indices.is_empty() {
        affected_para_indices.push(0);
    }

    let mut paragraphs = Vec::new();
    let mut visual_lines = Vec::new();
    let mut y: f64 = padding;
    let mut paragraph_start: usize = 0;
    let mut paragraph_qchar_start: usize = 0;
    let mut line_id: usize = 0;
    let mut current_para_idx: usize = 0;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        let is_affected = affected_para_indices.contains(&current_para_idx);

        if !is_affected {
            let mut reused_from_prev = false;
            if let Some(prev) = previous_snapshot {
                let prev_para_idx = if current_para_idx < prev.paragraphs.len()
                    && prev.paragraphs[current_para_idx].paragraph_text == paragraph_text
                {
                    Some(current_para_idx)
                } else {
                    None
                };
                let prev_lines: Vec<VisualLine> = if let Some(pidx) = prev_para_idx {
                    let prev_p = &prev.paragraphs[pidx];
                    let prev_para_byte_start = prev_p.paragraph_document_byte_start;
                    prev.visual_lines
                        .iter()
                        .filter(|l| l.para_start == prev_para_byte_start)
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                };

                if !prev_lines.is_empty() {
                    if let Some(first_prev) = prev_lines.first() {
                        let y_offset = y - first_prev.y;
                        let byte_offset = paragraph_start as i64 - first_prev.para_start as i64;
                        let qchar_offset = paragraph_qchar_start as i64
                            - first_prev.qchar_start as i64
                            + first_prev.para_qchar_start as i64;
                        for mut vl in prev_lines {
                            vl.id = line_id;
                            vl.y += y_offset;
                            if byte_offset != 0 {
                                let new_para_start = (vl.para_start as i64 + byte_offset) as usize;
                                let new_byte_start = (vl.byte_start as i64 + byte_offset) as usize;
                                let new_byte_end = (vl.byte_end as i64 + byte_offset) as usize;
                                vl.para_start = new_para_start;
                                vl.byte_start = new_byte_start;
                                vl.byte_end = new_byte_end;
                            }
                            if qchar_offset != 0 {
                                vl.qchar_start = (vl.qchar_start as i64 + qchar_offset) as usize;
                                vl.qchar_end = (vl.qchar_end as i64 + qchar_offset) as usize;
                            }
                            visual_lines.push(vl);
                            line_id += 1;
                        }
                        if let Some(last_vl) = visual_lines.last() {
                            y = last_vl.y + last_vl.height;
                        }
                        reused_from_prev = true;
                    }
                }

                if let Some(pidx) = prev_para_idx {
                    let mut para = prev.paragraphs[pidx].clone();
                    let byte_offset =
                        paragraph_start as i64 - para.paragraph_document_byte_start as i64;
                    if byte_offset != 0 {
                        para.paragraph_document_byte_start = paragraph_start;
                        para.index_map =
                            crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                                &para.paragraph_text,
                                paragraph_start,
                            );
                        for line in &mut para.lines {
                            line.document_byte_start =
                                (line.document_byte_start as i64 + byte_offset) as usize;
                            line.document_byte_end =
                                (line.document_byte_end as i64 + byte_offset) as usize;
                            for cluster in &mut line.clusters {
                                cluster.document_byte_start =
                                    (cluster.document_byte_start as i64 + byte_offset) as usize;
                                cluster.document_byte_end =
                                    (cluster.document_byte_end as i64 + byte_offset) as usize;
                            }
                        }
                    }
                    paragraphs.push(para);
                } else {
                    paragraphs.push(CanonicalParagraphSnapshot {
                        paragraph_text: paragraph_text.to_string(),
                        paragraph_document_byte_start: paragraph_start,
                        lines: Vec::new(),
                        index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                            paragraph_text,
                            paragraph_start,
                        ),
                    });
                }
            }

            if !reused_from_prev {
                if paragraph_text.is_empty() {
                    // Issue #658: 空段落也占 null slot。
                    let _empty_canonical = prepare_paragraph_visual_snapshot(
                        paragraph_text,
                        paragraph_start,
                        font_size,
                        font_family,
                        available,
                        indent,
                        dpr,
                        Some(text_color),
                        current_para_idx as i32,
                        line_spacing,
                        generation,
                        generate_animation_visuals,
                    );
                    visual_lines.push(VisualLine {
                        id: line_id,
                        byte_start: paragraph_start,
                        byte_end: paragraph_start,
                        qchar_start: paragraph_qchar_start,
                        qchar_end: paragraph_qchar_start,
                        hard_break,
                        x: padding + indent,
                        y,
                        width: 0.0,
                        height: line_height,
                        para_text: String::new(),
                        para_start: paragraph_start,
                        qtextline_idx: 0,
                        para_qchar_start: 0,
                        para_qchar_end: 0,
                        line_wrap_width: available - indent,
                        line_indent_x: indent,
                        para_indent: indent,
                        x_end_trailing: 0.0,
                        qt_ascent: get_font_ascent(font_family, font_size as f32),
                        qt_descent: get_font_descent(font_family, font_size as f32),
                        cache_slot: current_para_idx as i32,
                    });
                    line_id += 1;
                    y += line_height;
                } else {
                    let canonical = prepare_paragraph_visual_snapshot(
                        paragraph_text,
                        paragraph_start,
                        font_size,
                        font_family,
                        available,
                        indent,
                        dpr,
                        Some(text_color),
                        current_para_idx as i32,
                        line_spacing,
                        generation,
                        generate_animation_visuals,
                    );
                    for (line_idx, canonical_line) in canonical.lines.iter().enumerate() {
                        let qt_metrics_h = canonical_line.ascent + canonical_line.descent;
                        let actual_line_h = if qt_metrics_h > 0.0 {
                            line_height.max(qt_metrics_h)
                        } else {
                            line_height
                        };
                        let is_first = line_idx == 0;
                        visual_lines.push(VisualLine {
                            id: line_id,
                            byte_start: canonical_line.document_byte_start,
                            byte_end: canonical_line.document_byte_end,
                            qchar_start: canonical_line.qchar_start + paragraph_qchar_start,
                            qchar_end: canonical_line.qchar_end + paragraph_qchar_start,
                            hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                            x: padding + canonical_line.x_pos,
                            y,
                            width: canonical_line.width,
                            height: actual_line_h,
                            para_text: paragraph_text.to_string(),
                            para_start: paragraph_start,
                            qtextline_idx: line_idx as i32,
                            para_qchar_start: canonical_line.qchar_start,
                            para_qchar_end: canonical_line.qchar_end,
                            line_wrap_width: if is_first {
                                available - indent
                            } else {
                                available
                            },
                            line_indent_x: if is_first { indent } else { 0.0 },
                            para_indent: indent,
                            x_end_trailing: canonical_line.x_end_trailing,
                            qt_ascent: canonical_line.ascent,
                            qt_descent: canonical_line.descent,
                            cache_slot: current_para_idx as i32,
                        });
                        line_id += 1;
                        y += actual_line_h;
                    }
                    paragraphs.push(canonical);
                }
            }

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            current_para_idx += 1;
            continue;
        }

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #658: 空段落也占 null slot。
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_start,
                font_size,
                font_family,
                available,
                indent,
                dpr,
                Some(text_color),
                current_para_idx as i32,
                line_spacing,
                generation,
                generate_animation_visuals,
            );
            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: paragraph_start,
                byte_end: paragraph_start,
                qchar_start: paragraph_qchar_start,
                qchar_end: paragraph_qchar_start,
                hard_break,
                x: padding + indent,
                y,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_start,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
                para_indent: indent,
                x_end_trailing: 0.0,
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
                cache_slot: current_para_idx as i32,
            });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    paragraph_start,
                ),
            });

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            current_para_idx += 1;
            continue;
        }

        let canonical = prepare_paragraph_visual_snapshot(
            paragraph_text,
            paragraph_start,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            Some(text_color),
            current_para_idx as i32,
            line_spacing,
            generation,
            generate_animation_visuals,
        );

        for (line_idx, canonical_line) in canonical.lines.iter().enumerate() {
            let qt_metrics_h = canonical_line.ascent + canonical_line.descent;
            let actual_line_h = if qt_metrics_h > 0.0 {
                line_height.max(qt_metrics_h)
            } else {
                line_height
            };

            let is_first = line_idx == 0;

            visual_lines.push(VisualLine {
                id: line_id,
                byte_start: canonical_line.document_byte_start,
                byte_end: canonical_line.document_byte_end,
                qchar_start: canonical_line.qchar_start + paragraph_qchar_start,
                qchar_end: canonical_line.qchar_end + paragraph_qchar_start,
                hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                x: padding + canonical_line.x_pos,
                y,
                width: canonical_line.width,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start: paragraph_start,
                qtextline_idx: line_idx as i32,
                para_qchar_start: canonical_line.qchar_start,
                para_qchar_end: canonical_line.qchar_end,
                line_wrap_width: if is_first {
                    available - indent
                } else {
                    available
                },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                cache_slot: current_para_idx as i32,
            });
            line_id += 1;
            y += actual_line_h;
        }

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        paragraphs.push(canonical);
        current_para_idx += 1;
    }

    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        // Issue #658: 尾部换行产生的空段也占 null slot。
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            text.len(),
            font_size,
            font_family,
            available,
            indent,
            dpr,
            Some(text_color),
            current_para_idx as i32,
            line_spacing,
            generation,
            generate_animation_visuals,
        );
        visual_lines.push(VisualLine {
            id: line_id,
            byte_start: text.len(),
            byte_end: text.len(),
            qchar_start: text_qchar_len,
            qchar_end: text_qchar_len,
            hard_break: false,
            x: padding + indent,
            y,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: text.len(),
            qtextline_idx: 0,
            para_qchar_start: 0,
            para_qchar_end: 0,
            line_wrap_width: available - indent,
            line_indent_x: indent,
            para_indent: indent,
            x_end_trailing: 0.0,
            qt_ascent: 0.0,
            qt_descent: 0.0,
            cache_slot: current_para_idx as i32,
        });
    }

    CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        paragraphs,
        visual_lines,
    }
}

/// Issue #658 评论 5624570557 问题 1: 把从已有 layout 提取的动画视觉注入到 doc snapshot。
///
/// `prepare_animation_visuals_from_layout` 从已有 QTextLine 提取的 `CanonicalLineSnapshot`
/// 包含 QImage/clusters，但 `old_doc_snapshot` 用 `generate_animation_visuals=false` 排版时
/// 其 `paragraphs[].lines[].image` 为 None。本函数按 `document_byte_start` 匹配，
/// 把提取的 QImage/clusters 注入到 doc snapshot 的对应行，使动画纹理可用。
pub fn inject_animation_visuals_into_snapshot(
    doc_snapshot: &mut CanonicalDocumentVisualSnapshot,
    animation_visuals: Vec<CanonicalLineSnapshot>,
) {
    for mut anim_line in animation_visuals {
        // 在 paragraphs 中找到包含该行的段落
        for para in &mut doc_snapshot.paragraphs {
            let para_start = para.paragraph_document_byte_start;
            let para_end = para_start + para.paragraph_text.len();
            // 检查 anim_line 是否属于该段落
            if anim_line.document_byte_start >= para_start
                && anim_line.document_byte_start < para_end
            {
                // 在该段落的 lines 中找到匹配的行（按 document_byte_start）
                for line in &mut para.lines {
                    if line.document_byte_start == anim_line.document_byte_start {
                        line.image = anim_line.image.take();
                        if !anim_line.clusters.is_empty() {
                            line.clusters = std::mem::take(&mut anim_line.clusters);
                        }
                        break;
                    }
                }
                break;
            }
        }
    }
}
