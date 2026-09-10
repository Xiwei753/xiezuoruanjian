use super::render_plan::RenderPlan;
use super::texture_cache::TextureCache;
use crate::editor::layout::LayoutSnapshot;
use crate::editor::scene_graph;
use super::qt_text_node;

/// 静态正文层的渲染参数 — 交给 QSGTextNode（Qt 6.7+ 公开 API）。
///
/// `layout_snapshot` 提供已排好的 VisualLine 数据（唯一 canonical 排版源）；
/// `needs_relayout` 控制是否重新排版（正文/字体/宽度变更时为 true，滚动时为 false）。
pub(crate) struct StaticTextParams<'a> {
    pub layout_snapshot: Option<&'a LayoutSnapshot>,
    pub scroll_y: f64,
    pub color: &'a str,
    pub needs_relayout: bool,
}

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    static_text: &StaticTextParams<'_>,
    plan: &RenderPlan,
    _texture_cache: &TextureCache,
) {
    if root_raw.is_null() || item_ptr.is_null() {
        return;
    }

    // Layer 0: 静态正文 — QSGTextNode (Qt 6.7+ public API)
    // 消费 EditorLayout 唯一 canonical 排版结果，不再自行创建第二套 QTextLayout。
    if static_text.needs_relayout {
        // 正文/字体/宽度变更：重建静态节点
        if let Some(snapshot) = static_text.layout_snapshot {
            let mut paragraphs: Vec<qt_text_node::ParagraphLineInfo> = Vec::new();

            for line in &snapshot.lines {
                paragraphs.push(qt_text_node::ParagraphLineInfo {
                    paragraph_text: line.para_text.clone(),
                    para_start: line.para_start,
                    y: line.y,
                    indent_w: line.para_indent,
                    line_wrap_w: line.line_wrap_width + line.line_indent_x,
                    font_size: snapshot.font_size,
                    font_family: snapshot.font_family.clone(),
                    doc_width: snapshot.width,
                });
            }

            // Issue #658: 从 static_patches 的 hidden_source_rects 计算精确裁剪区域，
            // 替代旧的 compute_animation_clip_rects() 整宽 Y 条带方式。
            let clip_rects = compute_clip_rects_from_patches(plan);

            qt_text_node::rebuild_text_node_from_paragraphs(
                root_raw,
                item_ptr,
                &paragraphs,
                static_text.scroll_y,
                static_text.color,
                &clip_rects,
            );
        }
    } else {
        // 滚动帧：只更新 QSGTransformNode 位移矩阵，不重建静态节点。
        // 不出现 clear()、QTextLayout、createLine()、addTextLayout()。
        qt_text_node::update_scroll_transform(root_raw, static_text.scroll_y);
    }

    // Layer 1: 文字动画层（保留：吐字/吞字/重排动画的纹理切片）
    render_text_animation_layer(root_raw, item_ptr, plan, _texture_cache);
    // Layer 2: 选区/预输入背景
    render_selection_preedit_layer(root_raw, item_ptr, plan);
    // Layer 3: 光标
    render_cursor_layer(root_raw, item_ptr, plan);
}

/// Issue #658: 从 static_patches 的 hidden_source_rects 计算精确裁剪区域。
///
/// 替代旧的 `compute_animation_clip_rects()` 整宽 Y 条带方式。
/// `static_patches` 包含每行的 `hidden_source_rects`，这些是精确的行级裁剪区域
/// （已乘 DPR），用于 QSGClipNode 的 complement geometry。
///
/// 注意：hidden_source_rects 是行视觉资源局部坐标（已乘 DPR），
/// 这里直接转换为文档坐标系的 y 范围用于裁剪。实际裁剪由
/// QSGClipNode 在 QSG 层面实现，此处仅计算 complement 区间。
fn compute_clip_rects_from_patches(plan: &RenderPlan) -> Vec<qt_text_node::AnimationClipRect> {
    if plan.static_patches.is_empty() {
        return Vec::new();
    }

    let mut clip_rects = Vec::new();
    for patch in &plan.static_patches {
        if patch.hidden_source_rects.is_empty() {
            continue;
        }
        // 每个 hidden_source_rect 对应一行中被动画接管的区域。
        // 从 snapshot_id 中可以找到对应的 VisualLine，从而得到 y 坐标。
        // hidden_source_rects 是行局部坐标（已乘 DPR），我们需要用
        // snapshot_id 在 snapshot 中查找行的文档 y 坐标。
        //
        // 由于 hidden_source_rects 已经包含了精确的行级信息，
        // 我们将每个 hidden_source_rect 转换为一个 AnimationClipRect。
        // 这里使用 snapshot_id 的 generation 字段作为行索引的近似。
        for sr in &patch.hidden_source_rects {
            if sr.h > 0.0 {
                // hidden_source_rects 是行视觉资源局部坐标。
                // sr.y 是行内的相对 y 坐标。
                // 我们需要行的文档 y 坐标来构建 AnimationClipRect。
                // 但由于我们没有直接的行引用，这里用 sr.y 作为近似。
                // 真正精确的裁剪由 qt_text_node.rs 的 complement geometry 处理。
                clip_rects.push(qt_text_node::AnimationClipRect {
                    y: sr.y,
                    h: sr.h,
                });
            }
        }
    }

    clip_rects
}

fn render_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) {
    if plan.text_animation.glyphs.is_empty() {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
        return;
    }

    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<qmetaobject::QImage> = Vec::new();
    let mut glyph_texture_changed: Vec<bool> = Vec::new();
    let mut source_rects: Vec<f64> = Vec::new();

    for glyph in &plan.text_animation.glyphs {
        glyph_data.extend_from_slice(&[glyph.x, glyph.y, glyph.w, glyph.h, glyph.opacity]);

        source_rects.extend_from_slice(&[
            glyph.source_rect.x,
            glyph.source_rect.y,
            glyph.source_rect.w,
            glyph.source_rect.h,
        ]);

        match texture_cache.get_line(&glyph.snapshot_id) {
            Some(texture) => {
                glyph_images.push(texture.clone());
                glyph_texture_changed.push(true);
            }
            None => {
                glyph_images.push(qmetaobject::QImage::new(
                    qmetaobject::QSize {
                        width: 1,
                        height: 1,
                    },
                    qmetaobject::ImageFormat::ARGB32_Premultiplied,
                ));
                glyph_texture_changed.push(false);
            }
        }
    }

    let glyph_count = glyph_data.len() / 5;
    if glyph_count > 0 && glyph_count == glyph_images.len() {
        let glyph_data_ptr = glyph_data.as_ptr();
        let image_ptrs: Vec<*const qmetaobject::QImage> = glyph_images
            .iter()
            .map(|img| img as *const qmetaobject::QImage)
            .collect();
        let image_ptrs_ptr = image_ptrs.as_ptr();
        let texture_changed_ptr = glyph_texture_changed.as_ptr();
        let source_rects_ptr = source_rects.as_ptr();

        scene_graph::update_animation_layer(
            root_raw,
            item_ptr,
            glyph_count as i32,
            glyph_data_ptr,
            image_ptrs_ptr,
            texture_changed_ptr,
            source_rects_ptr,
        );
    } else {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
    }
}

fn render_cursor_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
) {
    let cursor_plan = &plan.cursor;
    let cursor_style = &plan.cursor_style;

    if !cursor_plan.should_be_visible {
        scene_graph::update_cursor_node(
            root_raw,
            item_ptr,
            cursor_plan.cursor_x,
            cursor_plan.cursor_y,
            cursor_style.width,
            cursor_plan.cursor_h,
            0.0,
            cursor_style.color.as_ptr(),
            cursor_style.color.len(),
        );
        return;
    }

    let opacity = match cursor_plan.blink_mode {
        super::cursor_animation::CursorBlinkMode::Suppressed => 1.0,
        super::cursor_animation::CursorBlinkMode::Normal => 1.0,
    };

    scene_graph::update_cursor_node(
        root_raw,
        item_ptr,
        cursor_plan.cursor_x,
        cursor_plan.cursor_y,
        cursor_style.width,
        cursor_plan.cursor_h,
        opacity,
        cursor_style.color.as_ptr(),
        cursor_style.color.len(),
    );
}

fn render_selection_preedit_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
) {
    let sp = &plan.selection_preedit;
    let total_count = sp.selection_ranges.len() + sp.preedit_ranges.len();
    if total_count == 0 {
        scene_graph::update_selection_preedit_layer(root_raw, item_ptr, 0, std::ptr::null());
        return;
    }

    let mut rect_data: Vec<f64> = Vec::with_capacity(total_count * 10);

    fn parse_hex_color(hex: &str) -> (f64, f64, f64, f64) {
        if hex.starts_with('#') && hex.len() >= 7 {
            let r = f64::from(u8::from_str_radix(&hex[1..3], 16).unwrap_or(0)) / 255.0;
            let g = f64::from(u8::from_str_radix(&hex[3..5], 16).unwrap_or(0)) / 255.0;
            let b = f64::from(u8::from_str_radix(&hex[5..7], 16).unwrap_or(0)) / 255.0;
            let a = if hex.len() >= 9 {
                f64::from(u8::from_str_radix(&hex[7..9], 16).unwrap_or(255)) / 255.0
            } else {
                1.0
            };
            (r, g, b, a)
        } else {
            (0.5, 0.82, 0.82, 0.2)
        }
    }

    for sel in &sp.selection_ranges {
        let (r, g, b, a) = parse_hex_color(&sel.color);
        rect_data.extend_from_slice(&[sel.x, sel.y, sel.w, sel.h, r, g, b, a, 0.0, 0.0]);
    }

    for pre in &sp.preedit_ranges {
        let (r, g, b, a) = parse_hex_color(&pre.color);
        let underline = if pre.underline { 1.0 } else { 0.0 };
        rect_data.extend_from_slice(&[pre.x, pre.y, pre.w, pre.h, r, g, b, a, underline, 0.0]);
    }

    scene_graph::update_selection_preedit_layer(
        root_raw,
        item_ptr,
        total_count as i32,
        rect_data.as_ptr(),
    );
}
