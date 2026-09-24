use super::engine::{
    byte_offset_to_qchar_offset, cursor_rect_for_line, get_font_ascent, get_font_descent,
    qchar_offset_to_byte_offset, text_baseline_y,
};
use super::types::{CaretAffinity, CaretRect, LayoutSnapshot, VisualLine};

// ── Qt 文本布局模块：hit test / caret / cursor 定位 ──
//
// 全部只消费 qt_cache 当前 generation 的 QTextLine，不重新排版。

pub fn hit_test(
    snapshot: &LayoutSnapshot,
    x: f64,
    y: f64,
    scroll_y: f64,
) -> (usize, CaretAffinity) {
    let lines = &snapshot.lines;
    if lines.is_empty() {
        return (0, CaretAffinity::Downstream);
    }

    let doc_y = y + scroll_y;
    let line_opt = lines
        .iter()
        .enumerate()
        .find(|(_, line)| doc_y < line.y + line.height);
    let (_line_idx, line) = match line_opt {
        Some((idx, l)) => (idx, l),
        None => {
            let Some(l) = lines.last() else {
                return (0, CaretAffinity::Downstream);
            };
            (lines.len() - 1, l)
        }
    };
    let raw_index = index_at_line_x(snapshot, line, x);
    let index = raw_index.max(line.byte_start).min(line.byte_end);
    debug_assert!(
        index >= line.byte_start && index <= line.byte_end,
        "hit_test: index {} out of line range {}..{}",
        index,
        line.byte_start,
        line.byte_end
    );
    let affinity = affinity_for_index_on_line(line, index);

    #[cfg(debug_assertions)]
    {
        let rect = caret_rect(snapshot, index, affinity, scroll_y, f64::INFINITY);
        let rect_y_doc = rect.y + scroll_y;
        let line_top = line.y;
        let line_bottom = line.y + line.height;
        let diff = if rect_y_doc < line_top {
            line_top - rect_y_doc
        } else if rect_y_doc > line_bottom {
            rect_y_doc - line_bottom
        } else {
            0.0
        };
        debug_assert!(
            diff < 5.0,
            "hit_test debug assert failed: rect_y_doc={:.2} is not within hit line range {:.2}..{:.2} (diff={:.2})",
            rect_y_doc,
            line_top,
            line_bottom,
            diff
        );
    }

    (index, affinity)
}

pub fn caret_rect(
    snapshot: &LayoutSnapshot,
    cursor_byte: usize,
    affinity: CaretAffinity,
    scroll_y: f64,
    viewport_h: f64,
) -> CaretRect {
    let line = snapshot
        .lines
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            line_contains_cursor_with_affinity(&snapshot.lines, *idx, cursor_byte, affinity)
        })
        .map(|(_, line)| line)
        .or_else(|| snapshot.lines.last());

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
                height: f64::from(snapshot.font_size) * f64::from(snapshot.line_spacing),
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

    let cursor_x = calculate_cursor_x_for_line(line, cursor_byte, affinity, snapshot);
    let (cursor_y_doc, cursor_h) =
        cursor_rect_for_line(line, f64::from(snapshot.font_size), &snapshot.font_family);
    let cursor_y = cursor_y_doc - scroll_y;
    let visible = cursor_y + cursor_h > 0.0 && cursor_y < viewport_h.max(1.0);

    if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
        let ascent = if line.qt_ascent > 0.0 {
            line.qt_ascent
        } else {
            get_font_ascent(&snapshot.font_family, snapshot.font_size)
        };
        let descent = if line.qt_descent > 0.0 {
            line.qt_descent
        } else {
            get_font_descent(&snapshot.font_family, snapshot.font_size)
        };
        let text_baseline =
            text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);
        let cursor_top_to_baseline = text_baseline - cursor_y_doc;
        let cursor_bottom_to_baseline = cursor_y_doc + cursor_h - text_baseline;
        eprintln!(
            "[caret_rect] cursor={}, affinity={:?}, visual_line_id={}, line.y={:.1}, line.height={:.1}, line.x={:.1}, line.width={:.1}, target_x={:.1}, target_y={:.1}, cursor_h={:.1}, text_baseline_y={:.1}, font_ascent={:.1}, font_descent={:.1}, cursor_top_to_baseline={:.1}, cursor_bottom_to_baseline={:.1}, qt_ascent={:.1}, qt_descent={:.1}",
            cursor_byte, affinity, line.id, line.y, line.height, line.x, line.width,
            cursor_x, cursor_y_doc, cursor_h, text_baseline, ascent, descent,
            cursor_top_to_baseline, cursor_bottom_to_baseline, line.qt_ascent, line.qt_descent
        );
    }

    // Issue #712: baseline_y 从 QTextLine 的真实 ascent/descent 计算，
    // 不使用 `top + h * 0.8` 估算。text_baseline_y 返回文档坐标系 baseline。
    let baseline_y_doc =
        text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);
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

/// Issue #722 评论 5748596920 问题1: canonical caret 文档坐标入口（自由函数版）。
///
/// 与 `caret_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标（不减 scroll_y），
/// `visible` 始终为 true。供正文事务 caret track 使用，使 caret track 与
/// AnimatedSlice/StaticPatch 文档坐标系一致。
pub fn caret_rect_doc(
    snapshot: &LayoutSnapshot,
    cursor_byte: usize,
    affinity: CaretAffinity,
) -> CaretRect {
    let line = snapshot
        .lines
        .iter()
        .enumerate()
        .find(|(idx, _)| {
            line_contains_cursor_with_affinity(&snapshot.lines, *idx, cursor_byte, affinity)
        })
        .map(|(_, line)| line)
        .or_else(|| snapshot.lines.last());

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
                height: f64::from(snapshot.font_size) * f64::from(snapshot.line_spacing),
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

    let cursor_x = calculate_cursor_x_for_line(line, cursor_byte, affinity, snapshot);
    let (cursor_y_doc, cursor_h) =
        cursor_rect_for_line(line, f64::from(snapshot.font_size), &snapshot.font_family);
    let baseline_y_doc =
        text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);

    CaretRect {
        x: cursor_x,
        y: cursor_y_doc,
        h: cursor_h,
        visual_line_id: line.id,
        visible: true,
        baseline_y: baseline_y_doc,
    }
}

pub fn index_at_line_x(snapshot: &LayoutSnapshot, line: &VisualLine, x: f64) -> usize {
    let relative = (x - line.x).max(0.0);
    if line.para_text.is_empty() {
        return line.byte_start;
    }
    // Issue #658 评论 5621512329 问题 2: 不再 new QTextLayout 重新排版，
    // 直接从 snapshot.layout_generation + line.cache_slot 取已排好的 QTextLine，
    // 调用 xToCursor。para_text 仅用于 QChar↔byte offset 转换，不用于重新排版。
    let qchar_off = super::qt_cache::get_paragraph_layout_x_to_cursor_on_line(
        snapshot.layout_generation,
        line.cache_slot,
        line.qtextline_idx,
        relative,
    );
    let para_byte = qchar_offset_to_byte_offset(&line.para_text, qchar_off as usize);
    line.para_start + para_byte
}

pub fn cursor_line_and_x(
    snapshot: &LayoutSnapshot,
    cursor: usize,
    affinity: CaretAffinity,
) -> Option<(usize, f64)> {
    let lines = &snapshot.lines;
    if lines.is_empty() {
        return None;
    }
    for (idx, line) in lines.iter().enumerate() {
        if line_contains_cursor_with_affinity(lines, idx, cursor, affinity) {
            let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
            return Some((idx, cursor_x));
        }
    }
    lines.last().map(|line| {
        let cursor_x = calculate_cursor_x_for_line(line, cursor, affinity, snapshot);
        (lines.len() - 1, cursor_x)
    })
}

pub fn calculate_cursor_x_for_line(
    line: &VisualLine,
    cursor: usize,
    affinity: CaretAffinity,
    snapshot: &LayoutSnapshot,
) -> f64 {
    if line.para_text.is_empty() {
        if line.width > 0.0 && cursor == line.byte_end {
            line.x + line.width
        } else {
            line.x
        }
    } else {
        // Issue #658 评论 5621512329 问题 2: 不再 new QTextLayout 重新排版，
        // 直接从 snapshot.layout_generation + line.cache_slot 取已排好的 QTextLine，
        // 调用 cursorToX。para_text 仅用于 QChar↔byte offset 转换，不用于重新排版。
        let use_trailing = affinity == CaretAffinity::Upstream && cursor == line.byte_end;
        let cursor_in_para = cursor.saturating_sub(line.para_start);
        let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para) as i32;
        let x = line.x
            + super::qt_cache::get_paragraph_layout_cursor_to_x_on_line(
                snapshot.layout_generation,
                line.cache_slot,
                line.qtextline_idx,
                cursor_qchar,
                use_trailing,
            );

        // Fallback: if cursorToX returns near-zero for a non-empty line,
        // use the cached x_end_trailing as a last resort.
        if x <= line.x + 0.5
            && line.byte_start != line.byte_end
            && affinity == CaretAffinity::Upstream
            && cursor == line.byte_end
            && line.x_end_trailing > 0.0
        {
            let fallback_x = line.x + line.x_end_trailing;
            crate::sujian_editor_item::editor_debug_log(&format!(
                    "[calculate_cursor_x] fallback to cached x_end_trailing: cursor={}, line.byte_end={}, x_end_trailing={:.4}, realtime_x={:.4}, fallback_x={:.4}",
                    cursor, line.byte_end, line.x_end_trailing, x, fallback_x
                ));
            return fallback_x;
        }

        x
    }
}

/// Determine the caret affinity for a given byte index on a visual line.
///
/// At a wrap boundary (where the cursor sits at the end of one line and the
/// start of the next), we use QTextLine::cursorToX with Leading vs Trailing
/// to decide: if the two positions differ, the cursor is at a wrap boundary
/// and should use Upstream affinity so it renders at the end of the current
/// line rather than the start of the next.
pub fn affinity_for_index_on_line(line: &VisualLine, index: usize) -> CaretAffinity {
    if line.byte_start == line.byte_end || line.para_text.is_empty() {
        return CaretAffinity::Downstream;
    }
    // Only the line-end position can be a wrap boundary
    if index != line.byte_end {
        return CaretAffinity::Downstream;
    }
    // Convert byte index to qchar index within the paragraph.
    let cursor_in_para = index.saturating_sub(line.para_start);
    let cursor_qchar = byte_offset_to_qchar_offset(&line.para_text, cursor_in_para);
    // If the qchar index equals para_qchar_end, the cursor is at the line end
    // in qchar space, which means it's a wrap boundary candidate.
    if cursor_qchar == line.para_qchar_end && line.para_qchar_start != line.para_qchar_end {
        CaretAffinity::Upstream
    } else {
        CaretAffinity::Downstream
    }
}

pub fn line_contains_cursor_with_affinity(
    lines: &[VisualLine],
    idx: usize,
    cursor: usize,
    affinity: CaretAffinity,
) -> bool {
    let line = &lines[idx];
    if line.byte_start == line.byte_end {
        return cursor == line.byte_start;
    }
    if cursor > line.byte_start && cursor < line.byte_end {
        return true;
    }
    if cursor == line.byte_start {
        let has_prev_overlap = idx > 0 && lines[idx - 1].byte_end == line.byte_start;
        if has_prev_overlap {
            return affinity == CaretAffinity::Downstream;
        }
        return true;
    }
    if cursor == line.byte_end {
        let has_next_overlap = idx + 1 < lines.len() && lines[idx + 1].byte_start == line.byte_end;
        if has_next_overlap {
            return affinity == CaretAffinity::Upstream;
        }
        return true;
    }
    false
}
