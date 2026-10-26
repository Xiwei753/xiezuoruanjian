use crate::editor::layout::{
    get_paragraph_layout_cursor_to_x_on_line_checked, text_baseline_y,
    CanonicalDocumentVisualSnapshot, CanonicalLineSnapshot, LayoutSnapshot, VisualLine,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum TextDecorationKind {
    SpellErrorWave,
    SpellWarningWave,
    Highlight,
    ManualLinkUnderline,
    AutoNameUnderline,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    pub highlight_vertices: Vec<f64>,
    pub manual_link_vertices: Vec<f64>,
    pub auto_name_vertices: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DecorationLineKey {
    line_id: usize,
    paragraph_id: u64,
    cache_slot: i32,
    layout_epoch: u64,
    qtextline_idx: i32,
    byte_start: usize,
    byte_end: usize,
    x_bits: u64,
    y_bits: u64,
    width_bits: u64,
    height_bits: u64,
}

#[derive(Clone, Debug)]
struct DecorationLineGeometry {
    key: DecorationLineKey,
    paragraph_id: u64,
    error_vertices: Arc<[f64]>,
    warning_vertices: Arc<[f64]>,
    highlight_vertices: Arc<[f64]>,
    manual_link_vertices: Arc<[f64]>,
    auto_name_vertices: Arc<[f64]>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextDecorationGeometry {
    pub document_session: u64,
    pub text_revision: u64,
    pub layout_generation: u64,
    pub signature: u64,
    span_signature: u64,
    dpr_bits: u64,
    visible_line_keys: Vec<DecorationLineKey>,
    line_geometries: Vec<DecorationLineGeometry>,
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
            "highlight" => TextDecorationKind::Highlight,
            "manualLink" => TextDecorationKind::ManualLinkUnderline,
            "autoName" => TextDecorationKind::AutoNameUnderline,
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

/// 判断同一批标记的视口可见行是否变化。小幅滚动仍落在原有行集合时不触发几何生成。
pub(crate) fn geometry_needs_refresh(
    snapshot: &LayoutSnapshot,
    spans: &[TextDecorationSpan],
    document_session: u64,
    scroll_y: f64,
    viewport_height: f64,
    device_pixel_ratio: f64,
    current: &TextDecorationGeometry,
) -> bool {
    let metadata_matches = current.document_session == document_session
        && current.text_revision == snapshot.text_revision
        && current.layout_generation == snapshot.layout_generation
        && current.span_signature == spans_signature(spans)
        && current.dpr_bits == normalized_dpr(device_pixel_ratio).to_bits();
    if !metadata_matches {
        return true;
    }
    if spans.is_empty() || snapshot.layout_generation == 0 {
        return false;
    }
    snapshot
        .resolve_viewport_lines(scroll_y, viewport_height.max(1.0))
        .iter()
        .map(decoration_line_key)
        .ne(current.visible_line_keys.iter().cloned())
}

/// 从 canonical 布局生成视口附近的装饰几何；不创建或重排 QTextLayout。
/// 滚动改变可见行时复用仍在视口内的行几何，只对新进入视口的行读取 QTextLine。
pub(crate) fn build_geometry(
    snapshot: &LayoutSnapshot,
    canonical: Option<&CanonicalDocumentVisualSnapshot>,
    spans: &[TextDecorationSpan],
    document_session: u64,
    scroll_y: f64,
    viewport_height: f64,
    device_pixel_ratio: f64,
    previous: Option<&TextDecorationGeometry>,
) -> TextDecorationGeometry {
    let dpr = normalized_dpr(device_pixel_ratio);
    let span_signature = spans_signature(spans);
    let mut geometry = TextDecorationGeometry {
        document_session,
        text_revision: snapshot.text_revision,
        layout_generation: snapshot.layout_generation,
        span_signature,
        dpr_bits: dpr.to_bits(),
        ..TextDecorationGeometry::default()
    };
    if snapshot.layout_generation == 0 || spans.is_empty() {
        geometry.signature = geometry_signature(&geometry);
        return geometry;
    }

    let mut prefix_max_end = Vec::with_capacity(spans.len());
    let mut max_end = 0;
    for span in spans {
        max_end = max_end.max(span.end_byte);
        prefix_max_end.push(max_end);
    }

    let visible_lines = snapshot.resolve_viewport_lines(scroll_y, viewport_height.max(1.0));
    geometry.visible_line_keys = visible_lines.iter().map(decoration_line_key).collect();
    let Some(canonical) =
        canonical.filter(|canonical| canonical.text_revision == snapshot.text_revision)
    else {
        geometry.signature = geometry_signature(&geometry);
        return geometry;
    };

    let can_reuse_previous = previous.is_some_and(|previous| {
        previous.document_session == document_session
            && previous.text_revision == snapshot.text_revision
            && previous.layout_generation == snapshot.layout_generation
            && previous.span_signature == span_signature
            && previous.dpr_bits == dpr.to_bits()
    });
    let reusable: HashMap<DecorationLineKey, &DecorationLineGeometry> = previous
        .filter(|_| can_reuse_previous)
        .into_iter()
        .flat_map(|previous| previous.line_geometries.iter())
        .map(|line| (line.key.clone(), line))
        .collect();

    for line in &visible_lines {
        let key = decoration_line_key(line);
        if let Some(cached) = reusable.get(&key) {
            geometry.line_geometries.push((*cached).clone());
            continue;
        }
        geometry.line_geometries.push(build_line_geometry(
            snapshot,
            canonical,
            spans,
            &prefix_max_end,
            line,
            key,
            dpr,
        ));
    }

    let mut by_paragraph = BTreeMap::<u64, DecorationParagraphGeometry>::new();
    for line in &geometry.line_geometries {
        let paragraph =
            by_paragraph
                .entry(line.paragraph_id)
                .or_insert_with(|| DecorationParagraphGeometry {
                    paragraph_id: line.paragraph_id,
                    ..DecorationParagraphGeometry::default()
                });
        paragraph
            .error_vertices
            .extend_from_slice(&line.error_vertices);
        paragraph
            .warning_vertices
            .extend_from_slice(&line.warning_vertices);
        paragraph
            .highlight_vertices
            .extend_from_slice(&line.highlight_vertices);
        paragraph
            .manual_link_vertices
            .extend_from_slice(&line.manual_link_vertices);
        paragraph
            .auto_name_vertices
            .extend_from_slice(&line.auto_name_vertices);
    }
    geometry.paragraphs = by_paragraph
        .into_values()
        .filter(|paragraph| {
            !paragraph.error_vertices.is_empty()
                || !paragraph.warning_vertices.is_empty()
                || !paragraph.highlight_vertices.is_empty()
                || !paragraph.manual_link_vertices.is_empty()
                || !paragraph.auto_name_vertices.is_empty()
        })
        .collect();
    geometry.signature = geometry_signature(&geometry);
    geometry
}

fn build_line_geometry(
    snapshot: &LayoutSnapshot,
    canonical: &CanonicalDocumentVisualSnapshot,
    spans: &[TextDecorationSpan],
    prefix_max_end: &[usize],
    line: &VisualLine,
    key: DecorationLineKey,
    dpr: f64,
) -> DecorationLineGeometry {
    let mut error_vertices = Vec::new();
    let mut warning_vertices = Vec::new();
    let mut highlight_vertices = Vec::new();
    let mut manual_link_vertices = Vec::new();
    let mut auto_name_vertices = Vec::new();
    if line.has_qt_layout && line.byte_start < line.byte_end {
        if let Some(canonical_line) = canonical_line_for(canonical, line) {
            let first_span = prefix_max_end.partition_point(|&max_end| max_end <= line.byte_start);
            let last_span = spans.partition_point(|span| span.start_byte < line.byte_end);
            for span in &spans[first_span..last_span] {
                if span.end_byte <= line.byte_start {
                    continue;
                }
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

                let intervals =
                    visual_intervals(snapshot, line, canonical_line, local_start, local_end);
                if intervals.is_empty() {
                    continue;
                }
                let baseline =
                    text_baseline_y(line, f64::from(snapshot.font_size), &snapshot.font_family);
                for (left, right) in intervals {
                    match span.kind {
                        TextDecorationKind::SpellErrorWave
                        | TextDecorationKind::SpellWarningWave => {
                            let available_descent = (line.y + line.height - baseline).max(0.0);
                            if available_descent <= 0.35 / dpr {
                                continue;
                            }
                            let stroke = (0.9 / dpr).min(available_descent * 0.5);
                            let amplitude = (0.8 / dpr)
                                .min((available_descent - stroke).max(0.0) * 0.5);
                            let center_y = baseline + (available_descent - stroke) * 0.5;
                            let vertices = if span.kind == TextDecorationKind::SpellErrorWave {
                                &mut error_vertices
                            } else {
                                &mut warning_vertices
                            };
                            append_wave_strip(vertices, left, right, center_y, amplitude, stroke, dpr);
                        }
                        TextDecorationKind::Highlight => append_rect(
                            &mut highlight_vertices,
                            left,
                            line.y + 1.0 / dpr,
                            right,
                            (line.y + line.height - 1.0 / dpr).max(line.y + 1.0 / dpr),
                        ),
                        TextDecorationKind::ManualLinkUnderline
                        | TextDecorationKind::AutoNameUnderline => {
                            let vertices = if span.kind == TextDecorationKind::ManualLinkUnderline {
                                &mut manual_link_vertices
                            } else {
                                &mut auto_name_vertices
                            };
                            let thickness = 1.0 / dpr;
                            let y = (baseline + 1.0 / dpr).min(line.y + line.height - thickness);
                            append_rect(vertices, left, y, right, y + thickness);
                        }
                    }
                }
            }
        }
    }

    DecorationLineGeometry {
        key,
        paragraph_id: line.paragraph_id.as_u64(),
        error_vertices: Arc::from(error_vertices.into_boxed_slice()),
        warning_vertices: Arc::from(warning_vertices.into_boxed_slice()),
        highlight_vertices: Arc::from(highlight_vertices.into_boxed_slice()),
        manual_link_vertices: Arc::from(manual_link_vertices.into_boxed_slice()),
        auto_name_vertices: Arc::from(auto_name_vertices.into_boxed_slice()),
    }
}

fn canonical_line_for<'a>(
    canonical: &'a CanonicalDocumentVisualSnapshot,
    line: &VisualLine,
) -> Option<&'a CanonicalLineSnapshot> {
    let paragraph_slot = usize::try_from(line.logical_slot).ok()?;
    let line_slot = usize::try_from(line.qtextline_idx).ok()?;
    canonical
        .paragraphs
        .get(paragraph_slot)?
        .lines
        .get(line_slot)
}

fn visual_intervals(
    snapshot: &LayoutSnapshot,
    line: &VisualLine,
    canonical_line: &CanonicalLineSnapshot,
    byte_start: usize,
    byte_end: usize,
) -> Vec<(f64, f64)> {
    let mut intervals = Vec::new();
    let first_cluster = canonical_line
        .cluster_byte_order
        .partition_point(|&index| canonical_line.clusters[index].document_byte_end <= byte_start);
    for &cluster_index in &canonical_line.cluster_byte_order[first_cluster..] {
        let cluster = &canonical_line.clusters[cluster_index];
        if cluster.document_byte_start >= byte_end {
            break;
        }
        if cluster.document_byte_end <= byte_start {
            continue;
        }

        // Query both sides of this canonical glyph cluster from its logical start. In Qt,
        // Trailing advances from a cluster start to that cluster's next visual caret; using
        // it at the half-open end boundary would advance one more cluster.
        let start_qchar = cluster.qchar_start as i32;
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
                start_qchar,
                true,
            );
        let left = leading_x.min(trailing_x);
        let right = leading_x.max(trailing_x);
        if right - left > 0.05 {
            intervals.push((left, right));
        }
    }

    // Merge only visually touching clusters. Bidi runs that are separated in x remain
    // separate intervals even when their logical byte ranges are adjacent.
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(intervals.len());
    for (left, right) in intervals {
        if let Some(last) = merged.last_mut() {
            if left <= last.1 + 0.05 {
                last.1 = last.1.max(right);
                continue;
            }
        }
        merged.push((left, right));
    }
    merged
}

fn decoration_line_key(line: &VisualLine) -> DecorationLineKey {
    DecorationLineKey {
        line_id: line.id,
        paragraph_id: line.paragraph_id.as_u64(),
        cache_slot: line.cache_slot,
        layout_epoch: line.layout_epoch,
        qtextline_idx: line.qtextline_idx,
        byte_start: line.byte_start,
        byte_end: line.byte_end,
        x_bits: line.x.to_bits(),
        y_bits: line.y.to_bits(),
        width_bits: line.width.to_bits(),
        height_bits: line.height.to_bits(),
    }
}

fn normalized_dpr(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}

fn spans_signature(spans: &[TextDecorationSpan]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    spans.hash(&mut hasher);
    hasher.finish()
}

fn geometry_signature(geometry: &TextDecorationGeometry) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    geometry.document_session.hash(&mut hasher);
    geometry.text_revision.hash(&mut hasher);
    geometry.layout_generation.hash(&mut hasher);
    geometry.span_signature.hash(&mut hasher);
    geometry.dpr_bits.hash(&mut hasher);
    geometry.visible_line_keys.hash(&mut hasher);
    for paragraph in &geometry.paragraphs {
        paragraph.paragraph_id.hash(&mut hasher);
        paragraph.error_vertices.len().hash(&mut hasher);
        for vertex in &paragraph.error_vertices {
            vertex.to_bits().hash(&mut hasher);
        }
        paragraph.warning_vertices.len().hash(&mut hasher);
        for vertex in &paragraph.warning_vertices {
            vertex.to_bits().hash(&mut hasher);
        }
        for vertices in [
            &paragraph.highlight_vertices,
            &paragraph.manual_link_vertices,
            &paragraph.auto_name_vertices,
        ] {
            vertices.len().hash(&mut hasher);
            for vertex in vertices {
                vertex.to_bits().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
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

fn append_rect(vertices: &mut Vec<f64>, left: f64, top: f64, right: f64, bottom: f64) {
    if right <= left || bottom <= top {
        return;
    }
    push_vertex(vertices, left, top);
    push_vertex(vertices, right, top);
    push_vertex(vertices, left, bottom);
    push_vertex(vertices, left, bottom);
    push_vertex(vertices, right, top);
    push_vertex(vertices, right, bottom);
}

fn push_vertex(vertices: &mut Vec<f64>, x: f64, y: f64) {
    vertices.push(x);
    vertices.push(y);
}
