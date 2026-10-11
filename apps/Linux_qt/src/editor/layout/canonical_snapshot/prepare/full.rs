//! 排版结果到 snapshot 的实际组装。
//!
//! 从 `prepare.rs` 拆出：`prepare_document_visual_snapshot_impl` 加上「从已有
//! VisualLine 组装」「只组装受影响段落」两个入口共 837 行，是整条搬运逻辑；
//! `prepare.rs` 只保留对外入口与动画视觉资源提取。

use super::*;
// Issue #901 第五轮阻塞2: 简化 paragraph_layout_store 全路径引用。
use super::super::super::paragraph_layout_store::{ParagraphLayoutStore, ParagraphSpliceRanges};

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
            // Issue #785 评论 5857873894 修改 2a: 填充稳定行身份。
            // 这里从 VisualLine 组装 canonical line，行身份直接来自 VisualLine。
            paragraph_document_byte_start: line.para_start,
            qtextline_idx: line.qtextline_idx,
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

// ── Issue #901: 增量排版实现 ──

/// 从 `CanonicalDocumentVisualSnapshot` 的 `visual_lines` 提取每段高度。
///
/// 按 `cache_slot` 分组累加行高，返回按段落顺序排列的高度数组。
///
/// Issue #901: 用 HashMap 索引代替 Vec `contains/position` 线性查找，
/// 避免 O(行数 × 段数) 最坏复杂度。
fn extract_paragraph_heights(snapshot: &CanonicalDocumentVisualSnapshot) -> Vec<f64> {
    use std::collections::HashMap;

    // 用 HashMap 按 para_start 去重并记录段落顺序索引
    let mut para_start_to_idx: HashMap<usize, usize> = HashMap::new();
    let mut para_starts: Vec<usize> = Vec::new();
    for vl in &snapshot.visual_lines {
        if !para_start_to_idx.contains_key(&vl.para_start) {
            para_start_to_idx.insert(vl.para_start, para_starts.len());
            para_starts.push(vl.para_start);
        }
    }

    let mut heights = vec![0.0f64; para_starts.len()];
    // 用 HashMap 直接索引，O(1) 查找
    for vl in &snapshot.visual_lines {
        if let Some(&idx) = para_start_to_idx.get(&vl.para_start) {
            heights[idx] += vl.height;
        }
    }
    heights
}

/// Issue #901: 从 snapshot 提取受影响段落的高度更新列表。
///
/// 只对 `affected_indices` 中列出的段落提取 `(段落索引, 新高度)`，
/// 用于 `ParagraphHeightIndex::update_heights_incremental`。
///
/// Issue #901 第二轮复核问题 3: 一次构建 para_start → 段落顺序索引 和
/// para_start → 累计高度 的 HashMap，不在循环内每次重建全部 para_starts + Vec::contains。
fn extract_affected_height_updates(
    snapshot: &CanonicalDocumentVisualSnapshot,
    affected_indices: &[usize],
) -> Vec<(usize, f64)> {
    use std::collections::HashMap;

    if affected_indices.is_empty() {
        return Vec::new();
    }

    // 一次遍历 visual_lines，构建：
    // - para_start → 段落顺序索引（按首次出现顺序编号）
    // - para_start → 累计高度
    let mut para_start_to_order: HashMap<usize, usize> = HashMap::new();
    let mut para_start_to_height: HashMap<usize, f64> = HashMap::new();
    for vl in &snapshot.visual_lines {
        // 先取当前长度再插入，避免 entry 借用与 len 借用冲突
        let order = para_start_to_order.len();
        para_start_to_order.entry(vl.para_start).or_insert(order);
        *para_start_to_height.entry(vl.para_start).or_insert(0.0) += vl.height;
    }

    // 反转 order → para_start，用于按段落顺序索引查找
    let mut order_to_para_start: HashMap<usize, usize> = HashMap::new();
    for (&para_start, &order) in &para_start_to_order {
        order_to_para_start.insert(order, para_start);
    }

    // 对每个受影响段落，O(1) 查表获取高度
    let mut updates: Vec<(usize, f64)> = Vec::new();
    for &idx in affected_indices {
        if let Some(&para_start) = order_to_para_start.get(&idx) {
            if let Some(&height) = para_start_to_height.get(&para_start) {
                updates.push((idx, height));
            }
        }
    }
    updates
}

/// Issue #901 第三轮复核问题 4: 检查 previous_snapshot 的段落文本是否与 new text 完全一致。
///
/// 用于区分"真正无正文变化"和"换行结构变化/空行插删"。
/// 从 prev snapshot 的 `paragraphs` 重建文本，与 `text` 比较。
/// O(n) 但只在 `no_affected_range` 边界情况调用，不影响常规编辑性能。
fn prev_snapshot_text_matches(prev: &CanonicalDocumentVisualSnapshot, text: &str) -> bool {
    let mut reconstructed = String::with_capacity(text.len());
    for para in &prev.paragraphs {
        reconstructed.push_str(&para.paragraph_text);
        reconstructed.push('\n');
    }
    // 如果 text 不以 '\n' 结尾，reconstructed 会多一个尾部 '\n'，去掉。
    if !text.ends_with('\n') && reconstructed.ends_with('\n') {
        reconstructed.pop();
    }
    reconstructed == text
}

/// Issue #901 第三轮复核问题 2: 从 previous_snapshot 的 visual_lines 构建
/// `para_start → (line_start_idx, line_end_idx)` 索引，O(all_lines) 一次构建。
///
/// 用于增量排版路径中未变段落的 O(1) 行集合查找，
/// 代替 `prev.visual_lines.iter().filter(...)` 的 O(all_lines) 每段扫描。
fn build_para_start_to_line_range(
    visual_lines: &[VisualLine],
) -> std::collections::HashMap<usize, (usize, usize)> {
    use std::collections::HashMap;
    let mut map: HashMap<usize, (usize, usize)> = HashMap::new();
    if visual_lines.is_empty() {
        return map;
    }
    let mut range_start = 0usize;
    let mut current_para_start = visual_lines[0].para_start;
    for (i, vl) in visual_lines.iter().enumerate().skip(1) {
        if vl.para_start != current_para_start {
            map.insert(current_para_start, (range_start, i));
            range_start = i;
            current_para_start = vl.para_start;
        }
    }
    // 最后一个段落的范围
    map.insert(current_para_start, (range_start, visual_lines.len()));
    map
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
                visual_lines: prev.visual_lines.clone(),
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
        );

        // 从 snapshot 提取段落高度，重建 height_index
        let heights = extract_paragraph_heights(&snapshot);
        height_index.rebuild(heights);

        // 同步 paragraph_store 的排版结果
        paragraph_store.sync_from_visual_lines(&snapshot.visual_lines, generation);

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
                visual_lines: Vec::new(),
            };
        }
    };

    // 用 store 计算受影响段落范围（用于 full_relayout 判断）
    let affected = paragraph_store.compute_affected_paragraphs(affected_byte_start, affected_byte_end);

    // Issue #901 问题5: 段落身份用 paragraph_store 的 ParagraphId 匹配，
    // 不按 prev.paragraphs[current_idx] 下标匹配。
    let prev_para_count = prev.paragraphs.len();

    let new_paras: Vec<&str> = text.split_inclusive('\n').collect();
    let new_para_count = new_paras.len();

    // Issue #901 第五轮阻塞2: 用段落文本比较计算旧段 splice 范围和新段建立范围。
    // Enter 拆段（旧1段→新2段）和 Backspace 合段（旧2段→新1段）时，
    // 旧删段数不等于新添段数，不能用同一 affected_start..affected_end
    // 同时 splice 旧段数组和新段数组。分别用两套独立范围：
    // - old_splice_start..old_splice_end：在旧段数组中删除的范围
    // - new_layout_start..new_layout_end：在新段数组中建立（重排版）的范围
    let new_para_texts: Vec<&str> = new_paras.iter().map(|p| p.trim_end_matches('\n')).collect();
    let old_para_texts: Vec<&str> = prev.paragraphs.iter().map(|p| p.paragraph_text.as_str()).collect();

    let splice_ranges = if affected.full_relayout {
        ParagraphSpliceRanges {
            old_splice_start: 0,
            old_splice_end: prev_para_count,
            new_layout_start: 0,
            new_layout_end: new_para_count,
        }
    } else {
        ParagraphLayoutStore::compute_old_new_paragraph_ranges(
            &old_para_texts,
            &new_para_texts,
        )
    };

    // 增量更新 store 的文本信息
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
                splice_ranges.new_layout_start,
                splice_ranges.new_layout_end,
                font_size,
                font_family,
                line_spacing,
                indent,
                padding,
                width,
            );
        }
    }

    // 构建 para_start → (line_start, line_end) 索引
    let prev_line_index = build_para_start_to_line_range(&prev.visual_lines);

    // Issue #901 问题5: take prev 的 visual_lines 和 paragraphs（move，不 clone）。
    // 后续只 splice 受影响段落的行和段落，未修改段直接复用 move 出的数据。
    let mut visual_lines: Vec<VisualLine> = std::mem::take(&mut prev.visual_lines);
    let mut prev_paragraphs: Vec<CanonicalParagraphSnapshot> = std::mem::take(&mut prev.paragraphs);

    // Issue #901 第五轮阻塞2: 用旧段范围找旧视觉行范围，用新段范围建立新段。
    let old_splice_start = splice_ranges.old_splice_start;
    let old_splice_end = splice_ranges.old_splice_end;
    let new_layout_start = splice_ranges.new_layout_start;
    let new_layout_end = splice_ranges.new_layout_end;

    // 找到旧段 splice 范围在 visual_lines 中的行范围（用旧段落 byte_start 匹配）
    let mut vl_start_idx: usize = 0;
    let mut vl_end_idx: usize = visual_lines.len();
    if old_splice_start < prev_paragraphs.len() {
        let para_start = prev_paragraphs[old_splice_start].paragraph_document_byte_start;
        if let Some(&(s, _)) = prev_line_index.get(&para_start) {
            vl_start_idx = s;
        }
    }
    if old_splice_end < prev_paragraphs.len() {
        let para_start = prev_paragraphs[old_splice_end].paragraph_document_byte_start;
        if let Some(&(s, _)) = prev_line_index.get(&para_start) {
            vl_end_idx = s;
        }
    }

    // 计算受影响段落的旧高度和旧 byte 范围
    let old_affected_height: f64 = visual_lines[vl_start_idx..vl_end_idx]
        .iter()
        .map(|vl| vl.height)
        .sum();
    let old_affected_byte_start: usize = visual_lines.get(vl_start_idx)
        .map(|vl| vl.byte_start)
        .unwrap_or(0);

    // 重新排版受影响段落
    let mut new_visual_lines: Vec<VisualLine> = Vec::new();
    let mut new_paragraphs: Vec<CanonicalParagraphSnapshot> = Vec::new();
    let mut relayouted_indices: Vec<usize> = Vec::new();

    // 计算受影响段落的起始 y 和起始 byte offset
    let y_start: f64 = if vl_start_idx > 0 {
        visual_lines[vl_start_idx - 1].y + visual_lines[vl_start_idx - 1].height
    } else {
        padding
    };

    // 计算受影响段落的起始 paragraph_start 和 paragraph_qchar_start
    // Issue #901 第五轮阻塞2: 用新段范围 new_layout_start 计算起始偏移
    let mut paragraph_start: usize = 0;
    let mut paragraph_qchar_start: usize = 0;
    for para in &new_paras[..new_layout_start.min(new_para_count)] {
        paragraph_start += para.len();
        paragraph_qchar_start += para.chars().map(|c| c.len_utf16()).sum::<usize>();
    }

    let mut y: f64 = y_start;
    let mut line_id: usize = vl_start_idx;
    let mut paragraph_idx: i32 = new_layout_start as i32;

    // Issue #901 第五轮阻塞2: 用新段范围 new_layout_start..new_layout_end 建立新段
    for para_idx in new_layout_start..new_layout_end {
        let paragraph = new_paras[para_idx];
        let hard_break = paragraph.ends_with('\n');
        let paragraph_text = paragraph.trim_end_matches('\n');

        relayouted_indices.push(para_idx);

        if paragraph_text.is_empty() {
            let empty_ascent = get_font_ascent(font_family, font_size as f32);
            let empty_descent = get_font_descent(font_family, font_size as f32);
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
                false,
            );
            new_visual_lines.push(VisualLine {
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

            new_paragraphs.push(CanonicalParagraphSnapshot {
                paragraph_text: String::new(),
                paragraph_document_byte_start: paragraph_start,
                lines: Vec::new(),
                index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                    "",
                    paragraph_start,
                ),
            });

            paragraph_store.update_layout_result(
                para_idx,
                generation,
                paragraph_idx,
                line_height,
                1,
                line_id - 1,
            );

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
                line_wrap_width: if is_first { available - indent } else { available },
                line_indent_x: if is_first { indent } else { 0.0 },
                para_indent: indent,
                x_end_trailing: canonical_line.x_end_trailing,
                qt_ascent: canonical_line.ascent,
                qt_descent: canonical_line.descent,
                cache_slot: paragraph_idx,
            });
            line_id += 1;
            y += actual_line_h;
            para_height += actual_line_h;
        }

        paragraph_store.update_layout_result(
            para_idx,
            generation,
            paragraph_idx,
            para_height,
            line_count,
            line_start_for_store,
        );

        paragraph_start += paragraph.len();
        paragraph_qchar_start += paragraph.chars().map(|c| c.len_utf16()).sum::<usize>();

        new_paragraphs.push(canonical);
        paragraph_idx += 1;
    }

    // 计算新受影响段落的高度
    let new_affected_height: f64 = new_visual_lines.iter().map(|vl| vl.height).sum();
    let delta_height = new_affected_height - old_affected_height;

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

    // Issue #901 问题5: 更新后续段的 y 和 byte offset。
    // 只在高度变化或 byte offset 变化时更新，避免不必要的遍历。
    // 对长章非结构性编辑（高度不变），此处跳过，成本 O(1)。
    if delta_height.abs() > 0.001 {
        let update_start = vl_start_idx + new_affected_line_count;
        for vl in &mut visual_lines[update_start..] {
            vl.y += delta_height;
        }
    }

    // 更新后续段的 byte offset 和 line_id
    // byte offset 变化 = 新文本中受影响段落后的 byte offset - 旧文本中对应的 byte offset
    // 用新文本的 paragraph_start（已累加到受影响段落后）和旧文本的 old_affected_byte_start 比较
    let new_affected_byte_end = paragraph_start;
    // 旧受影响段落的 byte end = vl_end_idx 对应行的 byte_end
    let old_affected_byte_end: usize = if vl_end_idx > 0 && vl_end_idx <= visual_lines.len() {
        // vl_end_idx 可能已经因为 splice 变化，用 splice 后的位置
        let after_splice_end = vl_start_idx + new_affected_line_count;
        if after_splice_end < visual_lines.len() {
            visual_lines[after_splice_end].byte_start
        } else {
            new_affected_byte_end
        }
    } else {
        new_affected_byte_end
    };
    let byte_delta = new_affected_byte_end as i64 - old_affected_byte_end as i64;

    if byte_delta != 0 {
        let update_start = vl_start_idx + new_affected_line_count;
        for vl in &mut visual_lines[update_start..] {
            vl.para_start = (vl.para_start as i64 + byte_delta) as usize;
            vl.byte_start = (vl.byte_start as i64 + byte_delta) as usize;
            vl.byte_end = (vl.byte_end as i64 + byte_delta) as usize;
        }
        // 更新后续段落的 paragraph_document_byte_start
        // Issue #901 第五轮阻塞2: 用新段范围 new_layout_end 定位后续段
        let para_update_start = new_layout_end.min(paragraphs.len());
        for para in &mut paragraphs[para_update_start..] {
            para.paragraph_document_byte_start =
                (para.paragraph_document_byte_start as i64 + byte_delta) as usize;
            // 重建 index_map
            para.index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                &para.paragraph_text,
                para.paragraph_document_byte_start,
            );
            for line in &mut para.lines {
                line.document_byte_start =
                    (line.document_byte_start as i64 + byte_delta) as usize;
                line.document_byte_end =
                    (line.document_byte_end as i64 + byte_delta) as usize;
                line.paragraph_document_byte_start =
                    (line.paragraph_document_byte_start as i64 + byte_delta) as usize;
                for cluster in &mut line.clusters {
                    cluster.document_byte_start =
                        (cluster.document_byte_start as i64 + byte_delta) as usize;
                    cluster.document_byte_end =
                        (cluster.document_byte_end as i64 + byte_delta) as usize;
                }
            }
        }
    }

    // 更新后续段的 line_id（确保连续）
    let mut next_id = vl_start_idx + new_affected_line_count;
    for vl in &mut visual_lines[vl_start_idx + new_affected_line_count..] {
        vl.id = next_id;
        next_id += 1;
    }

    // 更新后续段的 qchar_start/qchar_end
    // qchar offset 变化 = 新文本中受影响段落后的 qchar offset - 旧文本中对应的 qchar offset
    let new_affected_qchar_end = paragraph_qchar_start;
    let old_affected_qchar_end: usize = if vl_start_idx + new_affected_line_count < visual_lines.len() {
        let after = vl_start_idx + new_affected_line_count;
        visual_lines[after].qchar_start - visual_lines[after].para_qchar_start
    } else {
        new_affected_qchar_end
    };
    let qchar_delta = new_affected_qchar_end as i64 - old_affected_qchar_end as i64;
    if qchar_delta != 0 {
        for vl in &mut visual_lines[vl_start_idx + new_affected_line_count..] {
            vl.qchar_start = (vl.qchar_start as i64 + qchar_delta) as usize;
            vl.qchar_end = (vl.qchar_end as i64 + qchar_delta) as usize;
        }
    }
    // Issue #901 问题5: 增量路径只遍历了受影响段落，line_id/y/paragraph_idx
    // 是受影响段落结束后的值。空文本/尾部换行处理需要文档末尾的值。
    // 更新到 splice 后的文档末尾。
    line_id = visual_lines.len();
    y = visual_lines.last().map(|vl| vl.y + vl.height).unwrap_or(padding);
    paragraph_idx = paragraphs.len() as i32;

    // 空文本处理
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
            false,
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

    // 尾部换行处理
    // Issue #901 第五轮阻塞2: 增量路径已搬运 prev 的尾部空行（如果 prev 也有尾部 \n），
    // 修改前面段落时可能重复插入末尾空视觉行。只对**确实需要新增**的末尾空视觉行
    // 创建节点，不允许沿用的 prev 哨兵再复制。
    // 检查 splice 后的最后一个视觉行是否已经是末尾空行（para_text 为空且
    // byte_start == text.len()），如果是则不再追加。
    if text.ends_with('\n') {
        let text_qchar_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        let already_has_trailing_empty = visual_lines.last().map_or(false, |vl| {
            vl.para_text.is_empty() && vl.byte_start == text.len()
        });
        if !already_has_trailing_empty {
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
                false,
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
        }
    }

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
        visual_lines,
    };

    // Issue #901: 增量更新 height_index 和 paragraph_store，不扫描全章。
    // 只对受影响段落调用 update_height（O(k log n)），不 rebuild（O(n)）。
    // 只对受影响段落同步排版结果，不 sync_from_visual_lines（O(n)）。

    // Issue #901 第三轮复核问题 4: 用实际重排的段落索引驱动后续更新。
    // Issue #901 第五轮阻塞2: 用 new_layout 范围替代 affected byte range 范围，
    // 因为 new_layout 范围精确反映了新段中需要重排的段落。
    let affected_indices: Vec<usize> = if affected.full_relayout {
        (0..snapshot.paragraphs.len()).collect()
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
    // 段落数变化时用 rebuild 从 snapshot 提取全部高度（O(n) 但不涉及 QTextLayout），
    // 段落数不变时用 update_heights_incremental 只更新受影响段（O(k log n)）。
    // prev_para_count 已在 take 之前定义（line 683），此处直接复用。
    let curr_para_count = snapshot.paragraphs.len();
    if curr_para_count != prev_para_count {
        // 段落数变化（Enter 拆段/Backspace 合段），重建高度索引
        let heights = extract_paragraph_heights(&snapshot);
        height_index.rebuild(heights);
    } else {
        // 段落数不变，只更新受影响段落的高度（O(k log n)）
        let height_updates = extract_affected_height_updates(&snapshot, &affected_indices);
        height_index.update_heights_incremental(&height_updates, curr_para_count, line_height);
    }

    // 增量同步 paragraph_store：只更新受影响段落的排版结果
    paragraph_store.sync_from_visual_lines_incremental(
        &snapshot.visual_lines,
        generation,
        &affected_indices,
    );

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
