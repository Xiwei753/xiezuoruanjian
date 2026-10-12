use cpp::cpp;

use std::borrow::Cow;
use std::sync::Arc;

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

/// Issue #901 第十轮问题5: 后缀段平移变换 — 记录未改后缀段的坐标平移量，
/// 避免每笔编辑逐行写绝对位置。
///
/// 增量编辑时，受影响段 splice 后，后续未改段的 y / byte / qchar / line_id
/// 需要平移一个常量 delta。此结构记录该平移量，消费方通过
/// [`CanonicalDocumentVisualSnapshot::resolve_visual_line`] 按需应用，
/// 不在编辑路径逐行物化。
///
/// # 不变量
///
/// - `start_line_idx` 是 `visual_lines` 中第一个受平移影响的行索引。
/// - `end_line_idx` 是 `visual_lines` 中最后一个受平移影响的行索引（ exclusive）。
/// - `visual_lines[0..start_line_idx]` 是已物化的绝对坐标（前缀段 + 受影响段）。
/// - `visual_lines[start_line_idx..end_line_idx]` 是**旧绝对坐标**（基础值），消费时加 delta。
/// - `visual_lines[end_line_idx..]` 是已物化的绝对坐标（空文本/尾部换行追加的新行）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SuffixTransform {
    /// 后缀段起始行索引（`visual_lines[start_line_idx..end_line_idx]` 受平移影响）。
    pub start_line_idx: usize,
    /// 后缀段结束行索引（exclusive）。
    pub end_line_idx: usize,
    /// y 坐标平移量（段落高度变化导致后续段 y 漂移）。
    pub y_delta: f64,
    /// byte 坐标平移量（段前插入/删除导致后续段 byte 起点漂移）。
    pub byte_delta: i64,
    /// qchar 坐标平移量（段前插入/删除导致后续段 qchar 起点漂移）。
    pub qchar_delta: i64,
    /// line_id 平移量（受影响行数变化导致后续行 id 漂移）。
    pub line_id_delta: i64,
}

impl SuffixTransform {
    /// 是否为恒等变换（无平移）。
    pub fn is_identity(&self) -> bool {
        self.y_delta.abs() < 1e-9
            && self.byte_delta == 0
            && self.qchar_delta == 0
            && self.line_id_delta == 0
    }
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
    /// Issue #901 第九轮阻塞5: 用 Arc 共享视觉行数据，避免每键 clone Vec<VisualLine>。
    /// `LayoutSnapshot.lines` 和本字段通过 `Arc::clone` 共享同一份数据。
    pub visual_lines: Arc<Vec<VisualLine>>,
    /// Issue #901 第十轮问题5: 后缀段平移变换。
    ///
    /// `None` 表示 `visual_lines` 全部是已物化的绝对坐标。
    /// `Some(t)` 表示 `visual_lines[t.start_line_idx..]` 是旧绝对坐标（基础值），
    /// 消费方通过 [`resolve_visual_line`] 按需加 delta 得到正确绝对坐标。
    /// 增量编辑路径不逐行写后缀段绝对位置，只记录此变换。
    pub suffix_transform: Option<SuffixTransform>,
}

impl CanonicalDocumentVisualSnapshot {
    /// Issue #901 第十轮问题5: 按需解析单行绝对坐标。
    ///
    /// 如果 `suffix_transform` 为 None 或 `idx < start_line_idx`，返回基础值 clone。
    /// 否则返回基础值 + delta（y/byte/qchar/line_id 平移）。
    ///
    /// 消费方用此方法代替直接访问 `self.visual_lines[idx]`，
    /// 以正确处理增量编辑后未物化的后缀段。
    pub fn resolve_visual_line(&self, idx: usize) -> VisualLine {
        let base = match self.visual_lines.get(idx) {
            Some(vl) => vl.clone(),
            None => return VisualLine {
                id: 0, byte_start: 0, byte_end: 0, qchar_start: 0, qchar_end: 0,
                hard_break: true, x: 0.0, y: 0.0, width: 0.0,
                height: self.font_size * self.line_spacing,
                para_text: String::new(), para_start: 0, qtextline_idx: 0,
                para_qchar_start: 0, para_qchar_end: 0, line_wrap_width: 0.0,
                line_indent_x: 0.0, para_indent: 0.0, x_end_trailing: 0.0,
                qt_ascent: 0.0, qt_descent: 0.0, cache_slot: 0,
            },
        };
        match self.suffix_transform {
            Some(t) if idx >= t.start_line_idx && idx < t.end_line_idx => {
                apply_suffix_transform(base, t)
            }
            _ => base,
        }
    }

    /// Issue #901 第十一轮问题2: 就地物化后缀段平移变换到 visual_lines，
    /// 然后清除 suffix_transform。
    ///
    /// 消费方在需要直接访问 visual_lines 基础数组的原始字段
    ///（如增量排版下一笔编辑用 `find_line_range_by_para_start` 定位段落）
    /// 前调用此方法，确保绝对坐标已物化，不会读到未应用 delta 的旧值。
    ///
    /// 仅对 `suffix_transform` 受影响范围 `[start_line_idx, end_line_idx)` 的行
    /// 就地更新 y/byte/qchar/line_id，其余行已是正确绝对坐标不动。
    /// 物化后 `suffix_transform = None`，后续 `resolved_visual_lines` 走零分配借用路径。
    pub fn materialize_suffix_in_place(&mut self) {
        if let Some(t) = self.suffix_transform {
            if !t.is_identity() {
                let vls = Arc::make_mut(&mut self.visual_lines);
                let apply_start = t.start_line_idx.min(vls.len());
                let apply_end = t.end_line_idx.min(vls.len());
                for i in apply_start..apply_end {
                    let vl = &mut vls[i];
                    vl.y += t.y_delta;
                    if t.byte_delta != 0 {
                        vl.byte_start = (vl.byte_start as i64 + t.byte_delta) as usize;
                        vl.byte_end = (vl.byte_end as i64 + t.byte_delta) as usize;
                        vl.para_start = (vl.para_start as i64 + t.byte_delta) as usize;
                    }
                    if t.qchar_delta != 0 {
                        vl.qchar_start = (vl.qchar_start as i64 + t.qchar_delta) as usize;
                        vl.qchar_end = (vl.qchar_end as i64 + t.qchar_delta) as usize;
                    }
                    if t.line_id_delta != 0 {
                        vl.id = (vl.id as i64 + t.line_id_delta) as usize;
                    }
                }
            }
            self.suffix_transform = None;
        }
    }

    /// Issue #901 第十轮问题5: 物化全部视觉行为绝对坐标的 Cow 视图。
    ///
    /// - `suffix_transform` 为 None → `Cow::Borrowed(&visual_lines)`，零分配。
    /// - `suffix_transform` 为 Some → `Cow::Owned(物化后的 Vec)`，O(n) 物化。
    ///
    /// 消费方用此方法代替直接访问 `&self.visual_lines`，
    /// 以正确处理增量编辑后未物化的后缀段。
    /// 物化只在 `suffix_transform` 非空时发生（段前插入/删除），
    /// 纯字符替换（等长同高）不物化。
    ///
    /// Issue #901 第十一轮问题2: 优化物化路径——只对受影响范围
    /// `[start_line_idx, end_line_idx)` 的行应用平移变换，
    /// 其余行直接 clone 基础值（已是正确绝对坐标），
    /// 避免逐行调用 `resolve_visual_line` 的 match 分支开销。
    /// 消费方若持有 `&mut self`，建议先调用 `materialize_suffix_in_place`
    /// 就地物化并清除 transform，后续 `resolved_visual_lines` 即走零分配借用路径。
    pub fn resolved_visual_lines(&self) -> Cow<'_, [VisualLine]> {
        match self.suffix_transform {
            Some(t) if !t.is_identity() => {
                let total = self.visual_lines.len();
                let mut resolved: Vec<VisualLine> = self.visual_lines.as_slice().to_vec();
                let apply_start = t.start_line_idx.min(total);
                let apply_end = t.end_line_idx.min(total);
                for i in apply_start..apply_end {
                    let vl = &mut resolved[i];
                    vl.y += t.y_delta;
                    if t.byte_delta != 0 {
                        vl.byte_start = (vl.byte_start as i64 + t.byte_delta) as usize;
                        vl.byte_end = (vl.byte_end as i64 + t.byte_delta) as usize;
                        vl.para_start = (vl.para_start as i64 + t.byte_delta) as usize;
                    }
                    if t.qchar_delta != 0 {
                        vl.qchar_start = (vl.qchar_start as i64 + t.qchar_delta) as usize;
                        vl.qchar_end = (vl.qchar_end as i64 + t.qchar_delta) as usize;
                    }
                    if t.line_id_delta != 0 {
                        vl.id = (vl.id as i64 + t.line_id_delta) as usize;
                    }
                }
                Cow::Owned(resolved)
            }
            _ => Cow::Borrowed(self.visual_lines.as_slice()),
        }
    }

    pub fn cursor_rect(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        // Issue #901 第十轮问题5: 用 resolved_visual_lines 按需物化后缀段绝对坐标。
        let resolved = self.resolved_visual_lines();
        let lines_slice = resolved.as_ref();
        let line = lines_slice
            .iter()
            .enumerate()
            .find(|(idx, _)| {
                super::hit_test::line_contains_cursor_with_affinity(
                    lines_slice,
                    *idx,
                    cursor_byte,
                    affinity,
                )
            })
            .map(|(_, line)| line)
            .or_else(|| lines_slice.last());

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
        // Issue #901 第十轮问题5: 用 resolved_visual_lines 按需物化后缀段绝对坐标。
        let resolved = self.resolved_visual_lines();
        let lines_slice = resolved.as_ref();
        let line = lines_slice
            .iter()
            .enumerate()
            .find(|(idx, _)| {
                super::hit_test::line_contains_cursor_with_affinity(
                    lines_slice,
                    *idx,
                    cursor_byte,
                    affinity,
                )
            })
            .map(|(_, line)| line)
            .or_else(|| lines_slice.last());

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
            // Issue #901 第九轮阻塞5: Arc::clone 共享视觉行数据，不 clone Vec。
            lines: Arc::clone(&self.visual_lines),
            // Issue #658 评论 5620035970 问题 2: 动画路径不直接渲染此 snapshot，
            // generation 填 0 表示不用于 rebuild_text_node_from_paragraphs。
            layout_generation: 0,
            // Issue #901 第十轮问题5: 传递后缀段平移变换，消费方按需解析。
            suffix_transform: self.suffix_transform,
        }
    }
}

/// Issue #901 第十轮问题5: 对单行应用后缀段平移变换，返回绝对坐标。
///
/// `base` 是旧绝对坐标（基础值），`t` 是平移变换。
/// 返回 `base` 的 y/byte/qchar/line_id 加上对应 delta。
fn apply_suffix_transform(mut vl: VisualLine, t: SuffixTransform) -> VisualLine {
    vl.y += t.y_delta;
    if t.byte_delta != 0 {
        vl.byte_start = (vl.byte_start as i64 + t.byte_delta) as usize;
        vl.byte_end = (vl.byte_end as i64 + t.byte_delta) as usize;
        vl.para_start = (vl.para_start as i64 + t.byte_delta) as usize;
    }
    if t.qchar_delta != 0 {
        vl.qchar_start = (vl.qchar_start as i64 + t.qchar_delta) as usize;
        vl.qchar_end = (vl.qchar_end as i64 + t.qchar_delta) as usize;
    }
    if t.line_id_delta != 0 {
        vl.id = (vl.id as i64 + t.line_id_delta) as usize;
    }
    vl
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
    prepare_incremental_visual_snapshot,
};
// Issue #810 评论 5932233052 问题1: AnimationRasterVisual 定义在本模块（pub struct），
// 外部通过 canonical_snapshot::AnimationRasterVisual 访问，无需额外 re-export。
