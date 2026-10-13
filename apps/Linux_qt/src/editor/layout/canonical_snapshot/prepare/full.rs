//! 排版结果到 snapshot 的实际组装。
//!
//! 从 `prepare.rs` 拆出：`prepare_document_visual_snapshot_impl` 加上「从已有
//! VisualLine 组装」「只组装受影响段落」两个入口共 837 行，是整条搬运逻辑；
//! `prepare.rs` 只保留对外入口与动画视觉资源提取。

use super::*;
// Issue #901 第五轮阻塞2: 简化 paragraph_layout_store 全路径引用。
use super::super::super::paragraph_layout_store::{ParagraphLayoutStore, ParagraphSpliceRanges};
// Issue #901 第十三轮问题1+2+6: 引入 ParagraphOffsetIndex 作为段首坐标权威来源。
use super::super::super::paragraph_height_index::ParagraphOffsetIndex;
use super::super::super::paragraph_height_index::ParagraphHeightIndex;
use super::super::super::paragraph_height_index::ParagraphLineCountIndex;
// Issue #901 第十八轮问题4: ParagraphId 现定义在 types.rs，用于 VisualLine.paragraph_id。
use super::super::super::types::ParagraphId;
// Issue #901 第九轮阻塞5: Arc 共享视觉行数据。
use std::sync::Arc;

/// Issue #901 第十三轮问题1+2+6: 从全文文本构建段落前缀偏移索引。
///
/// 每段 byte/qchar 长度含尾部 `'\n'`。段首绝对坐标由前缀和按需计算，
/// 不逐段存储 `document_byte_start`，也不再用 SuffixTransform 记录平移量。
fn build_offset_index_from_text(text: &str) -> ParagraphOffsetIndex {
    // Issue #901 第二十四轮问题1: 用统一切分器生成段落序列，
    // 为尾空段也生成长度 0 的 offset entry，与 canonical/store/line-count/height 段数一致。
    let paras = super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text);
    let mut byte_lengths = Vec::with_capacity(paras.len());
    let mut qchar_lengths = Vec::with_capacity(paras.len());
    for (content, hard_break) in &paras {
        byte_lengths.push(content.len() + if *hard_break { 1 } else { 0 });
        qchar_lengths.push(
            content.chars().map(|c| c.len_utf16()).sum::<usize>() + if *hard_break { 1 } else { 0 },
        );
    }
    let mut idx = ParagraphOffsetIndex::new();
    idx.rebuild(byte_lengths, qchar_lengths);
    idx
}

/// Issue #901 第十四轮问题1: 从视觉行数组构建段落高度索引。
///
/// 按 `para_start`（段落索引）分组累加行高，返回 `ParagraphHeightIndex`。
fn build_height_index_from_lines(lines: &[VisualLine]) -> ParagraphHeightIndex {
    if lines.is_empty() {
        return ParagraphHeightIndex::new();
    }
    use std::collections::HashMap;
    let mut para_start_to_idx: HashMap<usize, usize> = HashMap::new();
    let mut para_starts: Vec<usize> = Vec::new();
    for vl in lines {
        if !para_start_to_idx.contains_key(&vl.para_start) {
            para_start_to_idx.insert(vl.para_start, para_starts.len());
            para_starts.push(vl.para_start);
        }
    }
    let mut heights = vec![0.0f64; para_starts.len()];
    for vl in lines {
        if let Some(&idx) = para_start_to_idx.get(&vl.para_start) {
            heights[idx] += vl.height;
        }
    }
    let mut idx = ParagraphHeightIndex::new();
    idx.rebuild(heights);
    idx
}

/// Issue #901 第十八轮问题4: 从视觉行数组构建 ParagraphId → slot 映射。
///
/// 遍历 `visual_lines`，对每个 `paragraph_id != INVALID` 的行，
/// 用 `logical_slot` 作为 slot 建立 `paragraph_id → logical_slot` 映射。
/// 同一段落的多行会有相同 `paragraph_id`，用 `or_insert` 保留第一个（都相同）。
fn build_paragraph_id_to_slot_from_lines(
    visual_lines: &[VisualLine],
) -> std::collections::HashMap<ParagraphId, usize> {
    let mut map = std::collections::HashMap::new();
    for vl in visual_lines {
        if vl.paragraph_id != ParagraphId::INVALID {
            map.entry(vl.paragraph_id)
                .or_insert(vl.logical_slot as usize);
        }
    }
    map
}

/// Issue #901 第二十一轮问题2: 从视觉行数组构建段落行数前缀索引。
///
/// 按 `logical_slot`（段落文档次序）分组统计每段视觉行数，
/// 返回 `ParagraphLineCountIndex`（Fenwick 树）。
/// 替代旧的 `build_paragraph_id_to_line_range_from_lines` ——
/// 旧函数构建 `HashMap<ParagraphId, (usize, usize)>` 以全局下标为绝对值，
/// 只对改动段更新时后缀段范围会指向旧下标。Fenwick 树前缀和自动平移后缀段范围。
fn build_line_count_index_from_lines(visual_lines: &[VisualLine]) -> ParagraphLineCountIndex {
    if visual_lines.is_empty() {
        return ParagraphLineCountIndex::new();
    }
    // 按 logical_slot 分组统计行数。logical_slot 非降序，同段连续。
    let mut counts: Vec<usize> = Vec::new();
    let mut current_slot: i32 = visual_lines[0].logical_slot;
    let mut count = 0usize;
    for vl in visual_lines {
        if vl.logical_slot != current_slot {
            counts.push(count);
            current_slot = vl.logical_slot;
            count = 0;
        }
        count += 1;
    }
    counts.push(count);
    ParagraphLineCountIndex::from_counts(counts)
}

/// Issue #901 第十八轮问题4: 从 paragraph_store 构建 ParagraphId → slot 映射。
///
/// 遍历 `paragraphs.len()` 个段落，对每个 `paragraph_store.paragraph_id_at(slot)`
/// 建立 `ParagraphId → slot` 映射。用于增量排版路径，`paragraph_store` 持有
/// 拆段/合段后的最新 ParagraphId → slot 关系。
fn build_paragraph_id_to_slot_from_store(
    paragraph_store: &ParagraphLayoutStore,
    paragraph_count: usize,
) -> std::collections::HashMap<ParagraphId, usize> {
    let mut map = std::collections::HashMap::new();
    for slot in 0..paragraph_count {
        if let Some(id) = paragraph_store.paragraph_id_at(slot) {
            map.insert(id, slot);
        }
    }
    map
}

pub(super) fn prepare_document_visual_snapshot_impl(
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
    // Issue #901 第二十轮阻塞2: 由 ParagraphLayoutStore 统一分配的 ParagraphId 序列。
    // paragraph_ids[i] 是第 i 个段落的稳定身份。移除 slot+1 伪造 Id 的做法。
    paragraph_ids: &[ParagraphId],
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
    // Issue #901 第十四轮问题1: 段内相对存储 — 追踪段顶 y 用于计算相对 y。
    let mut paragraph_top_y: f64 = padding;

    for paragraph in text.split_inclusive('\n') {
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        // Issue #901 第十四轮问题1: 每段开始时记录段顶 y。
        paragraph_top_y = y;

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #658: 空段落也调用 prepare_paragraph_visual_snapshot 占 null slot，
            // 保持 cache_slot 与文档段落一一对应。
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_idx as usize,
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
                byte_start: 0,
                byte_end: 0,
                qchar_start: 0,
                qchar_end: 0,
                hard_break,
                x: padding + indent,
                y: 0.0,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_idx as usize,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
                para_indent: indent,
                x_end_trailing: 0.0,
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
                // Issue #901 第二十轮阻塞3: cache_slot 是 Qt 物理槽号，
                // 与 paragraph_idx 一致（全篇排版时物理槽 = 文档次序）。
                cache_slot: paragraph_idx,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx,
                // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId，
                // 不再用 slot+1 伪造 Id。fallback 到 INVALID。
                paragraph_id: paragraph_ids
                    .get(paragraph_idx as usize)
                    .copied()
                    .unwrap_or(ParagraphId::INVALID),
            });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_idx as usize,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    0,
                ),
            });

            paragraph_start += paragraph.len();
            paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();
            paragraph_idx += 1;
            continue;
        }

        let canonical = prepare_paragraph_visual_snapshot(
            paragraph_text,
            paragraph_idx as usize,
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
                qchar_start: canonical_line.qchar_start,
                qchar_end: canonical_line.qchar_end,
                hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                x: padding + canonical_line.x_pos,
                y: y - paragraph_top_y,
                width: canonical_line.width,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start: (paragraph_idx - 1) as usize,
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
                // Issue #901 第二十轮阻塞3: cache_slot 是 Qt 物理槽号。
                cache_slot: paragraph_idx - 1,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx - 1,
                // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId，
                // 不再用 slot+1 伪造 Id。paragraph_idx 已递增，当前段索引为 paragraph_idx - 1。
                paragraph_id: paragraph_ids
                    .get((paragraph_idx - 1) as usize)
                    .copied()
                    .unwrap_or(ParagraphId::INVALID),
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
            y: 0.0,
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
            logical_slot: 0,
            // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId。
            paragraph_id: paragraph_ids
                .get(0)
                .copied()
                .unwrap_or(ParagraphId::INVALID),
        });
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: String::new(),
            paragraph_document_byte_start: 0,
            lines: Vec::new(),
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", 0),
        });
    }

    if text.ends_with('\n') {
        // Issue #901 第十四轮问题1: 段内相对存储 — 尾部换行空段的段顶 y。
        paragraph_top_y = y;
        // Issue #658: 尾部换行产生的空段也占 null slot。
        let _empty_canonical = prepare_paragraph_visual_snapshot(
            "",
            paragraph_idx as usize,
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
            byte_start: 0,
            byte_end: 0,
            qchar_start: 0,
            qchar_end: 0,
            hard_break: false,
            x: padding + indent,
            y: 0.0,
            width: 0.0,
            height: line_height,
            para_text: String::new(),
            para_start: paragraph_idx as usize,
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
            logical_slot: paragraph_idx,
            // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId。
            paragraph_id: paragraph_ids
                .get(paragraph_idx as usize)
                .copied()
                .unwrap_or(ParagraphId::INVALID),
        });
        // Issue #901 第二十三轮问题1: 为尾空段创建对应的 CanonicalParagraphSnapshot，
        // 保证 store entries、canonical paragraphs、VisualLine、ParagraphId 逐段一一对应。
        // 尾空段是结构事实，不是游离视觉行。
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: String::new(),
            paragraph_document_byte_start: paragraph_idx as usize,
            lines: Vec::new(),
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", 0),
        });
        // paragraph_idx 递增省略：尾部换行分支后不再使用。
    }

    let height_index = build_height_index_from_lines(&visual_lines);
    // Issue #901 第十八轮问题4: 构建 paragraph_id → slot 映射。
    // 全篇排版路径无 paragraph_store，从 visual_lines 的 paragraph_id 构建。
    // Issue #901 第十九轮问题5: Arc 共享，避免 to_layout_snapshot 中 clone O(段落数)。
    let paragraph_id_to_slot =
        std::sync::Arc::new(build_paragraph_id_to_slot_from_lines(&visual_lines));
    // Issue #901 第二十二轮问题4: Arc 包装行数前缀索引，O(1) 共享。
    let paragraph_line_count_index = std::sync::Arc::new(build_line_count_index_from_lines(&visual_lines));
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
        visual_lines: Arc::new(visual_lines),
        paragraph_offset_index: Arc::new(build_offset_index_from_text(text)),
        height_index,
        paragraph_id_to_slot,
        paragraph_line_count_index,
    }
}

// ── Issue #901: 增量排版实现 ──

/// 从 `CanonicalDocumentVisualSnapshot` 的 `visual_lines` 提取每段高度。
///
/// 按 `cache_slot` 分组累加行高，返回按段落顺序排列的高度数组。
///
/// Issue #901: 用 HashMap 索引代替 Vec `contains/position` 线性查找，
/// 避免 O(行数 × 段数) 最坏复杂度。
fn extract_paragraph_heights(visual_lines: &[VisualLine]) -> Vec<f64> {
    use std::collections::HashMap;

    // 用 HashMap 按 para_start 去重并记录段落顺序索引
    let mut para_start_to_idx: HashMap<usize, usize> = HashMap::new();
    let mut para_starts: Vec<usize> = Vec::new();
    for vl in visual_lines {
        if !para_start_to_idx.contains_key(&vl.para_start) {
            para_start_to_idx.insert(vl.para_start, para_starts.len());
            para_starts.push(vl.para_start);
        }
    }

    let mut heights = vec![0.0f64; para_starts.len()];
    // 用 HashMap 直接索引，O(1) 查找
    for vl in visual_lines {
        if let Some(&idx) = para_start_to_idx.get(&vl.para_start) {
            heights[idx] += vl.height;
        }
    }
    heights
}

/// Issue #901: 从本轮新排版的视觉行按段索引聚合受影响段落的高度更新列表。
///
/// 只对 `affected_indices` 中列出的段落提取 `(段落索引, 新高度)`，
/// 用于 `ParagraphHeightIndex::update_heights_incremental`。
///
/// Issue #901 第二轮复核问题 3: 一次构建 para_start → 段落顺序索引 和
/// para_start → 累计高度 的 HashMap，不在循环内每次重建全部 para_starts + Vec::contains。
///
/// Issue #901 第十六轮问题3: 不再遍历全章 `visual_lines`，改为只遍历
/// `new_lines`（本轮新排版的受影响段视觉行）。`new_lines` 只包含受影响段
/// 的新行，按 `cache_slot` 聚合高度是 O(k)（k=受影响段行数），
/// 不随全章行数线性增长。调用方传入 splice 范围内的新行切片。
fn extract_affected_height_updates(
    new_lines: &[VisualLine],
    affected_indices: &[usize],
) -> Vec<(usize, f64)> {
    use std::collections::HashMap;

    if affected_indices.is_empty() || new_lines.is_empty() {
        return Vec::new();
    }

    // Issue #901 第十六轮问题3: 只遍历 new_lines，按 logical_slot 聚合高度，O(k)。
    // new_lines 中的行 logical_slot 是新段索引（文档次序），
    // 同一段落的所有行 logical_slot 相同。
    // Issue #901 第二十轮阻塞1+3: 改用 logical_slot（文档次序）而非 cache_slot（Qt 物理槽）。
    let mut slot_to_height: HashMap<i32, f64> = HashMap::new();
    for vl in new_lines {
        *slot_to_height.entry(vl.logical_slot).or_insert(0.0) += vl.height;
    }

    // 对每个受影响段落，O(1) 查表获取高度
    let mut updates: Vec<(usize, f64)> = Vec::new();
    for &idx in affected_indices {
        if let Some(&height) = slot_to_height.get(&(idx as i32)) {
            updates.push((idx, height));
        }
    }
    updates
}

/// Issue #901 第三轮复核问题 4 + 第二十三轮问题2: 检查 previous_snapshot 的段落结构
/// 是否与 new text 完全一致（含 hard_break）。
///
/// 用 `split_paragraphs_with_hard_break(text)` 切分新文本，与 prev 的段落结构
///（`paragraphs[i].paragraph_text` + 从 `visual_lines` 获取的 `hard_break`）逐段比较。
/// 段落数、每段 content 和 hard_break 都相同才返回 true。
/// O(n) 但只在 `no_affected_range` 边界情况调用，不影响常规编辑性能。
fn prev_snapshot_text_matches(prev: &CanonicalDocumentVisualSnapshot, text: &str) -> bool {
    let new_paras = super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text);
    let prev_para_count = prev.paragraphs.len();
    if new_paras.len() != prev_para_count {
        return false;
    }
    for i in 0..prev_para_count {
        let prev_content = &prev.paragraphs[i].paragraph_text;
        let (new_content, new_hard_break) = new_paras[i];
        if prev_content.as_str() != new_content {
            return false;
        }
        // 从 visual_lines 获取 prev 段 i 最后一行的 hard_break。
        // paragraph_line_count_index 的行范围基于段内行数前缀和，不依赖 logical_slot。
        let (line_start, line_end) = prev
            .paragraph_line_count_index
            .paragraph_line_range(i);
        let prev_hard_break = if line_start < line_end && line_end <= prev.visual_lines.len() {
            prev.visual_lines[line_end - 1].hard_break
        } else {
            false
        };
        if prev_hard_break != new_hard_break {
            return false;
        }
    }
    true
}

/// Issue #901: 增量排版实现 — 只重排受影响段落，其他段落复用已有 shaping/行数据/纹理。
///
/// # 核心逻辑
///
/// 1. **全局重排条件**：`paragraph_store` 为空、排版参数变化、或无受影响范围时，
///    走全篇重排（`prepare_document_visual_snapshot_impl`），然后重建 store 和 height_index。
/// 2. **增量重排**：
///    - 用 `ParagraphLayoutStore` 判断哪些段落需要重排（cache_key 不匹配或未排版）。
///    - 受影响段落（包括手动 Enter 拆段、Backspace 合段）重新走 QTextLayout。
///    - 其他段落复用 `previous_snapshot` 的 VisualLine（只更新 y 坐标和 byte offset）。
///    - 用 `ParagraphHeightIndex` 计算每段 y 坐标（前缀和），不逐段存储 y。
///    - 段落高度变化只改受影响段的数据（Fenwick 树单点更新 O(log n)）。
///    - **不能因其高度变化，顺带重新 shaping 后面所有段落。**
///
/// # 参数
///
/// - `affected_byte_start/end`：Core 提供的受影响字节范围（DisplayPatch 的 replace_byte_range）。
/// - `previous_snapshot`：上一次排版的 snapshot，用于复用不受影响段落的 VisualLine。
/// - `paragraph_store`：段落布局存储，判断哪些段落需要重排。
/// - `height_index`：段落高度索引，计算 y 坐标和更新高度。
pub(super) fn prepare_incremental_visual_snapshot_impl(
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
    // Issue #901 问题5: 接收 previous_snapshot 的所有权，使函数内部能 move
    // visual_lines/paragraphs，避免全章 clone。
    previous_snapshot: Option<CanonicalDocumentVisualSnapshot>,
    paragraph_store: &mut super::super::super::paragraph_layout_store::ParagraphLayoutStore,
    height_index: &mut super::super::super::paragraph_height_index::ParagraphHeightIndex,
) -> CanonicalDocumentVisualSnapshot {
    let metrics_h = get_font_ascent(font_family, font_size as f32)
        + get_font_descent(font_family, font_size as f32);
    let line_height = (font_size * line_spacing)
        .max(font_size + 4.0)
        .max(metrics_h);
    let available = (width - padding * 2.0).max(font_size);

    // ── 1. 判断是否需要全局重排 ──
    let store_empty = paragraph_store.is_empty();
    let no_affected_range = affected_byte_start >= affected_byte_end;
    // 检查排版参数是否变化（通过 store 中已有段落的 cache_key 判断）
    let params_changed = check_params_changed(paragraph_store, font_size, font_family, line_spacing, indent, padding, width);

    // Issue #901 第二轮复核问题 1: previous_snapshot 非 None 时不进入全篇重排。
    // no_affected_range（无明确受影响范围）在有 previous_snapshot 时表示"无变化"，
    // 增量排版路径会全部复用 previous_snapshot，不重排任何段落。
    // 只有 store 为空、参数变化、或无 previous_snapshot 时才全篇重排。
    let needs_full_relayout = store_empty || params_changed || previous_snapshot.is_none();

    // Issue #901 第三轮复核问题 4: no_affected_range 且有 previous_snapshot 时，
    // 不能盲目复制旧 layout 并换新 text_revision。
    // 必须区分"真正无正文变化"和"换行结构变化/空行插删/只有换行符变化"：
    // - 如果 prev snapshot 的段落文本与 new text 完全一致 → 真正无变化，安全复用。
    // - 如果不一致 → 即使 affected_range 为空，也必须走增量重排，
    //   否则会把旧排版伪装成新正文（text_revision 标新但 layout 是旧的）。
    if !needs_full_relayout && no_affected_range {
        let prev = previous_snapshot.as_ref().unwrap();
        if prev_snapshot_text_matches(prev, text) {
            // 真正无正文变化：安全复用旧 snapshot，只更新 text_revision 和排版参数。
            return CanonicalDocumentVisualSnapshot {
                text_revision,
                font_size,
                font_family: font_family.to_string(),
                line_spacing,
                text_indent: indent,
                padding,
                width,
                dpr,
                paragraphs: prev.paragraphs.clone(),
                // Issue #901 第九轮阻塞5: Arc::clone 共享视觉行数据。
                visual_lines: Arc::clone(&prev.visual_lines),
                // Issue #901 第十三轮问题1+2+6: 沿用 prev 的 paragraph_offset_index。
                paragraph_offset_index: prev.paragraph_offset_index.clone(),
                height_index: prev.height_index.clone(),
                // Issue #901 第十八轮问题4: 沿用 prev 的 paragraph_id_to_slot 映射。
                // Issue #901 第十九轮问题5: Arc 共享，O(1) clone 而非 O(段落数)。
                paragraph_id_to_slot: std::sync::Arc::clone(&prev.paragraph_id_to_slot),
                // Issue #901 第二十一轮问题2: 沿用 prev 的段落行数前缀索引。
                paragraph_line_count_index: std::sync::Arc::clone(
                    &prev.paragraph_line_count_index,
                ),
            };
        }
        // prev 与 new text 不一致但 affected_range 为空：
        // 这是错误范围/换行结构变化/空行插删的情况，fall through 到增量重排。
        // 增量重排会通过段落文本比较找到真正变化的段落并重排。
    }

    if needs_full_relayout {
        // 全局重排路径：重建 store + 全篇排版 + 重建 height_index
        paragraph_store.rebuild_from_text(
            text,
            font_size,
            font_family,
            line_spacing,
            indent,
            padding,
            width,
        );

        // Issue #901 第二十轮阻塞2: 从 paragraph_store 获取统一分配的 ParagraphId 序列，
        // 传给 prepare_document_visual_snapshot_impl，移除 slot+1 伪造 Id。
        let paragraph_ids: Vec<ParagraphId> = paragraph_store.paragraph_ids();

        let snapshot = prepare_document_visual_snapshot_impl(
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
            &paragraph_ids,
        );

        // 从 snapshot 提取段落高度，重建 height_index
        let heights = extract_paragraph_heights(&snapshot.visual_lines);
        height_index.rebuild(heights);

        // 同步 paragraph_store 的排版结果
        paragraph_store.sync_from_visual_lines(snapshot.visual_lines.as_slice(), generation);

        return snapshot;
    }

    // ── 2. 增量重排路径 ──
    // Issue #901 问题5: 不遍历整章。take prev 的 visual_lines/paragraphs（move，不 clone），
    // 只重排受影响段落，splice 受影响段落的行和段落。
    // 对长章非结构性编辑，每帧成本与受影响段落+后续段行数相关，
    // 不再逐段 to_vec() + clone()。
    let mut prev = match previous_snapshot {
        Some(s) => s,
        None => {
            // 逻辑上不会走到这里（needs_full_relayout 已覆盖 None 情况）。
            return CanonicalDocumentVisualSnapshot {
                text_revision,
                font_size,
                font_family: font_family.to_string(),
                line_spacing,
                text_indent: indent,
                padding,
                width,
                dpr,
                paragraphs: Vec::new(),
                // Issue #901 第九轮阻塞5: Arc 包装空 Vec。
                visual_lines: Arc::new(Vec::new()),
                paragraph_offset_index: Arc::new(ParagraphOffsetIndex::new()),
                height_index: ParagraphHeightIndex::new(),
                // Issue #901 第十八轮问题4: 空映射。
                // Issue #901 第十九轮问题5: Arc 共享空映射。
                paragraph_id_to_slot: std::sync::Arc::new(std::collections::HashMap::new()),
                // Issue #901 第二十轮阻塞1: 空映射。
                // Issue #901 第二十一轮问题2: 空行数索引。
                paragraph_line_count_index: std::sync::Arc::new(
                    ParagraphLineCountIndex::new(),
                ),
            };
        }
    };

    // 用 store 计算受影响段落范围（用于 full_relayout 判断）
    let affected = paragraph_store.compute_affected_paragraphs(affected_byte_start, affected_byte_end);

    // Issue #901 问题5: 段落身份用 paragraph_store 的 ParagraphId 匹配，
    // 不按 prev.paragraphs[current_idx] 下标匹配。
    let prev_para_count = prev.paragraphs.len();

    // Issue #901 第二十轮阻塞5: 全文 split_inclusive 仍是 O(全文 byte) 扫描，
    // 但只找单字节 '\n' 不分配字符串内容，常数小。完全局部化为只 split 受影响范围
    // 需要协调 new_paras 在多处使用（compute_old_new_paragraph_ranges 前后缀匹配、
    // 重排段文本、offset_index rebuild 等），byte 范围计算易出错，重构风险高。
    // 本轮已将更重的热路径——908 行前缀段逐段 utf16 遍历——替换为
    // paragraph_offset_index 的 O(log n) Fenwick 前缀和查询，消除了 O(前缀段总 char 数)
    // 的每笔遍历。ID→slot/line_range 映射通过 Arc 共享（1327-1328 行），
    // height_index/offset_index 增量更新（1248-1272、1299-1312 行），
    // 均不再每笔全量重建。此 split 为剩余优化点，正确性优先保留。
    //
    // Issue #901 第二十一轮问题5: 本轮复核确认保留。offset_index clone 已改为
    // Arc + mem::take + Arc::make_mut（O(1) COW），消除了每笔 O(段落数) 深拷。
    // split_inclusive 的 O(全文 byte) 常数小（单字节 memchr），且局部化需协调
    // 多处 new_paras 使用点，正确性风险高于性能收益，保留标注。
    // Issue #901 第二十三轮问题1+2: 用统一切分器生成 (content, hard_break) 序列，
    // 包括末尾换行产生的尾空段。段落比较保留 hard_break，避免仅插入/删除末尾回车时
    // 按去掉 \n 后的段落正文比较误判"无变化"。
    // Issue #901 第二十五轮复核问题3: 增量路径中仍需全文切分来获取段落文本（重排段
    // 需要段落文本内容）。这是剩余优化点 — 未来可用 Core DisplayPatch 的旧/新 byte
    // 区间和 paragraph_store 的 byte 前缀索引定位受影响段，不每笔生成新全文 Vec。
    // 本轮保留全文切分，因 split_paragraphs_with_hard_break 的 O(全文 byte) 常数小
    //（单字节 memchr），且重排段需要段落文本内容。
    let new_paras = super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text);
    let new_para_count = new_paras.len();

    // Issue #901 第五轮阻塞2: 用段落文本比较计算旧段 splice 范围和新段建立范围。
    // Enter 拆段（旧1段→新2段）和 Backspace 合段（旧2段→新1段）时，
    // 旧删段数不等于新添段数，不能用同一 affected_start..affected_end
    // 同时 splice 旧段数组和新段数组。分别用两套独立范围：
    // - old_splice_start..old_splice_end：在旧段数组中删除的范围
    // - new_layout_start..new_layout_end：在新段数组中建立（重排版）的范围
    //
    // Issue #901 第二十三轮问题2: 段落身份比较包含 hard_break。
    // new_paras 已含 hard_break（来自 split_paragraphs_with_hard_break）。
    // old_paras 从 prev.paragraphs 获取 content，从 prev.visual_lines 获取 hard_break。
    let new_para_texts: Vec<&str> = new_paras.iter().map(|(p, _)| *p).collect();
    let old_paras: Vec<(&str, bool)> = (0..prev_para_count)
        .map(|i| {
            let content = prev.paragraphs[i].paragraph_text.as_str();
            let (line_start, line_end) = prev
                .paragraph_line_count_index
                .paragraph_line_range(i);
            let hard_break = if line_start < line_end
                && line_end <= prev.visual_lines.len()
            {
                prev.visual_lines[line_end - 1].hard_break
            } else {
                false
            };
            (content, hard_break)
        })
        .collect();

    let splice_ranges = if affected.full_relayout {
        ParagraphSpliceRanges {
            old_splice_start: 0,
            old_splice_end: prev_para_count,
            new_layout_start: 0,
            new_layout_end: new_para_count,
        }
    } else {
        ParagraphLayoutStore::compute_old_new_paragraph_ranges(
            &old_paras,
            &new_paras,
        )
    };

    // Issue #901 第五轮阻塞2: 用旧段范围找旧视觉行范围，用新段范围建立新段。
    let old_splice_start = splice_ranges.old_splice_start;
    let old_splice_end = splice_ranges.old_splice_end;
    let new_layout_start = splice_ranges.new_layout_start;
    let new_layout_end = splice_ranges.new_layout_end;

    // Issue #901 第二十四轮问题4: 提前计算 vl_start_idx/vl_end_idx，
    // 在 store 更新之前取走并 drop prev.paragraph_line_count_index，
    // 使 paragraph_store.line_count_index 的 strong_count 降为 1，消除 COW 深复制。
    // 此前 store 更新（rebuild_from_text/update_affected_paragraph_texts）在 take 之前执行，
    // 此时 prev 仍持有旧 Arc 的 clone，strong_count=2，Arc::make_mut 必须深复制。
    //
    // Issue #901 第二十一轮问题3: vl_start_idx/vl_end_idx 改用 prev snapshot 的
    // 段落行数前缀索引（Fenwick 树）查询，而非对 prev.visual_lines 做
    // `find_line_range_by_logical_slot` 二分查找。
    // 前缀和由段内行数维护，不依赖 raw `logical_slot`，且后缀段范围自动平移。
    //
    // Issue #901 第六轮阻塞2: 纯尾部插入（old_splice_start == prev.paragraphs.len()）
    // 时 vl_start_idx/vl_end_idx 必须是 visual_lines.len()..visual_lines.len()，
    // 在末尾追加不删除任何旧行。paragraph_line_start 越界返回 total_lines，正好满足。
    let prev_vls_len = prev.visual_lines.len();
    let vl_start_idx: usize = prev
        .paragraph_line_count_index
        .paragraph_line_start(old_splice_start);
    let vl_end_idx: usize = prev
        .paragraph_line_count_index
        .paragraph_line_start(old_splice_end);
    // 安全 clamp：前缀和不应超过 visual_lines 长度（防御性，保证不越界）。
    let vl_start_idx = vl_start_idx.min(prev_vls_len);
    let vl_end_idx = vl_end_idx.min(prev_vls_len);

    // Issue #901 第二十五轮复核问题2: prev_height_index 的 clone 已延迟到
    // curr_para_count != prev_para_count 结构变化分支内（见下方），普通编辑（无回车）
    // 不再每笔 clone 整章高度索引 O(段落数)。

    // Issue #901 第二十四轮问题4: 取走 prev 的 line_count_index Arc 并显式释放，
    // 使 paragraph_store.line_count_index 的 strong_count 降为 1。
    // prev.paragraph_line_count_index 已在上方 vl_start_idx/vl_end_idx 计算和
    // old_paras 构建中使用完毕，后续不再直接消费（新 snapshot 从
    // paragraph_store.line_count_index_arc() 获取）。
    // 必须在 store 更新之前 drop，否则 store 更新中的 Arc::make_mut 因 strong_count=2 深复制。
    let prev_line_count_index = std::mem::take(&mut prev.paragraph_line_count_index);
    drop(prev_line_count_index);

    // 增量更新 store 的文本信息
    // Issue #901 第二十四轮问题4: 移到 take/drop prev.paragraph_line_count_index 之后，
    // 此时 paragraph_store.line_count_index 的 strong_count=1，Arc::make_mut 原地改不 COW 深复制。
    {
        let store_needs_rebuild = paragraph_store.paragraph_count() != new_paras.len()
            || prev_para_count != new_paras.len();

        if store_needs_rebuild {
            paragraph_store.rebuild_from_text(
                text,
                font_size,
                font_family,
                line_spacing,
                indent,
                padding,
                width,
            );
        } else {
            // Issue #901 第五轮阻塞2: 用新段范围更新 store，不用 affected byte range。
            paragraph_store.update_affected_paragraph_texts(
                &new_para_texts,
                new_layout_start,
                new_layout_end,
                font_size,
                font_family,
                line_spacing,
                indent,
                padding,
                width,
            );
        }
    }

    // Issue #901 第十轮问题5: 不再构建 O(all_lines) HashMap，
    // 用二分查找 O(log n) 按需定位段落行范围。

    // Issue #901 问题5: take prev 的 visual_lines 和 paragraphs（move，不 clone）。
    // 后续只 splice 受影响段落的行和段落，未修改段直接复用 move 出的数据。
    // Issue #901 第九轮阻塞5: prev.visual_lines 是 Arc<Vec>，用 Arc::make_mut 取 &mut Vec
    //（refcount==1 原地改，refcount>1 COW），再 mem::take 取出 Vec 做 splice。
    // prev 从 mem::take(&mut self.last_snapshot) 取走，self.cache 已 None，refcount==1，
    // Arc::make_mut 原地改不产生 Vec clone。

    // Issue #901 第十三轮问题1+2+6: SuffixTransform 物化逻辑已移除。
    // prev.visual_lines 全部是已物化的绝对坐标，直接 take 使用。

    let mut prev_paragraphs: Vec<CanonicalParagraphSnapshot> = std::mem::take(&mut prev.paragraphs);
    let mut visual_lines: Vec<VisualLine> =
        std::mem::take(Arc::make_mut(&mut prev.visual_lines));

    // Issue #901 第十三轮问题1+2+6: vl_start_idx 和 vl_end_idx 用
    // find_line_range_by_para_start 一次匹配计算（prev.visual_lines 全部物化）。
    // 不需要重新计算。物化后 [t.start_line_idx, vl_end_idx) 范围的行已更新为
    // 正确绝对坐标，find_line_range_by_para_start 用新 para_start 能匹配到同样的索引。

    // 计算受影响段落的旧高度和旧 byte 范围
    let old_affected_height: f64 = visual_lines[vl_start_idx..vl_end_idx]
        .iter()
        .map(|vl| vl.height)
        .sum();
    let old_affected_byte_start: usize = visual_lines.get(vl_start_idx)
        .map(|vl| vl.byte_start)
        .unwrap_or(0);

    // Issue #901 第六轮阻塞2: Enter 在文末加段时，前缀公共段最后一段的末行
    // hard_break 需要从 false→true 同步重建。前缀公共段（0..new_layout_start）
    // 复用 prev 视觉行，但其末行 hard_break 可能因新段加入而变化：旧段原为
    // 文档末尾无 \n（hard_break=false），新段加入后该段以 \n 结尾（hard_break=true）。
    // 只有紧邻差异段的前缀公共段最后一段（index = new_layout_start - 1）可能受影响；
    // 更前面的段若文本相同且都以 \n 结尾，hard_break 不变。
    // 该行位于 visual_lines[vl_start_idx - 1]，不在 splice 范围
    // [vl_start_idx, vl_end_idx) 内，可安全就地更新。
    if new_layout_start > 0 && vl_start_idx > 0 && new_layout_start <= new_paras.len() {
        let prev_seg_idx = new_layout_start - 1;
        let new_hard_break = new_paras[prev_seg_idx].1;
        visual_lines[vl_start_idx - 1].hard_break = new_hard_break;
    }

    // 重新排版受影响段落
    let mut new_visual_lines: Vec<VisualLine> = Vec::new();
    let mut new_paragraphs: Vec<CanonicalParagraphSnapshot> = Vec::new();
    let mut relayouted_indices: Vec<usize> = Vec::new();

    // 计算受影响段落的起始 y 和起始 byte offset
    // Issue #901 第十四轮问题1: 段内相对存储 — y_start 用 height_index 计算绝对 y。
    // Issue #901 第二十轮阻塞3: 用 logical_slot 查 height_index。
    let y_start: f64 = if vl_start_idx > 0 {
        let prev_line = &visual_lines[vl_start_idx - 1];
        padding + height_index.paragraph_top_y(prev_line.logical_slot as usize) + prev_line.y + prev_line.height
    } else {
        padding
    };

    // 计算受影响段落的起始 paragraph_start 和 paragraph_qchar_start
    // Issue #901 第五轮阻塞2: 用新段范围 new_layout_start 计算起始偏移
    // Issue #901 第二十轮阻塞5: 用 prev 的 paragraph_offset_index 查询前缀和，
    // 替代遍历 new_paras 前缀段。前缀段（0..new_layout_start）内容不变（编辑点之前），
    // prev 的 offset_index 对前缀段的前缀和 = 新前缀和。
    // O(log n) Fenwick 查询替代 O(前缀段总 char 数) 遍历，长篇热路径不再每笔
    // 对前缀段逐段做 len + utf16 计算。new_layout_start <= prev 段落数
    //（由 compute_old_new_paragraph_ranges 的 prefix_common <= min(old,new) 保证），
    // 越界时 paragraph_byte_start 返回 byte_total，安全 fallback。
    let mut paragraph_start: usize =
        prev.paragraph_offset_index.paragraph_byte_start(new_layout_start);
    let mut paragraph_qchar_start: usize =
        prev.paragraph_offset_index.paragraph_qchar_start(new_layout_start);

    let mut y: f64 = y_start;
    let mut line_id: usize = vl_start_idx;
    let mut paragraph_idx: i32 = new_layout_start as i32;
    // Issue #901 第十四轮问题1: 段内相对存储 — 追踪段顶 y 用于计算相对 y。
    let mut paragraph_top_y: f64 = y_start;

    // Issue #901 第五轮阻塞2: 用新段范围 new_layout_start..new_layout_end 建立新段
    for para_idx in new_layout_start..new_layout_end {
        let (paragraph_text, hard_break) = new_paras[para_idx];

        // Issue #901 第十四轮问题1: 每段开始时记录段顶 y。
        paragraph_top_y = y;

        relayouted_indices.push(para_idx);

        // Issue #901 第二十一轮问题1: 获取或分配该段的 Qt 物理槽号。
        // physical_slot 与文档段序（logical_slot）无关，新段分配新槽，
        // 复用段保持已有槽。避免新段覆盖存活段的 QTextLayout。
        let physical_slot = paragraph_store.paragraph_physical_slot(para_idx);

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            let _empty_canonical = prepare_paragraph_visual_snapshot(
                paragraph_text,
                paragraph_idx as usize,
                font_size,
                font_family,
                available,
                indent,
                dpr,
                text_color,
                physical_slot,
                line_spacing,
                generation,
                false,
            );
            new_visual_lines.push(VisualLine {
                id: line_id,
                byte_start: 0,
                byte_end: 0,
                qchar_start: 0,
                qchar_end: 0,
                hard_break,
                x: padding + indent,
                y: 0.0,
                width: 0.0,
                height: line_height,
                para_text: String::new(),
                para_start: paragraph_idx as usize,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 0,
                line_wrap_width: available - indent,
                line_indent_x: indent,
                para_indent: indent,
                x_end_trailing: 0.0,
                qt_ascent: empty_ascent,
                qt_descent: empty_descent,
                // Issue #901 第二十一轮问题1: cache_slot 是独立分配的 Qt 物理槽号，
                // 与文档段序（logical_slot）无关，避免新段覆盖存活段的 QTextLayout。
                cache_slot: physical_slot,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx,
                // Issue #901 第十八轮问题4: 增量排版路径从 paragraph_store 获取稳定段落身份。
                paragraph_id: paragraph_store
                    .paragraph_id_at(para_idx)
                    .unwrap_or(ParagraphId::INVALID),
            });
            line_id += 1;
            y += line_height;

            new_paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_idx as usize,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    0,
                ),
            });

            paragraph_store.update_layout_result(
                para_idx,
                generation,
                physical_slot,
                line_height,
                1,
            );

            paragraph_start += paragraph_text.len() + if hard_break { 1 } else { 0 };
            paragraph_qchar_start += paragraph_text.chars().map(|c| c.len_utf16()).sum::<usize>()
                + if hard_break { 1 } else { 0 };
            paragraph_idx += 1;
            continue;
        }

        let canonical = prepare_paragraph_visual_snapshot(
            paragraph_text,
            paragraph_idx as usize,
            font_size,
            font_family,
            available,
            indent,
            dpr,
            text_color,
            physical_slot,
            line_spacing,
            generation,
            false,
        );

        let line_start_for_store = line_id;
        let mut para_height = 0.0f64;
        let line_count = canonical.lines.len();

        for (line_idx, canonical_line) in canonical.lines.iter().enumerate() {
            let qt_metrics_h = canonical_line.ascent + canonical_line.descent;
            let actual_line_h = if qt_metrics_h > 0.0 {
                line_height.max(qt_metrics_h)
            } else {
                line_height
            };

            let is_first = line_idx == 0;

            new_visual_lines.push(VisualLine {
                id: line_id,
                byte_start: canonical_line.document_byte_start,
                byte_end: canonical_line.document_byte_end,
                qchar_start: canonical_line.qchar_start,
                qchar_end: canonical_line.qchar_end,
                hard_break: hard_break && line_idx == canonical.lines.len() - 1,
                x: padding + canonical_line.x_pos,
                y: y - paragraph_top_y,
                width: canonical_line.width,
                height: actual_line_h,
                para_text: paragraph_text.to_string(),
                para_start: paragraph_idx as usize,
                qtextline_idx: line_idx as i32,
                para_qchar_start: canonical_line.qchar_start,
                para_qchar_end: canonical_line.qchar_end,
                line_wrap_width: if is_first { available - indent } else { available },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                // Issue #901 第二十一轮问题1: cache_slot 是独立分配的 Qt 物理槽号，
                // 与文档段序（logical_slot）无关，避免新段覆盖存活段的 QTextLayout。
                cache_slot: physical_slot,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx,
                // Issue #901 第十八轮问题4: 增量排版路径从 paragraph_store 获取稳定段落身份。
                paragraph_id: paragraph_store
                    .paragraph_id_at(para_idx)
                    .unwrap_or(ParagraphId::INVALID),
            });
            line_id += 1;
            y += actual_line_h;
            para_height += actual_line_h;
        }

        paragraph_store.update_layout_result(
            para_idx,
            generation,
            physical_slot,
            para_height,
            line_count,
        );

        paragraph_start += paragraph_text.len() + if hard_break { 1 } else { 0 };
        paragraph_qchar_start += paragraph_text.chars().map(|c| c.len_utf16()).sum::<usize>()
            + if hard_break { 1 } else { 0 };

        new_paragraphs.push(canonical);
        paragraph_idx += 1;
    }

    // 计算新受影响段落的高度
    let new_affected_height: f64 = new_visual_lines.iter().map(|vl| vl.height).sum();
    let _delta_height = new_affected_height - old_affected_height;

    // Issue #901 第十四轮问题1: 段内相对存储 — 不再需要 old_suffix_byte_start/qchar_start
    // 和 byte_delta/qchar_delta/line_id_delta，因为后缀段的相对坐标不需要更新。
    // 绝对坐标由消费方通过 ParagraphOffsetIndex + ParagraphHeightIndex 按需计算。

    // splice visual_lines：替换受影响段落的行
    let new_affected_line_count = new_visual_lines.len();
    visual_lines.splice(vl_start_idx..vl_end_idx, new_visual_lines);

    // splice paragraphs：替换受影响段落
    // Issue #901 第五轮阻塞2: 用旧段范围 old_splice_start..old_splice_end 切旧段数组，
    // 用新段范围 new_layout_start..new_layout_end 建立新段。
    // Enter 拆段（旧1段→新2段）时 old_splice 范围=[0,1)，new_layout 范围=[0,2)，
    // 旧删1段新添2段；Backspace 合段（旧2段→新1段）时 old_splice=[0,2)，new_layout=[0,1)，
    // 旧删2段新添1段。不能用同一范围同时切新旧段。
    let splice_para_start = old_splice_start.min(prev_paragraphs.len());
    let splice_para_end = old_splice_end.min(prev_paragraphs.len());
    prev_paragraphs.splice(splice_para_start..splice_para_end, new_paragraphs);
    let mut paragraphs = prev_paragraphs;

    // Issue #901 第十四轮问题1: 移除后缀绝对坐标就地更新循环。
    // 段内相对存储后，后缀段的 y/byte/qchar/id/para_start 全部是段内相对坐标，
    // 不随编辑变化，不需要更新。绝对坐标由消费方通过偏移索引按需计算。
    // 同理，后缀段的 paragraph_document_byte_start（现为段落索引）和内部行/cluster
    // 的 document_byte_start/end（现为段内相对偏移）也不需要更新。
    //
    // Issue #901 第十八轮问题4: 不再遍历后缀行更新 cache_slot/para_start。
    // materialize_line 通过 paragraph_id_to_slot 映射在读取时解析 slot，
    // 不需要在编辑时遍历后缀行更新 cache_slot/para_start。
    // 后缀行的 cache_slot 可能过时，但 materialize_line 优先用 paragraph_id 解析。
    // 仍需更新后缀 canonical paragraph 的 paragraph_document_byte_start（段落序号），
    // 因为它被用作段落索引（非 paragraph_id），cursor_x_from_canonical 等消费方按
    // paragraphs[slot] 索引。
    let old_splice_para_count = splice_para_end - splice_para_start;
    let new_splice_para_count = new_layout_end - new_layout_start;
    let para_delta: i64 = new_splice_para_count as i64 - old_splice_para_count as i64;
    if para_delta != 0 {
        // 更新后缀 canonical paragraph 的 paragraph_document_byte_start（段落序号）
        let suffix_para_start = splice_para_start + new_splice_para_count;
        for (i, para) in paragraphs[suffix_para_start..].iter_mut().enumerate() {
            para.paragraph_document_byte_start = suffix_para_start + i;
        }
    }

    // Issue #901 第二十四轮问题2: 删除循环外的空文本处理分支和尾部换行处理分支。
    // 主重排循环 `for para_idx in new_layout_start..new_layout_end` 已用 new_paras
    //（来自 split_paragraphs_with_hard_break，含尾空段和空文本段）统一处理所有段。
    // 循环外的两个独立追加分支会重复创建尾空段/空文本段：
    // - 旧文 甲\n乙\n，开头新插回车变成 \n甲\n乙\n。后缀复用旧尾空段（logical_slot=2），
    //   新 canonical 有 4 段，检查 2 == 4-1 得 false，错误追加第 5 段。
    // - 清空文档：new_paras=[("",false)]，主循环已创建 1 个空段，
    //   if text.is_empty() 又 push 第二个。
    // paragraph_idx 在主循环中已正确递增，此处不再需要重新赋值和追加逻辑。

    // Issue #901 第十四轮问题1: 增量更新 paragraph_offset_index，不扫全文。
    // Issue #901 第二十一轮问题5: 用 Arc 共享 + Arc::make_mut COW。
    // prev 是 move 进来的，prev.paragraph_offset_index 用 mem::take 取出（refcount==1），
    // Arc::make_mut 原地改（O(1)），不深拷 Fenwick 底层数组。
    // 段落数变化时用 rebuild（O(n)），段落数不变时用 update_paragraph_length（O(log n)）。
    let paragraph_offset_index = {
        let mut idx_arc = std::mem::take(&mut prev.paragraph_offset_index);
        let curr_count = paragraphs.len();
        if curr_count != prev_para_count {
            // 段落数变化（Enter 拆段/Backspace 合段），重建偏移索引
            let mut new_idx = ParagraphOffsetIndex::new();
            let mut byte_lengths = Vec::with_capacity(curr_count);
            let mut qchar_lengths = Vec::with_capacity(curr_count);
            for (para_text, hard_break) in new_paras.iter() {
                // 段落 byte/qchar 长度含尾部 '\n'（如果 hard_break）
                byte_lengths.push(para_text.len() + if *hard_break { 1 } else { 0 });
                qchar_lengths.push(
                    para_text.chars().map(|c| c.len_utf16()).sum::<usize>()
                        + if *hard_break { 1 } else { 0 },
                );
            }
            new_idx.rebuild(byte_lengths, qchar_lengths);
            Arc::new(new_idx)
        } else {
            // 段落数不变，COW 增量更新受影响段落的长度（O(k log n)）
            let idx = Arc::make_mut(&mut idx_arc);
            for para_idx in new_layout_start..new_layout_end {
                if para_idx < new_paras.len() {
                    let (para, hard_break) = new_paras[para_idx];
                    let byte_len = para.len() + if hard_break { 1 } else { 0 };
                    let qchar_len = para.chars().map(|c| c.len_utf16()).sum::<usize>()
                        + if hard_break { 1 } else { 0 };
                    idx.update_paragraph_length(para_idx, byte_len, qchar_len);
                }
            }
            idx_arc
        }
    };

    // Issue #901 第十五轮问题2: 先完成本次受影响段落的新高度计算并更新/重建
    // height_index，再构造 canonical snapshot，把同一份已更新的高度索引交给 snapshot。
    // 此前 snapshot 内是旧高度，而 LayoutSnapshot/渲染侧用新高度，导致换行后 y 错位。

    // Issue #901 第三轮复核问题 4: 用实际重排的段落索引驱动后续更新。
    // Issue #901 第五轮阻塞2: 用 new_layout 范围替代 affected byte range 范围，
    // 因为 new_layout 范围精确反映了新段中需要重排的段落。
    let affected_indices: Vec<usize> = if affected.full_relayout {
        (0..paragraphs.len()).collect()
    } else {
        // 合并 new_layout range 和实际重排的段落
        let mut indices: Vec<usize> = (new_layout_start..new_layout_end)
            .chain(relayouted_indices.iter().copied())
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    };

    // Issue #901 第二轮复核: 段落数变化时 height_index 的 ensure_len 用 resize 会导致
    // 高度错位（resize 只在末尾添加/截断，但拆段/合段变化在中间位置）。
    // 段落数变化时用 rebuild 从权威段序构造高度（O(n) 但不涉及 QTextLayout），
    // 段落数不变时用 update_heights_incremental 只更新受影响段（O(k log n)）。
    // prev_para_count 已在 take 之前定义（line 683），此处直接复用。
    //
    // Issue #901 第二十一轮问题5: 段落数变化时（Enter 拆段/Backspace 合段，不频繁），
    // O(段落数) 构造但不涉及 QTextLayout。
    let curr_para_count = paragraphs.len();
    if curr_para_count != prev_para_count {
        // Issue #901 第二十五轮复核问题2: prev_height_index 的 clone 延迟到结构变化分支内，
        // 普通编辑（无回车，curr_para_count == prev_para_count）不 clone 整章高度索引 O(段落数)。
        // 仅在拆段/合段（Enter/Backspace 换行符）时才 clone，这类操作不频繁。
        // Issue #901 第二十四轮问题3: 不从 raw visual_lines.para_start 提取高度，
        // 改为从权威段序构造：前缀未改段从 prev height_index + 新排版段从 new_visual_lines
        // + 后缀未改段从 prev height_index。
        // 增量路径保留了复用后缀的旧 para_start/logical_slot，raw 段号可能重复/错乱
        //（如 甲\n乙 → \n甲\n乙 后 raw logical_slot = [0,0,1]），
        // extract_paragraph_heights 用 raw para_start 去重会把不同段的高度误合并。
        let prev_height_index = prev.height_index.clone();
        let mut heights = vec![0.0f64; curr_para_count];

        // 前缀未改段 [0, new_layout_start)：从 prev height_index 获取
        for i in 0..new_layout_start {
            heights[i] = prev_height_index.height_at(i);
        }

        // 新排版段 [new_layout_start, new_layout_end)：从 splice 后的新行按段聚合。
        // splice 后新行在 [vl_start_idx, vl_start_idx + new_affected_line_count) 范围内，
        // 这些行的 para_start 是主循环中设置的正确新段索引（paragraph_idx）。
        {
            use std::collections::HashMap;
            let mut para_heights: HashMap<usize, f64> = HashMap::new();
            let new_lines_range = vl_start_idx..(vl_start_idx + new_affected_line_count);
            for vl in &visual_lines[new_lines_range] {
                *para_heights.entry(vl.para_start).or_insert(0.0) += vl.height;
            }
            for i in new_layout_start..new_layout_end {
                heights[i] = para_heights.get(&i).copied().unwrap_or(0.0);
            }
        }

        // 后缀未改段 [new_layout_end, curr_para_count)：从 prev height_index 获取。
        // 旧段对应 [old_splice_end, prev_para_count)。
        let suffix_new_start = new_layout_end;
        let suffix_old_start = old_splice_end;
        for (new_i, old_i) in
            (suffix_new_start..curr_para_count).zip(suffix_old_start..prev_para_count)
        {
            heights[new_i] = prev_height_index.height_at(old_i);
        }

        height_index.rebuild(heights);
    } else {
        // 段落数不变，只更新受影响段落的高度（O(k log n)）
        // Issue #901 第十六轮问题3: 从新行范围聚合高度，不遍历全章 visual_lines。
        // splice 后新行在 [vl_start_idx, vl_start_idx + new_affected_line_count) 范围内，
        // 这些行的 logical_slot 是新段索引，从中按段聚合高度是 O(k)。
        let new_lines_range = vl_start_idx..(vl_start_idx + new_affected_line_count);
        let height_updates =
            extract_affected_height_updates(&visual_lines[new_lines_range], &affected_indices);
        height_index.update_heights_incremental(&height_updates, curr_para_count, line_height);
    }

    // Issue #901 第十八轮问题4: 构建 paragraph_id → slot 映射。
    // Issue #901 第二十一轮阻塞5: 从 paragraph_store 持久缓存获取，不每笔遍历整章重建。
    // paragraph_store 在拆段/合段后已更新 ParagraphId 分配和 id_to_slot_cache，
    // 直接 Arc::clone 即可，O(1) 而非 O(段落数)。
    //
    // 先调用 sync_from_visual_lines_incremental 更新 store 的排版结果和 id_to_line_range，
    // 再从 store 获取两个映射，保证映射反映最新排版结果。
    // Issue #901 第二十二轮问题2: 只传本次新排版的行给 sync_from_visual_lines_incremental，
    // 不传全量 visual_lines。splice 后的全量 visual_lines 中，复用后缀段的 raw logical_slot
    // 可能过期/重复（如 甲\n乙 → \n甲\n乙 后 raw logical_slot = [0,0,1]），
    // find_line_range_by_logical_slot 在全量行上二分 idx=0 会把新空段和复用旧"甲"
    // 的行合为一个段。新排版的行（splice 后 [vl_start_idx, vl_start_idx+new_affected_line_count)
    // 范围内）的 logical_slot 是正确的新段索引，只包含受影响段的行，不会有复用后缀行干扰。
    let new_lines_for_sync: &[VisualLine] =
        &visual_lines[vl_start_idx..(vl_start_idx + new_affected_line_count)];
    paragraph_store.sync_from_visual_lines_incremental(
        new_lines_for_sync,
        generation,
        &affected_indices,
    );

    let paragraph_id_to_slot = paragraph_store.paragraph_id_to_slot_arc();
    // Issue #901 第二十一轮问题2: 从 paragraph_store 获取段落行数前缀索引。
    // sync_from_visual_lines_incremental 已更新受影响段的 visual_line_count
    // 和 line_count_index，后缀段行范围由前缀和自动平移。
    // Issue #901 第二十二轮问题4: Arc 共享行数前缀索引，O(1)。
    let paragraph_line_count_index = paragraph_store.line_count_index_arc();

    let snapshot = CanonicalDocumentVisualSnapshot {
        text_revision,
        font_size,
        font_family: font_family.to_string(),
        line_spacing,
        text_indent: indent,
        padding,
        width,
        dpr,
        paragraphs,
        // Issue #901 第九轮阻塞5: Arc 包装视觉行数据。
        visual_lines: Arc::new(visual_lines),
        // Issue #901 第十四轮问题1: 段落前缀偏移索引 — 增量更新，不扫全文。
        paragraph_offset_index,
        // Issue #901 第十五轮问题2: 段落高度索引 — 已在上面更新/重建，
        // 此处 clone 的是更新后的高度索引，保证 snapshot 内高度与渲染侧一致。
        // Issue #901 第二十五轮复核问题2: 保留 clone 而非 Arc 共享。
        // height_index 是 EditorLayout 的可变权威源（每笔编辑 rebuild/update_heights_incremental
        // 原地修改），snapshot 需要 owned 副本以隔离后续编辑。改 Arc 需把 EditorLayout.height_index
        // 和两个 snapshot 结构字段都改 Arc<ParagraphHeightIndex>，并在本函数增加 take/drop
        // prev.height_index Arc 的 COW 管理（类似 line_count_index），涉及多消费方，留作后续优化。
        height_index: height_index.clone(),
        paragraph_id_to_slot,
        paragraph_line_count_index,
    };

    // Issue #901 第二十五轮复核问题1: 增量排版完成后释放已无持有者的旧 QTextLayout。
    // retired slots 在 update_paragraph_text/split_paragraph/merge_paragraphs 中收集，
    // 此时新 snapshot 已构建，旧 layout 不再被任何 snapshot 引用，可安全释放。
    // 槽号本身不复用（next_physical_slot 持续递增），但 QTextLayout 对象内存不再泄漏。
    paragraph_store.release_retired_layouts();

    snapshot
}

/// 检查排版参数是否变化。
///
/// `EditorLayout::snapshot` 在参数变化时已经调用 `invalidate()` 清空 store，
/// 所以当 store 非空时参数一定没变。此函数始终返回 `false`，
/// 参数变化由 `store_empty` 条件覆盖（参数变化 → invalidate → store 为空）。
fn check_params_changed(
    _paragraph_store: &super::super::super::paragraph_layout_store::ParagraphLayoutStore,
    _font_size: f64,
    _font_family: &str,
    _line_spacing: f64,
    _indent: f64,
    _padding: f64,
    _width: f64,
) -> bool {
    false
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
                    // Issue #901 第十七轮问题1: 用 logical_slot（文档次序/段落编号）匹配，
                    // 不用 para_start（全文 byte offset），与 paragraph_document_byte_start
                    // 现已是段落编号语义一致。
                    // Issue #901 第二十轮阻塞1+3: 改用 logical_slot 而非 cache_slot。
                    let prev_para_slot = prev.paragraphs[pidx].paragraph_document_byte_start;
                    prev.visual_lines
                        .iter()
                        .filter(|l| l.logical_slot as usize == prev_para_slot)
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                };

                if !prev_lines.is_empty() {
                    if let Some(first_prev) = prev_lines.first() {
                        let y_offset = y - first_prev.y;
                        // Issue #901 第十七轮问题1: byte_offset 用于更新复用行的
                        // para_start/byte_start/byte_end（全文 byte offset），
                        // 需要全文 byte offset 差量。用 paragraph_offset_index 获取
                        // prev 该段的全文 byte start（权威来源），不用 first_prev.para_start
                        //（可能是段落编号而非全文 byte offset）。
                        // prev_lines 非空意味着 prev_para_idx 为 Some（prev_lines 只在
                        // prev_para_idx=Some 分支才可能非空），用 if let 安全解构。
                        let prev_para_byte_start_actual = match prev_para_idx {
                            Some(pidx) if !prev.paragraph_offset_index.is_empty() => {
                                prev.paragraph_offset_index.paragraph_byte_start(pidx)
                            }
                            _ => first_prev.para_start,
                        };
                        let byte_offset =
                            paragraph_start as i64 - prev_para_byte_start_actual as i64;
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
                    // Issue #901 第十七轮问题1: byte_offset 用于更新 lines/clusters
                    // 的 document_byte_start/end（全文 byte offset），需要全文 byte offset
                    // 差量。用 paragraph_offset_index 获取 prev 该段的全文 byte start。
                    // paragraph_document_byte_start 现是段落编号，不能直接用于 byte offset 计算。
                    let prev_para_byte_start_actual =
                        if !prev.paragraph_offset_index.is_empty() {
                            prev.paragraph_offset_index.paragraph_byte_start(pidx)
                        } else {
                            paragraph_start
                        };
                    let byte_offset =
                        paragraph_start as i64 - prev_para_byte_start_actual as i64;
                    // Issue #901 第十七轮问题1: paragraph_document_byte_start 现是段落编号，
                    // 直接赋值 current_para_idx，不用 paragraph_start（全文 byte offset）。
                    para.paragraph_document_byte_start = current_para_idx;
                    if byte_offset != 0 {
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
                        // Issue #901 第十七轮问题1: paragraph_document_byte_start
                        // 现是段落编号（slot），不是全文 byte offset。
                        paragraph_document_byte_start: current_para_idx,
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
                        logical_slot: current_para_idx as i32,
                        // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
                        paragraph_id: ParagraphId::INVALID,
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
                            logical_slot: current_para_idx as i32,
                            // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
                            paragraph_id: ParagraphId::INVALID,
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
            logical_slot: current_para_idx as i32,
            // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
            paragraph_id: ParagraphId::INVALID,
        });
            line_id += 1;
            y += line_height;

            paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                // Issue #901 第十七轮问题1: paragraph_document_byte_start
                // 现是段落编号（slot），不是全文 byte offset。
                paragraph_document_byte_start: current_para_idx,
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
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: current_para_idx as i32,
                // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
                paragraph_id: ParagraphId::INVALID,
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
            // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
            logical_slot: current_para_idx as i32,
            // Issue #901 第十八轮问题4: 尾部换行新空段，用 INVALID。
            paragraph_id: ParagraphId::INVALID,
        });
    }

    let height_index = build_height_index_from_lines(&visual_lines);
    // Issue #901 第十八轮问题4: 构建 paragraph_id → slot 映射。
    // 此路径无 paragraph_store，从 visual_lines 的 paragraph_id 构建。
    // Issue #901 第十九轮问题5: Arc 共享，避免 to_layout_snapshot 中 clone O(段落数)。
    let paragraph_id_to_slot =
        std::sync::Arc::new(build_paragraph_id_to_slot_from_lines(&visual_lines));
    // Issue #901 第二十二轮问题4: Arc 包装行数前缀索引，O(1) 共享。
    let paragraph_line_count_index = std::sync::Arc::new(build_line_count_index_from_lines(&visual_lines));
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
        // Issue #901 第九轮阻塞5: Arc 包装视觉行数据。
        visual_lines: Arc::new(visual_lines),
        // Issue #901 第十三轮问题1+2+6: 段落前缀偏移索引。
        paragraph_offset_index: Arc::new(build_offset_index_from_text(text)),
        height_index,
        paragraph_id_to_slot,
        paragraph_line_count_index,
    }
}
