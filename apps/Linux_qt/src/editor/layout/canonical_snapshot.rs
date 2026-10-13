use cpp::cpp;

use std::borrow::Cow;
use std::sync::Arc;

// Issue #810 评论 5932233052 问题1: qchar_offset_to_byte_offset 和 QString 不再在此
// 模块使用（prepare_animation_visuals_from_layout 改为 raster-only，不再做 QChar→byte
// 转换，也不再从 C++ buffer 读取 cluster raw_font_fingerprint 返回 QString）。
use super::engine::{get_font_ascent, get_font_descent, prepare_paragraph_visual_snapshot};
use super::paragraph_height_index::{ParagraphHeightIndex, ParagraphOffsetIndex};
use super::types::{CaretAffinity, CaretRect, ParagraphId, VisualLine};

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
/// Issue #901 第十七轮问题3: `paragraph_slot` 是段落编号（cache_slot），
/// 用于 inject 时直接索引 `doc_snapshot.paragraphs[paragraph_slot]` 精确匹配目标行。
/// 不再用 `paragraph_document_byte_start`（现已是段落编号语义，但字段名易误用为全文 byte）。
/// `paragraph_slot` + `qtextline_idx` 是稳定行身份。
#[derive(Clone)]
pub struct AnimationRasterVisual {
    pub image: Option<qmetaobject::QImage>,
    pub paragraph_slot: usize,
    pub qtextline_idx: i32,
}

/// Issue #901 第十三轮问题1+2+6: 段落前缀偏移索引 — 维护段落文本长度（byte 和 qchar）
/// 的前缀和，作为段首绝对坐标的权威来源。
///
/// 增量编辑时只更新受影响段的长度，后续段的起始偏移由前缀和自动反映，
/// 不逐段写 `document_byte_start`，也不再用 SuffixTransform 记录平移量。
///
/// 消费方（hit_test、scene renderer 等）通过 `paragraph_offset_index` 获取段首
/// byte/qchar 坐标，通过 `ParagraphHeightIndex` 获取段首 y 坐标。

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
    ///
    /// Issue #901 第十三轮问题1+2+6: `visual_lines` 全部是已物化的绝对坐标。
    /// 移除了 SuffixTransform 机制——增量编辑时后缀段的绝对坐标就地更新，
    /// 不再依赖延迟物化的 delta 变换。这修复了连续编辑覆盖未物化 delta 的问题（问题2），
    /// 也消除了 find_vl_idx_with_suffix "先试新值失败试旧值"导致的段落误认（问题1）。
    pub visual_lines: Arc<Vec<VisualLine>>,
    /// Issue #901 第十三轮问题1+2+6: 段落前缀偏移索引 — 段首 byte/qchar 坐标的权威来源。
    ///
    /// 维护每段 byte/qchar 长度的前缀和。段首绝对坐标 = `paragraph_offset_index.paragraph_byte_start(idx)`。
    /// 增量编辑时只更新受影响段的长度（O(k log n)），后续段的起始偏移由前缀和自动反映。
    pub paragraph_offset_index: ParagraphOffsetIndex,
    /// Issue #901 第十四轮问题1: 段落高度索引 — 段首 y 坐标权威来源。
    /// 消费方通过 `padding + height_index.paragraph_top_y(cache_slot) + line.y`
    /// 计算文档级绝对 y 坐标。
    pub height_index: ParagraphHeightIndex,
    /// Issue #901 第十八轮问题4: ParagraphId → 当前段落 slot 映射。
    /// `materialize_line` 优先用此映射解析 slot，不依赖需要遍历后缀行更新的 `cache_slot`。
    /// 增量编辑拆段/合段后，后缀行的 `cache_slot` 可能过时，但 `paragraph_id` 不变，
    /// 通过此映射查到新 slot。`ParagraphId::INVALID` 不存入映射，
    /// `materialize_line` 遇到 INVALID 回退到 `cache_slot`。
    /// Issue #901 第十九轮问题5: Arc 共享，避免 to_layout_snapshot 中 clone O(段落数)。
    pub paragraph_id_to_slot: std::sync::Arc<std::collections::HashMap<ParagraphId, usize>>,
    /// Issue #901 第二十轮阻塞1: ParagraphId → 视觉行范围索引。
    /// `visual_lines` 中 ParagraphId 对应的行范围 `[start, end)`。
    /// 用于 `resolve_viewport_visual_lines` / `resolve_paragraph_visual_lines`
    /// 替换 raw `partition_point(cache_slot)` 二分查找，避免插段后复用后缀行
    /// cache_slot 过期导致二分找不到正确行。
    /// 按 visual_lines 中实际行顺序构建，同段落行连续。
    /// Issue #901 第二十轮阻塞1: Arc 共享，避免 to_layout_snapshot 中 clone O(段落数)。
    pub paragraph_id_to_line_range:
        std::sync::Arc<std::collections::HashMap<ParagraphId, (usize, usize)>>,
}

impl CanonicalDocumentVisualSnapshot {
    /// Issue #901 第十四轮问题1: 将段内相对坐标物化为文档级绝对坐标。
    /// Issue #901 第十五轮问题5a: `global_id` 参数保证行 ID 全局唯一。
    /// 物化时用 visual_lines 中的索引作为全局 id，不再依赖段内 ordinal，
    /// 避免受影响段新行与后缀未改行 id 冲突。
    fn materialize_line(&self, line: &VisualLine, global_id: usize) -> VisualLine {
        // Issue #901 第二十轮阻塞1+3: 用 logical_slot 查前缀索引得到段首绝对坐标。
        // logical_slot 是当前文档次序，与 paragraph_offset_index / height_index 的
        // 索引一致。cache_slot 保持原值（Qt 物理槽），传给 C++ qt_cache。
        // 优先用 paragraph_id_to_slot 映射解析 logical_slot（插段后后缀行 logical_slot
        // 可能过时，但 paragraph_id 不变，通过映射查到新 logical_slot）。
        let logical_slot = if line.paragraph_id != ParagraphId::INVALID {
            self.paragraph_id_to_slot
                .get(&line.paragraph_id)
                .copied()
                .unwrap_or(line.logical_slot as usize)
        } else {
            line.logical_slot as usize
        };
        let para_byte_start = self.paragraph_offset_index.paragraph_byte_start(logical_slot);
        let para_qchar_start = self.paragraph_offset_index.paragraph_qchar_start(logical_slot);
        let para_top_y = self.padding + self.height_index.paragraph_top_y(logical_slot);
        VisualLine {
            byte_start: line.byte_start + para_byte_start,
            byte_end: line.byte_end + para_byte_start,
            qchar_start: line.qchar_start + para_qchar_start,
            qchar_end: line.qchar_end + para_qchar_start,
            y: line.y + para_top_y,
            para_start: para_byte_start,
            id: global_id,
            hard_break: line.hard_break,
            x: line.x,
            width: line.width,
            height: line.height,
            para_text: line.para_text.clone(),
            qtextline_idx: line.qtextline_idx,
            para_qchar_start: line.para_qchar_start,
            para_qchar_end: line.para_qchar_end,
            line_wrap_width: line.line_wrap_width,
            line_indent_x: line.line_indent_x,
            para_indent: line.para_indent,
            x_end_trailing: line.x_end_trailing,
            qt_ascent: line.qt_ascent,
            qt_descent: line.qt_descent,
            // Issue #901 第二十轮阻塞3: cache_slot 保持原值（Qt 物理槽）。
            cache_slot: line.cache_slot,
            // Issue #901 第二十轮阻塞1+3: logical_slot 用解析后的当前文档次序。
            logical_slot: logical_slot as i32,
            // Issue #901 第十八轮问题4: 保留原行稳定段落身份。
            paragraph_id: line.paragraph_id,
        }
    }

    /// Issue #901 第十四轮问题1: 获取单行绝对坐标。
    ///
    /// `visual_lines` 存储段内相对坐标，本方法物化为文档级绝对坐标后返回。
    /// Issue #901 第十五轮问题5a: 行 id 用 visual_lines 中的索引（全局唯一）。
    pub fn resolve_visual_line(&self, idx: usize) -> VisualLine {
        match self.visual_lines.get(idx) {
            Some(vl) => self.materialize_line(vl, idx),
            None => VisualLine {
                id: 0, byte_start: 0, byte_end: 0, qchar_start: 0, qchar_end: 0,
                hard_break: true, x: 0.0, y: 0.0, width: 0.0,
                height: self.font_size * self.line_spacing,
                para_text: String::new(), para_start: 0, qtextline_idx: 0,
                para_qchar_start: 0, para_qchar_end: 0, line_wrap_width: 0.0,
                line_indent_x: 0.0, para_indent: 0.0, x_end_trailing: 0.0,
                qt_ascent: 0.0, qt_descent: 0.0, cache_slot: 0, logical_slot: 0,
                paragraph_id: super::types::ParagraphId::INVALID,
            },
        }
    }

    /// Issue #901 第十四轮问题1: 空操作 — 物化在 resolved_visual_lines 中完成。
    pub fn materialize_suffix_in_place(&mut self) {
        // 物化在 resolved_visual_lines / resolve_visual_line 中按需完成。
    }

    /// Issue #901 第十四轮问题1: 返回 visual_lines 的物化绝对坐标 Cow 视图。
    ///
    /// `visual_lines` 存储段内相对坐标，本方法通过 `paragraph_offset_index` + `height_index`
    /// 物化为文档级绝对坐标。消费方（cursor_rect 等）直接使用返回的绝对坐标。
    /// Issue #901 第十五轮问题5a: 行 id 用 visual_lines 中的索引（全局唯一）。
    pub fn resolved_visual_lines(&self) -> Cow<'_, [VisualLine]> {
        if self.visual_lines.is_empty() {
            return Cow::Borrowed(self.visual_lines.as_slice());
        }
        let materialized: Vec<VisualLine> = self
            .visual_lines
            .iter()
            .enumerate()
            .map(|(idx, line)| self.materialize_line(line, idx))
            .collect();
        Cow::Owned(materialized)
    }

    /// Issue #901 第十五轮问题5b: 物化 `[start, end)` 范围内的行为绝对坐标。
    ///
    /// 与 `resolved_visual_lines` 的区别：只物化指定范围的行，不物化整章，
    /// 避免每帧 O(整章) 物化。`start`/`end` 是 `visual_lines` 中的索引（半开区间）。
    /// 越界索引被 clamp 到 `visual_lines.len()`，`start >= end` 返回空 Vec。
    pub fn resolve_visual_line_range(&self, start: usize, end: usize) -> Vec<VisualLine> {
        if start >= end || start >= self.visual_lines.len() {
            return Vec::new();
        }
        let end = end.min(self.visual_lines.len());
        (start..end)
            .map(|idx| self.materialize_line(&self.visual_lines[idx], idx))
            .collect()
    }

    /// Issue #901 第十五轮问题5b: 物化视口内可见行的绝对坐标。
    ///
    /// 用 `ParagraphHeightIndex` 二分找到可见段范围 `[first_para, last_para_excl)`，
    /// 再用 `cache_slot` 在未物化的 `visual_lines` 上二分查找定位行索引范围，
    /// 只物化可见段内的行（加 `viewport_h.max(100.0)` 边距）。
    ///
    /// 与 `resolved_visual_lines` 的区别：只物化视口附近行，不物化整章。
    /// 返回的行可能包含少量视口外的边缘行（首尾段落的边缘行），
    /// 消费方需自行跳过视口外的行（与原 `resolved_visual_lines` 消费方一致）。
    ///
    /// 无 `height_index` 时退化为物化所有行（与 `resolved_visual_lines` 行为一致）。
    pub fn resolve_viewport_visual_lines(
        &self,
        scroll_y: f64,
        viewport_h: f64,
    ) -> Vec<VisualLine> {
        if self.visual_lines.is_empty() {
            return Vec::new();
        }
        let margin = viewport_h.max(100.0);
        let visible_top = scroll_y - margin;
        let visible_bottom = scroll_y + viewport_h + margin;

        if !self.height_index.is_empty() {
            let (first_para, last_para_excl) =
                self.height_index.paragraphs_in_y_range(visible_top, visible_bottom);
            if first_para < last_para_excl {
                // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 查行范围，
                // 替换 raw partition_point(cache_slot)。
                let (start_idx, end_idx) =
                    compute_line_range_from_logical_slots_canonical(self, first_para, last_para_excl);
                return self.resolve_visual_line_range(start_idx, end_idx);
            }
            return Vec::new();
        }

        // 无 height_index：退化为物化所有行。
        self.resolved_visual_lines().into_owned()
    }

    /// Issue #901 第十五轮问题5b: 物化指定段落的所有行为绝对坐标。
    ///
    /// 用 `paragraph_id_to_line_range` 查行范围，只物化该段落的行。
    /// `para_slot` 是段落在 `height_index` 中的索引（= `VisualLine.logical_slot`）。
    pub fn resolve_paragraph_visual_lines(&self, para_slot: usize) -> Vec<VisualLine> {
        if self.visual_lines.is_empty() {
            return Vec::new();
        }
        // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 查行范围，
        // 替换 raw partition_point(cache_slot)。
        let (start_idx, end_idx) =
            compute_line_range_from_logical_slots_canonical(self, para_slot, para_slot + 1);
        self.resolve_visual_line_range(start_idx, end_idx)
    }

    /// Issue #901 第二十轮阻塞1: 返回 `[first_logical_slot, last_logical_slot_excl)`
    /// 段落范围对应的 `visual_lines` 行索引范围 `[start_idx, end_idx)`。
    ///
    /// 优先用 `paragraph_id_to_line_range` 索引定位行范围，避免在插段/合段后
    /// 复用后缀行的 raw `logical_slot` 上做 `partition_point` 二分查找找错。
    /// 若索引为空（旧路径/fallback），退化为 `partition_point(logical_slot)`。
    ///
    /// 调用方拿到 `(start_idx, end_idx)` 后可用 `resolve_visual_line(idx)` 逐行物化。
    pub fn paragraph_visual_line_range(
        &self,
        first_logical_slot: usize,
        last_logical_slot_excl: usize,
    ) -> (usize, usize) {
        compute_line_range_from_logical_slots_canonical(
            self,
            first_logical_slot,
            last_logical_slot_excl,
        )
    }

    pub fn cursor_rect(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        // Issue #901 第十六轮问题2: 用 paragraph_offset_index O(log n) 定位段落，
        // 只物化该段和前一段的视觉行，不遍历全章 resolved_visual_lines。
        let line = self.find_caret_line(cursor_byte, affinity);

        let cursor_x = self.cursor_x_from_canonical(&line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) =
            super::engine::cursor_rect_for_line(&line, self.font_size, &self.font_family);
        let cursor_y = cursor_y_doc - scroll_y;
        let visible = cursor_y + cursor_h > 0.0 && cursor_y < viewport_h.max(1.0);

        // Issue #712: baseline_y 从 QTextLine 的真实 ascent/descent 计算。
        let baseline_y_doc =
            super::engine::text_baseline_y(&line, self.font_size, &self.font_family);
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

    /// Issue #901 第十六轮问题2: 在 CanonicalDocumentVisualSnapshot 中按需物化找到光标所在行。
    ///
    /// 用 `paragraph_offset_index.byte_to_paragraph` O(log n) 定位段落，
    /// 只物化该段和前一段的视觉行（处理段边界光标），不遍历全章。
    /// `paragraph_offset_index` 为空时退化为全章物化。
    fn find_caret_line(
        &self,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> VisualLine {
        let fallback = || VisualLine {
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
            logical_slot: 0,
            paragraph_id: super::types::ParagraphId::INVALID,
        };

        if self.visual_lines.is_empty() {
            return fallback();
        }

        if !self.paragraph_offset_index.is_empty() {
            let para_slot = self.paragraph_offset_index.byte_to_paragraph(cursor_byte);
            // 物化 para_slot-1 和 para_slot 两个段的行（处理段边界光标）
            let start_slot = para_slot.saturating_sub(1);
            let end_slot = para_slot + 1;
            // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 查行范围，
            // 替换 raw partition_point(cache_slot)。
            let (start_idx, end_idx) =
                compute_line_range_from_logical_slots_canonical(self, start_slot, end_slot);
            let lines = self.resolve_visual_line_range(start_idx, end_idx);

            for (local_idx, line) in lines.iter().enumerate() {
                if super::hit_test::line_contains_cursor_with_affinity(
                    &lines,
                    local_idx,
                    cursor_byte,
                    affinity,
                ) {
                    return line.clone();
                }
            }
            return lines.last().cloned().unwrap_or_else(fallback);
        }

        // paragraph_offset_index 为空，退化为全章物化
        let resolved = self.resolved_visual_lines();
        let lines_slice = resolved.as_ref();
        for (idx, line) in lines_slice.iter().enumerate() {
            if super::hit_test::line_contains_cursor_with_affinity(
                lines_slice,
                idx,
                cursor_byte,
                affinity,
            ) {
                return line.clone();
            }
        }
        lines_slice.last().cloned().unwrap_or_else(fallback)
    }

    /// Issue #722 评论 5748596920 问题1: canonical caret 文档坐标入口。
    ///
    /// 与 `cursor_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标（不减 scroll_y），
    /// `visible` 始终为 true。供正文事务 caret track 使用，使 caret track 与
    /// AnimatedSlice/StaticPatch 文档坐标系一致。
    pub fn cursor_rect_doc(&self, cursor_byte: usize, affinity: CaretAffinity) -> CaretRect {
        // Issue #901 第十六轮问题2: 用 paragraph_offset_index O(log n) 定位段落，
        // 只物化该段和前一段的视觉行，不遍历全章 resolved_visual_lines。
        let line = self.find_caret_line(cursor_byte, affinity);

        let cursor_x = self.cursor_x_from_canonical(&line, cursor_byte, affinity);
        let (cursor_y_doc, cursor_h) =
            super::engine::cursor_rect_for_line(&line, self.font_size, &self.font_family);
        let baseline_y_doc =
            super::engine::text_baseline_y(&line, self.font_size, &self.font_family);

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

        // Issue #901 第二十轮阻塞3: 用 line.logical_slot 直接索引 paragraphs，
        // 不再用 line.cache_slot（Qt 物理槽，与 paragraphs 数组索引不对应）。
        // paragraph_document_byte_start 现已是段落编号（slot），不是全文 byte offset，
        // 旧的 `p.paragraph_document_byte_start <= cursor_byte` 比较会因单位不匹配
        // 导致第二段以后找不到段落，函数直接 return line.x，造成光标定位行首。
        let slot = line.logical_slot as usize;
        let para = match self.paragraphs.get(slot) {
            Some(p) => p,
            None => return line.x,
        };

        // 用 paragraph_offset_index 计算段首 byte offset（全文坐标）。
        // line 是物化后的视觉行，line.para_start 也是段首全文 byte offset，
        // 但优先用 paragraph_offset_index 作为权威来源；index 为空时回退到 line.para_start。
        let para_byte_start = if !self.paragraph_offset_index.is_empty() {
            self.paragraph_offset_index.paragraph_byte_start(slot)
        } else {
            line.para_start
        };
        let cursor_in_para = cursor_byte.saturating_sub(para_byte_start);

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
            // Issue #901 第十四轮问题1: 段落偏移和高度索引。
            paragraph_offset_index: super::paragraph_height_index::ParagraphOffsetIndex::new(),
            height_index: super::paragraph_height_index::ParagraphHeightIndex::new(),
            // Issue #658 评论 5620035970 问题 2: 动画路径不直接渲染此 snapshot，
            // generation 填 0 表示不用于 rebuild_text_node_from_paragraphs。
            layout_generation: 0,
            // Issue #901 第十九轮问题1: 从 canonical snapshot 继承 ParagraphId → slot 映射。
            // Issue #901 第十九轮问题5: Arc 共享，O(1) clone 而非 O(段落数)。
            paragraph_id_to_slot: std::sync::Arc::clone(&self.paragraph_id_to_slot),
            // Issue #901 第二十轮阻塞1: 从 canonical snapshot 继承 ParagraphId → 行范围映射。
            paragraph_id_to_line_range: std::sync::Arc::clone(&self.paragraph_id_to_line_range),
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
    inject_animation_visuals_into_snapshot,
    prepare_affected_paragraphs_visual_snapshot, prepare_animation_visuals_from_layout,
    prepare_document_visual_snapshot, prepare_document_visual_snapshot_scoped,
    prepare_incremental_visual_snapshot,
};
// Issue #810 评论 5932233052 问题1: AnimationRasterVisual 定义在本模块（pub struct），
// 外部通过 canonical_snapshot::AnimationRasterVisual 访问，无需额外 re-export。

/// Issue #901 第二十轮阻塞1: 通过 logical_slot 范围计算 visual_lines 行范围。
///
/// `first_logical_slot..last_logical_slot_excl` 是文档次序段落范围。
/// 对每个 logical_slot，通过 `paragraph_id_to_slot` 反查 ParagraphId，
/// 再从 `paragraph_id_to_line_range` 查行范围 `[start, end)`。
///
/// 由于 `paragraph_id_to_slot` 是 id → slot 映射，需要反向查找。
/// 段落数远小于行数，O(段落数) 反向查找可接受。
/// 若 `paragraph_id_to_line_range` 为空（旧路径/fallback），退化为
/// `partition_point(logical_slot)` 二分查找。
fn compute_line_range_from_logical_slots_canonical(
    snapshot: &CanonicalDocumentVisualSnapshot,
    first_logical_slot: usize,
    last_logical_slot_excl: usize,
) -> (usize, usize) {
    // Issue #901 第二十轮阻塞1: 优先用 paragraph_id_to_line_range 索引。
    if !snapshot.paragraph_id_to_line_range.is_empty() {
        let id_to_slot = &snapshot.paragraph_id_to_slot;
        let id_to_range = &snapshot.paragraph_id_to_line_range;

        let mut start_idx = snapshot.visual_lines.len();
        let mut end_idx = 0usize;
        for (id, &slot) in id_to_slot.iter() {
            if slot >= first_logical_slot && slot < last_logical_slot_excl {
                if let Some(&(s, e)) = id_to_range.get(id) {
                    start_idx = start_idx.min(s);
                    end_idx = end_idx.max(e);
                }
            }
        }
        if start_idx <= end_idx && end_idx > 0 {
            return (start_idx, end_idx);
        }
    }

    // Fallback: 用 logical_slot 二分查找。
    let start_idx = snapshot
        .visual_lines
        .partition_point(|l| l.logical_slot < first_logical_slot as i32);
    let end_idx = snapshot
        .visual_lines
        .partition_point(|l| l.logical_slot < last_logical_slot_excl as i32);
    (start_idx, end_idx)
}
