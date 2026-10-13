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
            // Issue #901 第三十轮问题2: 空段不分配 QTextLayout physical_slot。
            // 空段没有 QTextLayout 对象，不需要 cache_slot >= 0，也不需要 layout_epoch。
            // VisualLine 仍创建（有高度、光标几何），但 cache_slot = -1, layout_epoch = 0。
            // hit-test 通过 para_text.is_empty() 跳过 Qt layout 查询，
            // 渲染通过 text_lens[i] == 0 跳过 layout 检查，get_paragraph_layout(gen, -1) 返回 nullptr。
            // 这样没有对象就没有需要登记、回收的物理槽，消除空段物理槽泄漏。
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
                // Issue #901 第三十轮问题2: 空段不分配物理槽，cache_slot = -1。
                cache_slot: -1,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx,
                // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId，
                // 不再用 slot+1 伪造 Id。fallback 到 INVALID。
                paragraph_id: paragraph_ids
                    .get(paragraph_idx as usize)
                    .copied()
                    .unwrap_or(ParagraphId::INVALID),
                // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
                // has_qt_layout = false，渲染路径跳过 layout 查询。
                has_qt_layout: false,
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

        // Issue #901 第二十九轮复核阻塞1: 全篇排版也通过 acquire_physical_slot_for_generation
        // 分配物理槽（不再直接用 paragraph_idx），next_slot 正确递增。
        let physical_slot =
            super::super::super::qt_cache::acquire_physical_slot_for_generation(generation);
        // Issue #901 第三十轮问题1: 获取 epoch，供 VisualLine.layout_epoch 校验。
        let layout_epoch =
            super::super::super::qt_cache::get_layout_epoch(generation, physical_slot);
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
                // Issue #901 第二十九轮复核阻塞1: cache_slot 是 acquire 分配的物理槽，
                // 不再用 paragraph_idx - 1，next_slot 正确递增。
                cache_slot: physical_slot,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx - 1,
                // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId，
                // 不再用 slot+1 伪造 Id。paragraph_idx 已递增，当前段索引为 paragraph_idx - 1。
                paragraph_id: paragraph_ids
                    .get((paragraph_idx - 1) as usize)
                    .copied()
                    .unwrap_or(ParagraphId::INVALID),
                // Issue #901 第三十轮问题1: layout_epoch 供光标/命中/渲染校验。
                layout_epoch,
                // Issue #901 第三十一轮问题1: 非空段有 QTextLayout，
                // has_qt_layout = true，走 get_paragraph_layout_checked。
                has_qt_layout: true,
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
        // Issue #901 第三十轮问题2: 空文本不分配 QTextLayout physical_slot。
        // 空段 cache_slot = -1, layout_epoch = 0，不调用 prepare_paragraph_visual_snapshot。
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
            // Issue #901 第三十轮问题2: 空段不分配物理槽。
            cache_slot: -1,
            logical_slot: 0,
            // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId。
            paragraph_id: paragraph_ids
                .get(0)
                .copied()
                .unwrap_or(ParagraphId::INVALID),
            // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
            layout_epoch: 0,
            // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
            // has_qt_layout = false，渲染路径跳过 layout 查询。
            has_qt_layout: false,
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
        // Issue #901 第三十轮问题2: 尾空段不分配 QTextLayout physical_slot。
        // 空段 cache_slot = -1, layout_epoch = 0，不调用 prepare_paragraph_visual_snapshot。
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
            // Issue #901 第三十轮问题2: 空段不分配物理槽。
            cache_slot: -1,
            logical_slot: paragraph_idx,
            // Issue #901 第二十轮阻塞2: 从 paragraph_ids 获取稳定 ParagraphId。
            paragraph_id: paragraph_ids
                .get(paragraph_idx as usize)
                .copied()
                .unwrap_or(ParagraphId::INVALID),
            // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
            layout_epoch: 0,
            // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
            // has_qt_layout = false，渲染路径跳过 layout 查询。
            has_qt_layout: false,
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

    // Issue #901 第二十九轮阻塞4: Arc 包装高度索引，O(1) 共享前缀。
    let height_index = std::sync::Arc::new(build_height_index_from_lines(&visual_lines));
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

/// Issue #901 第三十三轮问题1: 从已有 `VisualLine` 数组直接组装
/// `CanonicalDocumentVisualSnapshot`，**不调用任何
/// `acquire_physical_slot_for_generation` 或 `install_layout_slot_gen`**。
///
/// `build_canonical_snapshot_for_current_layout` 的正常路径（有 prepared_handle 时）
/// 改用本函数，复用 `EditorLayout` 当前已排版的 `VisualLine` 几何，不再在
/// `handle.generation` 上整章安装独立 `QTextLayout`。
///
/// # 为什么不调用 acquire/install
///
/// `prepare_document_visual_snapshot_scoped` 进入 `prepare_document_visual_snapshot_impl`，
/// 对整章每个非空段调 `acquire_physical_slot_for_generation(generation)` +
/// `prepare_paragraph_visual_snapshot`（C++ `install_layout_slot_gen` 创建每段
/// refcount=1 的新 `QTextLayout`）。这些新槽与 `EditorLayout.paragraph_store`
/// 原本保存的物理槽不同，且正常路径 `fallback_gen` 为 `None` 不会
/// `retire_temporary_generation`，新布局无 owner，随事务次数增长泄漏 Qt 内存及槽数组。
///
/// 本函数直接从 `PreparedLayoutHandle.lines`（`EditorLayout` 已排版的视觉行，
/// 其 `cache_slot` 指向 `paragraph_store` 已登记的物理槽，有正确 owner）读取几何，
/// 不创建任何新 `QTextLayout`，不泄漏。
///
/// # clusters / cursor_x_map
///
/// `VisualLine` 不携带 cluster / cursor_x_map 信息，本函数产出的
/// `CanonicalLineSnapshot.clusters` / `cursor_x_map` 为空 `Vec`。
/// - cluster 由基础 canonical 排版（`EditorLayout.snapshot`）直接产出并存储在
///   `EditorLayout` 的 canonical snapshot 中；本函数产出的 snapshot 用于
///   `reconcile_active_transactions_with_new_canonical`，cluster 缺失时 rebind
///   回退到 Snap（正确性降级，非错误），不会崩溃。
/// - `image` 由调用方后续 `inject_animation_visuals_into_snapshot` 注入。
/// - 几何字段（x/y/width/height/ascent/descent/byte/qchar offset）从 `VisualLine`
///   直接读取，与 `EditorLayout` 已排版结果一致。
///
/// # 参数
///
/// - `text`：全文文本，用于构建 `paragraph_offset_index`（段首 byte/qchar 前缀和）。
/// - `visual_lines`：`EditorLayout` 当前已排版的视觉行切片（`PreparedLayoutHandle.lines`）。
/// - 其余排版参数用于填充 `CanonicalDocumentVisualSnapshot` 的元数据字段。
pub fn build_snapshot_from_prepared_lines(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    visual_lines: &[VisualLine],
) -> CanonicalDocumentVisualSnapshot {
    let mut paragraphs: Vec<CanonicalParagraphSnapshot> = Vec::new();

    // 按 logical_slot 分组（同段连续，与 build_line_count_index_from_lines 一致）。
    // 每组对应一个段落，构建一个 CanonicalParagraphSnapshot。
    let mut idx = 0usize;
    while idx < visual_lines.len() {
        let first = &visual_lines[idx];
        let slot = first.logical_slot;
        let para_text = first.para_text.clone();
        let para_start = first.para_start;
        // 收集同段所有行的索引范围 [idx, end)。
        let mut end = idx + 1;
        while end < visual_lines.len() && visual_lines[end].logical_slot == slot {
            end += 1;
        }

        // 构建 CanonicalLineSnapshot 列表。
        // 空段（para_text 为空）lines 为空 Vec，与 prepare_document_visual_snapshot_impl
        // 一致（空段 VisualLine 存在但 CanonicalParagraphSnapshot.lines 为空）。
        let mut lines: Vec<CanonicalLineSnapshot> = Vec::new();
        if !para_text.is_empty() {
            lines.reserve(end - idx);
            for vl in &visual_lines[idx..end] {
                // VisualLine.x = padding + canonical_line.x_pos → x_pos = x - padding。
                // 排版时 x_pos >= 0，所以 vl.x >= padding；max(0.0) 防浮点误差。
                let x_pos = (vl.x - padding).max(0.0);
                lines.push(CanonicalLineSnapshot {
                    qchar_start: vl.qchar_start,
                    qchar_end: vl.qchar_end,
                    document_byte_start: vl.byte_start,
                    document_byte_end: vl.byte_end,
                    x_pos,
                    width: vl.width,
                    ascent: vl.qt_ascent,
                    descent: vl.qt_descent,
                    x_end_trailing: vl.x_end_trailing,
                    image: None,
                    clusters: Vec::new(),
                    cursor_x_map: Vec::new(),
                    // paragraph_document_byte_start 存段落索引（与 full.rs 历史语义一致）。
                    paragraph_document_byte_start: para_start,
                    qtextline_idx: vl.qtextline_idx,
                });
            }
        }

        let index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
            &para_text,
            0,
        );
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: para_text,
            paragraph_document_byte_start: para_start,
            lines,
            index_map,
        });
        idx = end;
    }

    // 边界：visual_lines 为空（极端情况，正常路径 prepared_handle.lines 应非空）。
    // 确保 paragraphs 至少有一个空段，使 snapshot 结构有效。
    if paragraphs.is_empty() {
        paragraphs.push(CanonicalParagraphSnapshot {
            paragraph_text: String::new(),
            paragraph_document_byte_start: 0,
            lines: Vec::new(),
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build("", 0),
        });
    }

    // 复用 full.rs 已有 helper 构建索引，保证与 prepare_document_visual_snapshot_impl
    // 产出的索引语义一致。
    let visual_lines_vec: Vec<VisualLine> = visual_lines.to_vec();
    let height_index = std::sync::Arc::new(build_height_index_from_lines(&visual_lines_vec));
    let paragraph_id_to_slot = std::sync::Arc::new(build_paragraph_id_to_slot_from_lines(
        &visual_lines_vec,
    ));
    let paragraph_line_count_index = std::sync::Arc::new(build_line_count_index_from_lines(
        &visual_lines_vec,
    ));
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
        visual_lines: Arc::new(visual_lines_vec),
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
    // Issue #901 第二十六轮复核缺口4: Core 提供的受影响文本字节范围，
    // 用于增量拼接 new_paras，避免每笔 split_paragraphs_with_hard_break 全文切分。
    // None 或范围不合理时回退到全文切分。
    affected_text_range: Option<(usize, usize)>,
    // Issue #901 问题5: 接收 previous_snapshot 的所有权，使函数内部能 move
    // visual_lines/paragraphs，避免全章 clone。
    previous_snapshot: Option<CanonicalDocumentVisualSnapshot>,
    paragraph_store: &mut super::super::super::paragraph_layout_store::ParagraphLayoutStore,
    height_index: &mut std::sync::Arc<super::super::super::paragraph_height_index::ParagraphHeightIndex>,
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
                // Issue #901 第二十九轮复核阻塞4: Arc::clone 共享，O(1)。
                height_index: std::sync::Arc::clone(&prev.height_index),
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
        std::sync::Arc::make_mut(height_index).rebuild(heights);

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
                height_index: std::sync::Arc::new(ParagraphHeightIndex::new()),
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
    // Issue #901 第三十二轮问题3: 用 prev.paragraph_offset_index 的 O(log n)
    // byte_to_paragraph 二分定位替换 compute_affected_paragraphs 的全文遍历。
    // 此处处于增量路径，paragraph_store 与 prev 描述同一编辑前段落结构，
    // 段落数一致。prev 已于上方 move 出来，此处不可变借用其 paragraph_offset_index
    // 不影响后续 mem::take。offset_index 为空/不一致时 via_index 内部回退旧逻辑。
    let affected = paragraph_store.compute_affected_paragraphs_via_index(
        affected_byte_start,
        affected_byte_end,
        &*prev.paragraph_offset_index,
    );

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
    // Issue #901 第二十三轮问题1+2: 用统一切分器生成 (content, hard_break) 序列，
    // 包括末尾换行产生的尾空段。段落比较保留 hard_break，避免仅插入/删除末尾回车时
    // 按去掉 \n 后的段落正文比较误判"无变化"。
    // Issue #901 第二十六轮复核缺口4 + 第二十七轮复核问题1+4: 当有 Core 提供的
    // 受影响范围（affected_text_range）且 paragraph_store 非空时，用增量拼接构建
    // new_paras，避免每笔全文切分：
    // - 受影响范围扩展到完整段落边界，之前的未改段（前缀段）从 entries 累计长度
    //   定位切片获取（不重新切分，不依赖可能过时的 document_byte_start）
    // - 扩展后的完整段落区域按真实换行边界切分（第二十八轮复核问题1修复：
    //   不再调用 split_paragraphs_with_hard_break，避免局部切片以 \n 结尾时
    //   产生不存在的尾空段；只有 expanded_end == text_len 时才产生尾空段）
    // - 扩展区域之后的未改段（后缀段）从 entries 累计长度 + delta 定位切片获取
    // 当 affected_text_range 不存在、范围不合理或边界检查失败时，回退全文切分。
    // Issue #901 第二十八轮复核问题4: build_new_paras_from_affected_range 额外返回
    // (prefix_count, suffix_count)，供直接构造 ParagraphSpliceRanges，避免全篇
    // old_paras 构造和逐段比较。splice_info 为 None 时回退到全篇比较。
    // Issue #901 第二十九轮复核阻塞4: 传入 prev.paragraph_offset_index 供 O(1)/O(log n)
    // 查询旧文总长度和前缀/后缀段定位，避免遍历全体 store entries。
    // Issue #901 第三十轮问题4: 增量路径只含受影响段（ParasSource::Incremental），
    // 不构造整章 new_paras/new_para_texts。前缀/后缀段由调用方从 prev 复用。
    // 回退路径（边界情况）仍用整章切分（ParasSource::Full）。
    let paras_source = {
        let store = &*paragraph_store;
        let offset_index = &*prev.paragraph_offset_index;
        if let Some((aff_start, aff_end)) = affected_text_range {
            if aff_start < aff_end && aff_end <= text.len() && !store.is_empty() {
                match build_new_paras_from_affected_range(
                    text,
                    store,
                    offset_index,
                    aff_start,
                    aff_end,
                ) {
                    Some((pc, sc, affected)) => ParasSource::Incremental {
                        prefix_count: pc,
                        suffix_count: sc,
                        affected,
                    },
                    None => ParasSource::Full(
                        super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text),
                    ),
                }
            } else {
                ParasSource::Full(
                    super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text),
                )
            }
        } else {
            ParasSource::Full(
                super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text),
            )
        }
    };
    let new_para_count = paras_source.para_count();

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
    // Issue #901 第三十轮问题4: 不构造整章 new_para_texts。增量路径只含受影响段文本。
    let splice_info = paras_source.splice_info();

    let splice_ranges = if affected.full_relayout {
        ParagraphSpliceRanges {
            old_splice_start: 0,
            old_splice_end: prev_para_count,
            new_layout_start: 0,
            new_layout_end: new_para_count,
        }
    } else if let Some((prefix_count, suffix_count)) = splice_info
        .filter(|_| paragraph_store.paragraph_count() == prev_para_count)
    {
        // Issue #901 第二十八轮复核问题4: 直接从 prefix_count/suffix_count 计算
        // splice 范围，避免构造全篇 old_paras 和逐段比较。
        // 前缀段和后缀段内容不变，old_splice_start == new_layout_start == prefix_count，
        // old_splice_end = prev_para_count - suffix_count，
        // new_layout_end = new_para_count - suffix_count。
        // 仅当 store.paragraph_count() == prev_para_count 时使用（确保旧段数一致）。
        ParagraphSpliceRanges {
            old_splice_start: prefix_count,
            old_splice_end: prev_para_count - suffix_count,
            new_layout_start: prefix_count,
            new_layout_end: new_para_count - suffix_count,
        }
    } else {
        // 回退到全篇比较：构造 old_paras 并用 compute_old_new_paragraph_ranges。
        // Issue #901 第三十轮问题4: Full 路径才有整章 new_paras；Incremental 路径
        // 走 splice_info 分支，不会到达这里。如果到达这里（理论上不应发生），
        // 从 store entries 构造 old_paras，new_paras 从 text 全文切分。
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
        let new_paras_full: Vec<(&str, bool)> = paras_source.full_paras().cloned().unwrap_or_else(|| {
            super::super::super::paragraph_layout_store::split_paragraphs_with_hard_break(text)
        });
        ParagraphLayoutStore::compute_old_new_paragraph_ranges(
            &old_paras,
            &new_paras_full,
        )
    };

    // Issue #901 第五轮阻塞2: 用旧段范围找旧视觉行范围，用新段范围建立新段。
    let old_splice_start = splice_ranges.old_splice_start;
    let old_splice_end = splice_ranges.old_splice_end;
    let new_layout_start = splice_ranges.new_layout_start;
    let new_layout_end = splice_ranges.new_layout_end;

    // Issue #901 第三十轮问题4: 只构造受影响段文本（不构造整章 new_para_texts）。
    // update_affected_paragraph_texts 只访问 new_layout_start..new_layout_end 范围。
    // affected_paras_list 含 (text, hard_break)，供 splice_affected_entries 和
    // offset_index splice 使用。
    let affected_paras_list: Vec<(&str, bool)> =
        paras_source.affected_paras(new_layout_start, new_layout_end);
    let new_para_texts: Vec<&str> =
        affected_paras_list.iter().map(|(p, _)| *p).collect();

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
    // splice_ranges 回退分支的 old_paras 构建中使用完毕，后续不再直接消费（新 snapshot 从
    // paragraph_store.line_count_index_arc() 获取）。
    // 必须在 store 更新之前 drop，否则 store 更新中的 Arc::make_mut 因 strong_count=2 深复制。
    let prev_line_count_index = std::mem::take(&mut prev.paragraph_line_count_index);
    drop(prev_line_count_index);

    // 增量更新 store 的文本信息
    // Issue #901 第二十四轮问题4: 移到 take/drop prev.paragraph_line_count_index 之后，
    // 此时 paragraph_store.line_count_index 的 strong_count=1，Arc::make_mut 原地改不 COW 深复制。
    // Issue #901 第三十轮问题4: 段落数变化时优先走局部 splice（splice_affected_entries），
    // 只 splice 受影响 entries，前缀/后缀 entries 直接保留。失败时回退到 rebuild_from_text。
    {
        let store_needs_rebuild = paragraph_store.paragraph_count() != new_para_count
            || prev_para_count != new_para_count;

        if store_needs_rebuild {
            // Issue #901 第三十轮问题4: 尝试局部 splice — 只删除/插入受影响的 entries，
            // 前缀/后缀 entries 直接保留（含 Qt layout lease）。失败时回退到全篇重建。
            let spliced = paragraph_store.splice_affected_entries(
                old_splice_start,
                old_splice_end,
                new_layout_start,
                new_layout_end,
                &affected_paras_list,
                font_size,
                font_family,
                line_spacing,
                indent,
                padding,
                width,
            );
            if !spliced {
                // 回退：全篇重建（正确性优先，边界情况不频繁）。
                paragraph_store.rebuild_from_text(
                    text,
                    font_size,
                    font_family,
                    line_spacing,
                    indent,
                    padding,
                    width,
                );
            }
        } else {
            // Issue #901 第五轮阻塞2: 用新段范围更新 store，不用 affected byte range。
            // Issue #901 第三十轮问题4: new_para_texts 只含受影响段文本（从 new_layout_start 起）。
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
    // Issue #901 第三十轮问题4: 从 paras_source 获取前缀段末段 hard_break。
    // Incremental 路径前缀段不在 affected 中，从 store entry 获取（store 已更新）；
    // Full 路径从整章 Vec 获取。
    if new_layout_start > 0 && vl_start_idx > 0 && new_layout_start <= new_para_count {
        let prev_seg_idx = new_layout_start - 1;
        let new_hard_break = paras_source
            .get_para(prev_seg_idx, new_layout_start)
            .map(|(_, hb)| hb)
            .unwrap_or_else(|| {
                // Incremental 路径前缀段不在 affected 中，从 store entry 获取。
                paragraph_store
                    .entry(prev_seg_idx)
                    .map(|e| e.hard_break)
                    .unwrap_or(false)
            });
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
    // Issue #901 第三十轮问题4: 从 paras_source 获取受影响段，不索引整章 new_paras。
    for para_idx in new_layout_start..new_layout_end {
        let (paragraph_text, hard_break) = match paras_source.get_para(para_idx, new_layout_start) {
            Some(para) => para,
            None => {
                // 防御性：get_para 返回 None 时从 store entry 获取文本。
                // 正常路径不应触发，splice_ranges 保证 para_idx 在受影响范围内。
                let entry = match paragraph_store.entry(para_idx) {
                    Some(e) => e,
                    None => break,
                };
                let seg_start = paragraph_start;
                let seg_end = seg_start + entry.paragraph_text_len;
                let text_ref = if seg_start <= seg_end && seg_end <= text.len() {
                    // SAFETY: seg_start/seg_end 来自 store entry 的累计偏移，
                    // 上一轮排版时已验证为 UTF-8 边界。
                    &text[seg_start..seg_end]
                } else {
                    ""
                };
                (text_ref, entry.hard_break)
            }
        };

        // Issue #901 第十四轮问题1: 每段开始时记录段顶 y。
        paragraph_top_y = y;

        relayouted_indices.push(para_idx);

        // Issue #901 第三十轮问题2: 空段不分配 QTextLayout physical_slot。
        // 空段在下方单独处理，不调用 paragraph_physical_slot / prepare_paragraph_visual_snapshot。
        // 非空段的 physical_slot / layout_epoch 在空段分支之后计算。

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
            // Issue #901 第三十轮问题2: 空段不分配物理槽，不调用 prepare_paragraph_visual_snapshot。
            // VisualLine 仍创建（有高度、光标几何），但 cache_slot = -1, layout_epoch = 0。
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
                // Issue #901 第三十轮问题2: 空段不分配物理槽，cache_slot = -1。
                cache_slot: -1,
                // Issue #901 第二十轮阻塞1+3: logical_slot 是当前文档次序。
                logical_slot: paragraph_idx,
                // Issue #901 第十八轮问题4: 增量排版路径从 paragraph_store 获取稳定段落身份。
                paragraph_id: paragraph_store
                    .paragraph_id_at(para_idx)
                    .unwrap_or(ParagraphId::INVALID),
                // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
                // has_qt_layout = false，渲染路径跳过 layout 查询。
                has_qt_layout: false,
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

            // Issue #901 第三十轮问题2: update_layout_result 对空段设置 is_empty_laid_out = true，
            // 不分配物理槽（cache_slot 保持 -1）。
            paragraph_store.update_layout_result(
                para_idx,
                generation,
                -1,
                line_height,
                1,
            );

            paragraph_start += paragraph_text.len() + if hard_break { 1 } else { 0 };
            paragraph_qchar_start += paragraph_text.chars().map(|c| c.len_utf16()).sum::<usize>()
                + if hard_break { 1 } else { 0 };
            paragraph_idx += 1;
            continue;
        }

        // Issue #901 第二十一轮问题1: 获取或分配该段的 Qt 物理槽号。
        // physical_slot 与文档段序（logical_slot）无关，新段分配新槽，
        // 复用段保持已有槽。避免新段覆盖存活段的 QTextLayout。
        // Issue #901 第三十轮问题2: 仅非空段才分配物理槽。
        let physical_slot = paragraph_store.paragraph_physical_slot(para_idx, generation);
        // Issue #901 第三十轮问题1: 获取 epoch，供 VisualLine.layout_epoch 校验。
        // 从 paragraph_store entry 获取（paragraph_physical_slot 已设置 entry.layout_epoch）。
        let layout_epoch = paragraph_store
            .entry(para_idx)
            .map(|e| e.layout_epoch)
            .unwrap_or(0);

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
                // Issue #901 第三十轮问题1: layout_epoch 供光标/命中/渲染校验。
                layout_epoch,
                // Issue #901 第三十一轮问题1: 非空段有 QTextLayout，
                // has_qt_layout = true，走 get_paragraph_layout_checked。
                has_qt_layout: true,
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
    //
    // Issue #901 第三十二轮问题3: 不再遍历后缀 canonical paragraph 更新
    // paragraph_document_byte_start（段落序号）。该字段始终等于数组索引，
    // 唯一读取方（全篇重排路径 line ~2052）已改为直接用数组索引 pidx，
    // 不依赖此字段。后缀段该字段可能过时（仍为旧序号），但无消费方读取。
    // 这消除了 O(suffix_count) 的后缀遍历。

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
    // 段落数变化时用局部 splice（O(k) 数据准备 + O(n) Fenwick 重建），
    // 段落数不变时用 update_paragraph_length（O(k log n)）。
    // Issue #901 第三十轮问题4: 段落数变化时不再遍历整章 new_paras 构造全量
    // byte_lengths/qchar_lengths，改为只构造受影响段数据 + splice。
    let paragraph_offset_index = {
        let mut idx_arc = std::mem::take(&mut prev.paragraph_offset_index);
        let curr_count = paragraphs.len();
        if curr_count != prev_para_count {
            // 段落数变化（Enter 拆段/Backspace 合段），局部 splice 偏移索引。
            // 只构造受影响段的 byte/qchar lengths，前缀/后缀段从旧索引保留。
            // Issue #901 第三十轮问题4: 复用已构造的 affected_paras_list。
            let mut new_byte_lengths = Vec::with_capacity(affected_paras_list.len());
            let mut new_qchar_lengths = Vec::with_capacity(affected_paras_list.len());
            for (para_text, hard_break) in &affected_paras_list {
                new_byte_lengths.push(para_text.len() + if *hard_break { 1 } else { 0 });
                new_qchar_lengths.push(
                    para_text.chars().map(|c| c.len_utf16()).sum::<usize>()
                        + if *hard_break { 1 } else { 0 },
                );
            }
            let idx = Arc::make_mut(&mut idx_arc);
            // splice: 删除旧 [old_splice_start, old_splice_end)，插入新段。
            idx.splice(old_splice_start, old_splice_end, new_byte_lengths, new_qchar_lengths);
            idx_arc
        } else {
            // 段落数不变，COW 增量更新受影响段落的长度（O(k log n)）
            let idx = Arc::make_mut(&mut idx_arc);
            for para_idx in new_layout_start..new_layout_end {
                if let Some((para, hard_break)) = paras_source.get_para(para_idx, new_layout_start) {
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
    // 段落数变化时用局部 splice（O(k) 数据准备 + O(n) BIT 重建，不涉及 QTextLayout），
    // 段落数不变时用 update_heights_incremental 只更新受影响段（O(k log n)）。
    // prev_para_count 已在 take 之前定义（line 683），此处直接复用。
    //
    // Issue #901 第三十轮问题4: 段落数变化时不再构造全量 heights 数组，
    // 改为只构造受影响段高度 + splice。前缀/后缀段高度从旧索引保留。
    let curr_para_count = paragraphs.len();
    // Issue #901 第二十九轮阻塞4: 取走 prev 的 height_index Arc 并显式释放，
    // 使 EditorLayout.height_index 的 strong_count 降为 1，Arc::make_mut 原地改不 COW 深复制。
    // prev.height_index 已在上方 prev_height_index 读取完毕（仅结构变化分支需要），
    // 后续不再直接消费（新 snapshot 从 height_index Arc::clone 获取）。
    let prev_height_index = std::mem::take(&mut prev.height_index);
    if curr_para_count != prev_para_count {
        // Issue #901 第二十五轮复核问题2: prev_height_index 的 clone 已延迟到
        // 结构变化分支内（见下方），普通编辑（无回车，curr_para_count == prev_para_count）
        // 不 clone 整章高度索引 O(段落数)。
        // 仅在拆段/合段（Enter/Backspace 换行符）时才 clone，这类操作不频繁。
        // Issue #901 第二十四轮问题3: 不从 raw visual_lines.para_start 提取高度，
        // 改为从权威段序构造：前缀未改段从 prev height_index + 新排版段从 new_visual_lines
        // + 后缀未改段从 prev height_index。
        // 增量路径保留了复用后缀的旧 para_start/logical_slot，raw 段号可能重复/错乱
        //（如 甲\n乙 → \n甲\n乙 后 raw logical_slot = [0,0,1]），
        // extract_paragraph_heights 用 raw para_start 去重会把不同段的高度误合并。

        // 新排版段高度：从 splice 后的新行按段聚合。
        // splice 后新行在 [vl_start_idx, vl_start_idx + new_affected_line_count) 范围内，
        // 这些行的 para_start 是主循环中设置的正确新段索引（paragraph_idx）。
        let mut new_heights: Vec<f64> = Vec::with_capacity(new_layout_end - new_layout_start);
        {
            use std::collections::HashMap;
            let mut para_heights: HashMap<usize, f64> = HashMap::new();
            let new_lines_range = vl_start_idx..(vl_start_idx + new_affected_line_count);
            for vl in &visual_lines[new_lines_range] {
                *para_heights.entry(vl.para_start).or_insert(0.0) += vl.height;
            }
            for i in new_layout_start..new_layout_end {
                new_heights.push(para_heights.get(&i).copied().unwrap_or(0.0));
            }
        }

        // Issue #901 第二十九轮阻塞4: 释放 prev_height_index Arc，
        // 使 EditorLayout.height_index 的 strong_count=1，Arc::make_mut 不 COW。
        drop(prev_height_index);
        // Issue #901 第三十轮问题4: 局部 splice — 删除旧 [old_splice_start, old_splice_end)，
        // 插入新受影响段高度。前缀/后缀段高度从旧索引保留。
        std::sync::Arc::make_mut(height_index)
            .splice(old_splice_start, old_splice_end, new_heights);
    } else {
        // 段落数不变，只更新受影响段落的高度（O(k log n)）
        // Issue #901 第十六轮问题3: 从新行范围聚合高度，不遍历全章 visual_lines。
        // splice 后新行在 [vl_start_idx, vl_start_idx + new_affected_line_count) 范围内，
        // 这些行的 logical_slot 是新段索引，从中按段聚合高度是 O(k)。
        // Issue #901 第二十九轮阻塞4: 释放 prev_height_index Arc，避免 COW。
        drop(prev_height_index);
        let new_lines_range = vl_start_idx..(vl_start_idx + new_affected_line_count);
        let height_updates =
            extract_affected_height_updates(&visual_lines[new_lines_range], &affected_indices);
        std::sync::Arc::make_mut(height_index).update_heights_incremental(&height_updates, curr_para_count, line_height);
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
        // 此处 Arc::clone 是 O(1) 引用计数增加，保证 snapshot 与 EditorLayout 共享同一份高度数据。
        // Issue #901 第二十九轮阻塞4: Arc 共享 + COW 模式（类似 line_count_index）。
        // 上方已 take/drop prev.height_index Arc，使 Arc::make_mut 原地改不 COW 深复制。
        // 后续编辑时 take/drop 新 prev.height_index 同样使 strong_count=1，不 COW。
        height_index: std::sync::Arc::clone(height_index),
        paragraph_id_to_slot,
        paragraph_line_count_index,
    };

    // Issue #901 第二十五轮复核问题1 + 第二十七轮复核问题3: 增量排版完成后
    // 立即释放已无持有者的旧 QTextLayout。retired slots 在
    // update_paragraph_text/split_paragraph/merge_paragraphs 中收集，
    // 此时新 snapshot 已构建，旧 layout 不再被任何 snapshot 引用，可安全释放。
    // 槽号通过按 generation 隔离的 freelist 复用，QTextLayout 对象内存不再泄漏。
    paragraph_store.release_retired_layouts();

    snapshot
}

/// Issue #901 第三十轮问题4: 段落来源 — 区分增量路径和回退路径。
///
/// - `Incremental`：只含受影响段 + prefix/suffix 计数，不构造整章 new_paras。
///   `new_para_count = prefix_count + affected.len() + suffix_count`。
///   前缀/后缀段由调用方从 prev 的 VisualLine/store entry 复用。
/// - `Full`：整章切分（回退路径，边界情况，不频繁）。
enum ParasSource<'a> {
    Incremental {
        prefix_count: usize,
        suffix_count: usize,
        affected: Vec<(&'a str, bool)>,
    },
    Full(Vec<(&'a str, bool)>),
}

impl<'a> ParasSource<'a> {
    /// 返回总段落数。
    fn para_count(&self) -> usize {
        match self {
            ParasSource::Incremental {
                prefix_count,
                suffix_count,
                affected,
            } => *prefix_count + affected.len() + *suffix_count,
            ParasSource::Full(paras) => paras.len(),
        }
    }

    /// 返回 splice_info：(prefix_count, suffix_count)。
    /// Incremental 路径返回 Some，Full 路径返回 None（走全篇比较）。
    fn splice_info(&self) -> Option<(usize, usize)> {
        match self {
            ParasSource::Incremental {
                prefix_count,
                suffix_count,
                ..
            } => Some((*prefix_count, *suffix_count)),
            ParasSource::Full(_) => None,
        }
    }

    /// 获取受影响段文本切片，用于 `update_affected_paragraph_texts`。
    /// Incremental 路径返回 `affected` 的文本（只含受影响段）；
    /// Full 路径返回 `paras[new_layout_start..new_layout_end]` 的文本。
    fn affected_texts(&self, new_layout_start: usize, new_layout_end: usize) -> Vec<&str> {
        match self {
            ParasSource::Incremental { affected, .. } => {
                affected.iter().map(|(p, _)| *p).collect()
            }
            ParasSource::Full(paras) => {
                let start = new_layout_start.min(paras.len());
                let end = new_layout_end.min(paras.len());
                paras[start..end].iter().map(|(p, _)| *p).collect()
            }
        }
    }

    /// 获取段 `para_idx` 的 `(text, hard_break)`。
    /// Incremental 路径：受影响段从 `affected` 获取（用 local index），
    ///   前缀/后缀段返回 None（调用方应从 store entry 获取）。
    /// Full 路径：直接从整章 Vec 获取。
    fn get_para(&self, para_idx: usize, new_layout_start: usize) -> Option<(&str, bool)> {
        match self {
            ParasSource::Incremental {
                affected, ..
            } => {
                let local_idx = para_idx.checked_sub(new_layout_start)?;
                affected.get(local_idx).copied()
            }
            ParasSource::Full(paras) => paras.get(para_idx).copied(),
        }
    }

    /// 获取整章段落引用（仅 Full 路径有）。
    /// Incremental 路径返回 None（不构造整章 Vec）。
    fn full_paras(&self) -> Option<&Vec<(&str, bool)>> {
        match self {
            ParasSource::Incremental { .. } => None,
            ParasSource::Full(paras) => Some(paras),
        }
    }

    /// 获取受影响段的 `(text, hard_break)` 列表。
    /// Incremental 路径返回 `affected`；Full 路径返回 `paras[new_layout_start..new_layout_end]`。
    fn affected_paras(&self, new_layout_start: usize, new_layout_end: usize) -> Vec<(&str, bool)> {
        match self {
            ParasSource::Incremental { affected, .. } => affected.clone(),
            ParasSource::Full(paras) => {
                let start = new_layout_start.min(paras.len());
                let end = new_layout_end.min(paras.len());
                paras[start..end].to_vec()
            }
        }
    }
}

/// Issue #901 第二十六轮复核缺口4 + 第二十七轮复核问题1+4: 从受影响范围增量拼接
/// new_paras，避免全文切分。
///
/// 当 Core 提供了受影响文本字节范围 `(aff_start, aff_end)` 时，用 paragraph_store
/// entries 中的未改段（受影响范围之前和之后）加上受影响区域的新切分段，拼接构成
/// 完整的 `new_paras`。
///
/// # 第二十七轮复核问题1 修复
///
/// 旧实现把 `aff_start..aff_end`（修改字节范围）当成完整段落范围，只切
/// `&text[aff_start..aff_end]` 作为新段落，会吞掉编辑所在段的未改变前半段和后半段
///（反例：`hello` 替换第二字符 → `hAllo`，旧实现只返回 `[("A", false)]`，丢失
/// `h`/`llo`），且在中文里可能切到 UTF-8 字符中间 panic。
///
/// 新实现把 `aff_start..aff_end` **扩展到新文本中的完整段落边界**
/// `expanded_start..expanded_end`，切出包含编辑所在段的完整段落文本。前缀段和后缀段
/// 从旧 store entries 获取（后缀段修正位置偏移 `delta = text.len() - old_text_len`）。
///
/// # 不依赖 `document_byte_start`（问题3 规避）
///
/// 旧 entries 的 `document_byte_start` 在增量路径中不随前文编辑平移
///（`shift_suffix_document_byte_starts` 未被调用），可能过时。本函数改用旧 entries
/// 的**累计长度**（`paragraph_text_len + hard_break`）定位段在新文本中的位置，
/// 不依赖 `document_byte_start`，天然规避问题3。
///
/// # 问题4 减少全篇扫描
///
/// 前缀段遍历到第一个不满足条件的 entry 即 break；后缀段从 `prefix_count` 开始遍历
/// 到末尾（只读 entry 元数据，不重新切分/排版）。
///
/// 返回 `None` 表示边界检查失败（UTF-8 边界/越界等），调用方应回退到全文切分。
///
/// # 第二十八轮复核问题1+4 修复
///
/// **Bug 1a（尾空段）**：步骤3不再调用 `split_paragraphs_with_hard_break`（它会在
/// 输入以 `\n` 结尾时无条件产生尾空段 `("", false)`）。局部切片 `expanded_start..expanded_end`
/// 以 `\n` 结尾但后面还有正文时，尾空段应由后缀段处理，不应在此产生。改为直接按真实
/// 换行边界切分，只有 `expanded_end == text_len`（全文末尾）时才产生尾空段。
///
/// **Bug 1b（前缀循环条件）**：前缀段条件从 `seg_end_cumulative <= expanded_start` 改为
/// `cumulative < expanded_start && seg_end_cumulative <= expanded_start`。旧条件在
/// `cumulative == expanded_start`（空段）时仍可能成立，导致空段被误当前缀段。
///
/// **Bug 2（性能）**：返回 `(prefix_count, suffix_count, affected_paras)`，调用方可直接
/// 构造 `ParagraphSpliceRanges`，避免构造全篇 `old_paras` 和逐段比较。
///
/// # 第二十九轮复核阻塞4 修复
///
/// **O(1) 旧文长度**：`old_text_len` 改用 `offset_index.total_byte_length()` 获取，
/// 不再遍历全体 entries 累加（原 1539-1542 行 O(n) 循环）。
///
/// **O(log n) 前缀段定位**：用 `offset_index.byte_to_paragraph()` 二分查找定位前缀段
/// 数量，不再从 i=0 线性扫描到第一个不满足条件的 entry。空段边界用 O(空段数) 回退
/// 扫描处理（空段通常只在文档末尾，常数级）。
///
/// **O(log n) 后缀段定位**：用 `offset_index.byte_to_paragraph()` 二分查找定位后缀段
/// 起始索引，后缀段直接从该索引切片构造，不再从 `prefix_count` 逐段扫描到末尾。
///
/// 当 `offset_index` 为空或长度与 store 不一致时，回退到线性扫描（O(n)），保证正确性。
///
/// # 第三十轮问题4 修复 — 消除整章临时数组
///
/// 返回 `(prefix_count, suffix_count, affected_paras)`，**只包含受影响段**。
/// 前缀段和后缀段只计数（`prefix_count`/`suffix_count`），不复制文本切片到 Vec。
/// 调用方（`prepare_incremental_visual_snapshot_impl`）直接从 prev 的 VisualLine/
/// store entry 复用未改段，不拼出另一份整章 `new_paras`。
/// 前缀/后缀段仍做 UTF-8 边界检查（防御性），失败返回 None 回退全文切分。
fn build_new_paras_from_affected_range<'a>(
    text: &'a str,
    store: &super::super::super::paragraph_layout_store::ParagraphLayoutStore,
    offset_index: &super::super::super::paragraph_height_index::ParagraphOffsetIndex,
    aff_start: usize,
    aff_end: usize,
) -> Option<(usize, usize, Vec<(&'a str, bool)>)> {
    let text_len = text.len();
    let n = store.paragraph_count();

    // aff_start/aff_end 必须是 UTF-8 边界，否则后续切片 panic。
    if !text.is_char_boundary(aff_start) || !text.is_char_boundary(aff_end) {
        return None;
    }

    // Issue #901 第二十九轮复核阻塞4: 用 offset_index 的 O(1) total_byte_length
    // 获取旧文总长度，不再遍历全体 entries 累加。
    // offset_index 维护的 byte_lengths[i] = paragraph_text_len[i] + hard_break[i]，
    // 与 store entries 的累计长度一致（两者都是上一轮编辑后的旧状态）。
    // 当 offset_index 为空或长度不匹配时回退到线性扫描。
    let old_text_len = if !offset_index.is_empty() && offset_index.len() == n {
        offset_index.total_byte_length()
    } else {
        let mut len = 0usize;
        for i in 0..n {
            let entry = store.entry(i)?;
            len += entry.paragraph_text_len + if entry.hard_break { 1 } else { 0 };
        }
        len
    };
    // delta = 新文本长度 - 旧文本长度（后缀段在新文本中的位置偏移）。
    let delta: i64 = text_len as i64 - old_text_len as i64;

    // 1. 扩展 aff_start..aff_end 到新文本中的完整段落边界 expanded_start..expanded_end。
    //    expanded_start: 从 aff_start 向前找上一个 '\n' 之后的位置（或文档开头 0）。
    //    expanded_end:   从 aff_end 向后找下一个 '\n' 的位置（含该 '\n'，或文档末尾）。
    //    '\n' 是单字节 ASCII，不切割多字节字符，故 expanded_start/expanded_end 是 UTF-8 边界。
    let expanded_start = if aff_start == 0 {
        0
    } else {
        match text[..aff_start].rfind('\n') {
            Some(pos) => pos + 1,
            None => 0,
        }
    };
    let expanded_end = if aff_end >= text_len {
        text_len
    } else {
        match text[aff_end..].find('\n') {
            Some(pos) => aff_end + pos + 1,
            None => text_len,
        }
    };

    // Issue #901 第三十轮问题4: 只收集受影响段，前缀/后缀段只计数不复制。
    // 前缀/后缀段内容不变，调用方（full.rs）直接从 prev 的 VisualLine/store entry
    // 复用未改段，不拼出另一份整章 new_paras。仍做 UTF-8 边界检查（防御性），
    // 失败返回 None 回退全文切分。
    let mut affected_paras: Vec<(&str, bool)> = Vec::new();

    // Issue #901 第二十九轮复核阻塞4: 当 offset_index 非空且长度匹配 store 时，
    // 用 O(log n) 二分查找定位前缀/后缀段，避免 O(n) 线性扫描。
    let can_use_offset_index = !offset_index.is_empty() && offset_index.len() == n;

    // 2. 前缀段：旧累计到段末（含 '\n'）<= expanded_start。
    //    这些段在 expanded_start 之前，新旧文本位置和内容都不变（编辑发生在之后）。
    //    Issue #901 第三十轮问题4: 只计数不复制。前缀段文本由调用方从 prev 复用。
    //    Issue #901 第二十八轮复核问题1: 条件改为 cumulative < expanded_start（严格小于）
    //    且 seg_end_cumulative <= expanded_start。旧条件 seg_end_cumulative <= expanded_start
    //    在 cumulative == expanded_start（空段，paragraph_text_len==0 && !hard_break）时
    //    仍成立，导致空段被误当前缀段（反例2：旧文 a\n，末尾空段输入 X）。
    //    Issue #901 第二十九轮复核阻塞4: 用 offset_index.byte_to_paragraph() O(log n)
    //    二分查找定位前缀段数量，不再从 i=0 线性扫描。
    let mut prefix_count = 0usize;
    if can_use_offset_index {
        // byte_to_paragraph(expanded_start) 返回最大的 k 使得
        // paragraph_byte_start(k) <= expanded_start。段 k 的 seg_end =
        // paragraph_byte_start(k+1) > expanded_start，所以段 k 不是前缀段。
        // 前缀段最多到段 k-1。
        let k = offset_index.byte_to_paragraph(expanded_start);
        // 从 k 往前排除空段：paragraph_byte_start(i) == expanded_start 的段
        // 不满足严格小于条件（cumulative < expanded_start）。
        let mut pc = k;
        while pc > 0 && offset_index.paragraph_byte_start(pc - 1) >= expanded_start {
            pc -= 1;
        }
        // UTF-8 边界检查（防御性），不切片 push。
        for i in 0..pc {
            let entry = store.entry(i)?;
            let seg_start = offset_index.paragraph_byte_start(i);
            let seg_end = seg_start + entry.paragraph_text_len;
            if seg_end > text_len
                || !text.is_char_boundary(seg_start)
                || !text.is_char_boundary(seg_end)
            {
                return None; // 越界或 UTF-8 边界检查失败，回退全文切分
            }
        }
        prefix_count = pc;
    } else {
        // 回退：线性扫描前缀段（只计数，不切片）。
        let mut cumulative = 0usize;
        for i in 0..n {
            let entry = store.entry(i)?;
            let seg_len_with_break =
                entry.paragraph_text_len + if entry.hard_break { 1 } else { 0 };
            let seg_end_cumulative = cumulative + seg_len_with_break;
            if cumulative < expanded_start && seg_end_cumulative <= expanded_start {
                let seg_start = cumulative;
                let seg_end = cumulative + entry.paragraph_text_len;
                if seg_end > text_len
                    || !text.is_char_boundary(seg_start)
                    || !text.is_char_boundary(seg_end)
                {
                    return None;
                }
                prefix_count += 1;
            } else {
                break;
            }
            cumulative = seg_end_cumulative;
        }
    }

    // 3. 受影响段：从新文本 expanded_start..expanded_end 按真实换行边界切分。
    //    expanded_start/expanded_end 是 '\n' 边界或文档边界，是有效 UTF-8 边界。
    //    Issue #901 第二十八轮复核问题1: 不使用 split_paragraphs_with_hard_break，
    //    因为它会在输入以 \n 结尾时无条件产生尾空段 ("", false)。局部切片以 \n 结尾
    //    但后面还有正文时，尾空段应由后缀段处理，不应在此产生。
    //    反例1：旧文 a\nb，第一段插入 X → 新文 aX\nb，切片 aX\n 不应产生尾空段。
    //    只有 expanded_end == text_len（全文末尾）时才产生尾空段。
    let mut affected_paras: Vec<(&str, bool)> = Vec::new();
    let mut seg_start = expanded_start;
    let affected_bytes = &text.as_bytes()[expanded_start..expanded_end];
    for (i, &b) in affected_bytes.iter().enumerate() {
        if b == b'\n' {
            let seg_end = expanded_start + i;
            // SAFETY: '\n' 是单字节 ASCII，seg_start..seg_end 是有效 UTF-8 边界
            //（不会切割多字节字符）。seg_start 是 expanded_start 或某个 \n+1，都是边界。
            affected_paras.push((&text[seg_start..seg_end], true));
            seg_start = seg_end + 1;
        }
    }
    // 最后一段处理
    if seg_start < expanded_end {
        // 有剩余内容（不以 \n 结尾）
        // SAFETY: seg_start 是 expanded_start 或某个 \n+1，expanded_end 是 UTF-8 边界。
        affected_paras.push((&text[seg_start..expanded_end], false));
    } else if expanded_end == text_len {
        // 以 \n 结尾且是全文末尾 → 产生尾空段
        // SAFETY: seg_start == expanded_end == text_len, 空字符串切片。
        affected_paras.push((&text[seg_start..expanded_end], false));
    }
    // 如果 seg_start == expanded_end 且 expanded_end < text_len，
    // 不产生尾空段（后面还有正文，尾空段由后缀段处理）。
    // Issue #901 第三十轮问题4: affected_paras 只含受影响段，不 extend 到 new_paras。

    // 4. 后缀段：新位置 = 旧累计 + delta >= expanded_end。
    //    这些段内容不变，但在新文本中位置偏移了 delta。从 prefix_count 开始遍历，
    //    跳过受影响段（不满足条件的），收集后缀段。只读 entry 元数据，不重新切分。
    //    Issue #901 第三十轮问题4: 只计数 suffix_count，不切片 push。后缀段文本由
    //    调用方从 prev 复用。仍做 UTF-8 边界检查（防御性），失败返回 None。
    //    Issue #901 第二十八轮复核问题4: 同时计数 suffix_count，供调用方直接构造
    //    ParagraphSpliceRanges，避免全篇 old_paras 构造和逐段比较。
    //    Issue #901 第二十八轮复核问题1: 后缀段判断条件从 new_seg_start >= expanded_end
    //    改为 new_seg_end_with_break > expanded_end（段末含 \n 严格超过 expanded_end）。
    //    旧条件在空段（seg_len_with_break=0）时，new_seg_start == expanded_end 会误判
    //    受影响段的尾空段为后缀段（反例2：旧文 a\n，新文 a\nX，空段新位置=3=expanded_end）。
    //    新条件确保空段只有在新位置 > expanded_end 时才被收集。
    //    Issue #901 第二十九轮复核阻塞4: 用 offset_index.byte_to_paragraph() O(log n)
    //    二分查找定位后缀段起始索引，从该索引开始遍历，不再从 prefix_count 逐段扫描
    //    到末尾。后缀段起始 = 第一个满足 paragraph_byte_start(i+1) + delta > expanded_end
    //    的段索引。
    let mut suffix_count = 0usize;
    if can_use_offset_index {
        // 后缀段条件：paragraph_byte_start(i+1) + delta > expanded_end
        //            && paragraph_byte_start(i) + delta >= 0
        // 即 paragraph_byte_start(i+1) > expanded_end - delta
        let threshold_i64 = expanded_end as i64 - delta;
        let suffix_start = if threshold_i64 <= 0 {
            // threshold <= 0，所有段都满足 paragraph_byte_start(i+1) > threshold
            prefix_count
        } else {
            let threshold = threshold_i64 as usize;
            // byte_to_paragraph(threshold) 返回最大的 j 使得
            // paragraph_byte_start(j) <= threshold。段 j 满足
            // paragraph_byte_start(j+1) > threshold，是第一个后缀段候选。
            offset_index.byte_to_paragraph(threshold).max(prefix_count)
        };
        // 从 suffix_start 开始遍历后缀段（只计数，不切片 push）。
        // 用 offset_index.paragraph_byte_start(i) O(log n) 获取累计长度，
        // 不需要从 0 重新累加到 suffix_start。
        for i in suffix_start..n {
            let entry = store.entry(i)?;
            let cumulative = offset_index.paragraph_byte_start(i);
            let seg_len_with_break =
                entry.paragraph_text_len + if entry.hard_break { 1 } else { 0 };
            let new_seg_start_i64 = cumulative as i64 + delta;
            let new_seg_end_with_break_i64 = new_seg_start_i64 + seg_len_with_break as i64;
            if new_seg_end_with_break_i64 > expanded_end as i64 && new_seg_start_i64 >= 0 {
                let new_seg_start = new_seg_start_i64 as usize;
                let new_seg_end = new_seg_start + entry.paragraph_text_len;
                if new_seg_end > text_len
                    || !text.is_char_boundary(new_seg_start)
                    || !text.is_char_boundary(new_seg_end)
                {
                    return None; // 越界或 UTF-8 边界检查失败，回退全文切分
                }
                suffix_count += 1;
            }
            // 不满足条件的段（空段在 expanded_end 边界）跳过，与原实现行为一致。
        }
    } else {
        // 回退：线性扫描后缀段（只计数，不切片 push）。
        let mut cumulative = 0usize;
        for i in 0..prefix_count {
            let entry = store.entry(i)?;
            cumulative += entry.paragraph_text_len + if entry.hard_break { 1 } else { 0 };
        }
        for i in prefix_count..n {
            let entry = store.entry(i)?;
            let seg_len_with_break =
                entry.paragraph_text_len + if entry.hard_break { 1 } else { 0 };
            let new_seg_start_i64 = cumulative as i64 + delta;
            let new_seg_end_with_break_i64 = new_seg_start_i64 + seg_len_with_break as i64;
            if new_seg_end_with_break_i64 > expanded_end as i64 && new_seg_start_i64 >= 0 {
                let new_seg_start = new_seg_start_i64 as usize;
                let new_seg_end = new_seg_start + entry.paragraph_text_len;
                if new_seg_end > text_len
                    || !text.is_char_boundary(new_seg_start)
                    || !text.is_char_boundary(new_seg_end)
                {
                    return None; // 越界或 UTF-8 边界检查失败，回退全文切分
                }
                suffix_count += 1;
            }
            cumulative += seg_len_with_break;
        }
    }

    Some((prefix_count, suffix_count, affected_paras))
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
                    // Issue #901 第三十二轮问题3: paragraph_document_byte_start 始终等于
                    // 数组索引，直接用 pidx 替代字段读取，消除增量路径中后缀遍历更新该字段的需要。
                    let prev_para_slot = pidx;
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
                    // Issue #901 第三十轮问题2: 空段不分配物理槽，不调用 prepare_paragraph_visual_snapshot。
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
                        // Issue #901 第三十轮问题2: 空段不分配物理槽。
                        cache_slot: -1,
                        logical_slot: current_para_idx as i32,
                        // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
                        paragraph_id: ParagraphId::INVALID,
                // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
                // has_qt_layout = false，渲染路径跳过 layout 查询。
                has_qt_layout: false,
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
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 非空段有 QTextLayout，
                // has_qt_layout = true，走 get_paragraph_layout_checked。
                has_qt_layout: true,
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
            // Issue #901 第三十轮问题2: 空段不分配物理槽，不调用 prepare_paragraph_visual_snapshot。
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
                // Issue #901 第三十轮问题2: 空段不分配物理槽。
                cache_slot: -1,
                logical_slot: current_para_idx as i32,
                // Issue #901 第十八轮问题4: 此路径无 paragraph_store，用 INVALID。
                paragraph_id: ParagraphId::INVALID,
                // Issue #901 第三十轮问题2: 空段无 layout，layout_epoch = 0。
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 空段无 QTextLayout，
                // has_qt_layout = false，渲染路径跳过 layout 查询。
                has_qt_layout: false,
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
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 非空段有 QTextLayout，
                // has_qt_layout = true，走 get_paragraph_layout_checked。
                has_qt_layout: true,
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
                layout_epoch: 0,
                // Issue #901 第三十一轮问题1: 尾部换行新空段无 QTextLayout，
                // has_qt_layout = false，渲染路径跳过 layout 查询。
                has_qt_layout: false,
        });
    }

    // Issue #901 第二十九轮阻塞4: Arc 包装高度索引，O(1) 共享前缀。
    let height_index = std::sync::Arc::new(build_height_index_from_lines(&visual_lines));
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
