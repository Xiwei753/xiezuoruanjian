use crate::editor::layout::{
    byte_offset_to_qchar_offset, get_paragraph_layout_cursor_to_x_on_line_checked, text_baseline_y,
    LayoutSnapshot, VisualLine,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TextDecorationKind {
    SpellErrorWave,
    SpellWarningWave,
    Highlight,
    Underline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextDecorationSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: TextDecorationKind,
    pub text_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DecorationParagraphGeometry {
    pub paragraph_id: u64,
    /// 文档坐标中的红色波浪线三角形顶点，按 x,y 顺序排列。
    pub error_vertices: Vec<f64>,
    /// 文档坐标中的橙色波浪线三角形顶点，按 x,y 顺序排列。
    pub warning_vertices: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextDecorationGeometry {
    pub document_session: u64,
    pub text_revision: u64,
    pub layout_generation: u64,
    pub paragraphs: Vec<DecorationParagraphGeometry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonDecorationSpan {
    start_byte: u64,
    end_byte: u64,
    severity: String,
    text_revision: u64,
}

/// 解析 QML 发来的紧凑范围并绑定到当前正文修订。
/// 任意非空批次含过期修订时整体拒收，避免旧异步结果覆盖当前建议。
pub(crate) fn parse_spans(
    json: &str,
    text: &str,
    current_revision: u64,
) -> Option<Vec<TextDecorationSpan>> {
    let raw: Vec<JsonDecorationSpan> = serde_json::from_str(json).ok()?;
    if raw
        .iter()
        .any(|span| span.text_revision != current_revision)
    {
        return None;
    }

    let mut spans = Vec::with_capacity(raw.len());
    for span in raw {
        let (Ok(start_byte), Ok(end_byte)) = (
            usize::try_from(span.start_byte),
            usize::try_from(span.end_byte),
        ) else {
            continue;
        };
        if start_byte >= end_byte
            || end_byte > text.len()
            || !text.is_char_boundary(start_byte)
            || !text.is_char_boundary(end_byte)
        {
            continue;
        }
        let kind = match span.severity.as_str() {
            "error" => TextDecorationKind::SpellErrorWave,
            "warning" => TextDecorationKind::SpellWarningWave,
            _ => continue,
        };
        spans.push(TextDecorationSpan {
            start_byte,
            end_byte,
            kind,
            text_revision: span.text_revision,
        });
    }
    spans.sort_by_key(|span| (span.start_byte, span.end_byte, span.kind as u8));
    spans.dedup();
    Some(spans)
}

/// 从 canonical 布局生成视口附近的装饰几何；不创建或重排 QTextLayout。
pub(crate) fn build_geometry(
    snapshot: &LayoutSnapshot,
    spans: &[TextDecorationSpan],
    document_session: u64,
    scroll_y: f64,
    viewport_height: f64,
    device_pixel_ratio: f64,
) -> TextDecorationGeometry {
    let mut geometry = TextDecorationGeometry {
        document_session,
        text_revision: snapshot.text_revision,
        layout_generation: snapshot.layout_generation,
        paragraphs: Vec::new(),
    };
    if spans.is_empty() || snapshot.layout_generation == 0 {
        return geometry;
    }

    let dpr = if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    };
    let lines = snapshot.resolve_viewport_lines(scroll_y, viewport_height.max(1.0));
    let mut by_paragraph = BTreeMap::<u64, DecorationParagraphGeometry>::new();

    for line in &lines {
        if !line.has_qt_layout || line.byte_start >= line.byte_end {
            continue;
        }
        for span in spans {
            if span.text_revision != snapshot.text_revision {
                continue;
            }
            let byte_start = span.start_byte.max(line.byte_start);
            let byte_end = span.end_byte.min(line.byte_end);
            if byte_start >= byte_end {
                continue;
            }
            let local_start = byte_start.saturating_sub(line.para_start);
            let local_end = byte_end.saturating_sub(line.para_start);
            if local_start >= local_end
                || local_end > line.para_text.len()
                || !line.para_text.is_char_boundary(local_start)
                || !line.para_text.is_char_boundary(local_end)
            {
                continue;
            }

            let intervals = visual_intervals(snapshot, line, local_start, local_end);
            if intervals.is_empty() {
                continue;
            }
            let baseline =
                text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);
            let available_descent = (line.y + line.height - baseline).max(0.0);
            if available_descent <= 0.35 / dpr {
                continue;
            }
            let stroke = (0.9 / dpr).min(available_descent * 0.5);
            let amplitude = (0.8 / dpr).min((available_descent - stroke).max(0.0) * 0.5);
            let center_y = baseline + (available_descent - stroke) * 0.5;
            let paragraph = by_paragraph
                .entry(line.paragraph_id.as_u64())
                .or_insert_with(|| DecorationParagraphGeometry {
                    paragraph_id: line.paragraph_id.as_u64(),
                    ..DecorationParagraphGeometry::default()
                });
            let vertices = match span.kind {
                TextDecorationKind::SpellErrorWave => &mut paragraph.error_vertices,
                TextDecorationKind::SpellWarningWave => &mut paragraph.warning_vertices,
                TextDecorationKind::Highlight | TextDecorationKind::Underline => continue,
            };
            for (left, right) in intervals {
                append_wave_strip(vertices, left, right, center_y, amplitude, stroke, dpr);
            }
        }
    }
    geometry.paragraphs = by_paragraph.into_values().collect();
    geometry
}

fn visual_intervals(
    snapshot: &LayoutSnapshot,
    line: &VisualLine,
    byte_start: usize,
    byte_end: usize,
) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    for (char_byte, character) in line.para_text.char_indices() {
        let char_end = char_byte + character.len_utf8();
        if char_byte < byte_start || char_end > byte_end {
            continue;
        }
        let start_qchar = byte_offset_to_qchar_offset(&line.para_text, char_byte) as i32;
        let end_qchar = byte_offset_to_qchar_offset(&line.para_text, char_end) as i32;
        let leading_x = line.x
            + get_paragraph_layout_cursor_to_x_on_line_checked(
                snapshot.layout_generation,
                line.cache_slot,
                line.layout_epoch,
                line.qtextline_idx,
                start_qchar,
                false,
            );
        let trailing_x = line.x
            + get_paragraph_layout_cursor_to_x_on_line_checked(
                snapshot.layout_generation,
                line.cache_slot,
                line.layout_epoch,
                line.qtextline_idx,
                end_qchar,
                true,
            );
        let left = leading_x.min(trailing_x);
        let right = leading_x.max(trailing_x);
        if right - left > 0.05 {
            intervals.push((left, right));
        }
    }

    // 按实际视觉位置合并相邻字形。双向文字会形成互不相邻的区间，不会误连成一条横跨
    // 其他视觉 run 的下划线。
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(intervals.len());
    for (left, right) in intervals {
        if let Some(last) = merged.last_mut() {
            if left <= last.1 + 0.35 {
                last.1 = last.1.max(right);
                continue;
            }
        }
        merged.push((left, right));
    }
    merged
}

fn append_wave_strip(
    vertices: &mut Vec<f64>,
    left: f64,
    right: f64,
    center_y: f64,
    amplitude: f64,
    stroke: f64,
    dpr: f64,
) {
    let wavelength = 4.0 / dpr;
    let step = (wavelength * 0.25).max(0.2);
    let mut x = left;
    let mut y = center_y + amplitude * (std::f64::consts::TAU * x / wavelength).sin();
    while x < right {
        let next_x = (x + step).min(right);
        let next_y = center_y + amplitude * (std::f64::consts::TAU * next_x / wavelength).sin();
        let dx = next_x - x;
        let dy = next_y - y;
        let length = dx.hypot(dy).max(f64::EPSILON);
        let nx = -dy / length * stroke * 0.5;
        let ny = dx / length * stroke * 0.5;
        push_vertex(vertices, x + nx, y + ny);
        push_vertex(vertices, x - nx, y - ny);
        push_vertex(vertices, next_x + nx, next_y + ny);
        push_vertex(vertices, x - nx, y - ny);
        push_vertex(vertices, next_x + nx, next_y + ny);
        push_vertex(vertices, next_x - nx, next_y - ny);
        x = next_x;
        y = next_y;
    }
}

fn push_vertex(vertices: &mut Vec<f64>, x: f64, y: f64) {
    vertices.push(x);
    vertices.push(y);
}
