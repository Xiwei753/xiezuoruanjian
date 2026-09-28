//! 排版结果到 snapshot 的实际组装。
//!
//! 从 `prepare.rs` 拆出：`prepare_document_visual_snapshot_impl` 加上「从已有
//! VisualLine 组装」「只组装受影响段落」两个入口共 837 行，是整条搬运逻辑；
//! `prepare.rs` 只保留对外入口与动画视觉资源提取。

use super::*;

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
