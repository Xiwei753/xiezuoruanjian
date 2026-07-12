use crate::editor::scene_graph;
use super::render_plan::RenderPlan;
use super::texture_cache::{TextureCache, TextureCacheKey, LineSnapshotTextureKey};
use qmetaobject::QImage;

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) {
    if root_raw.is_null() || item_ptr.is_null() {
        return;
    }

    render_text_animation_layer(root_raw, item_ptr, plan, texture_cache);
    render_selection_preedit_layer(root_raw, item_ptr, plan);
    render_cursor_layer(root_raw, item_ptr, plan);
}

fn render_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) {
    if plan.text_animation.glyphs.is_empty() && plan.text_animation.slice_items.is_empty() {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
        return;
    }

    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<QImage> = Vec::new();
    let mut glyph_texture_changed: Vec<bool> = Vec::new();

    for glyph in &plan.text_animation.glyphs {
        glyph_data.extend_from_slice(&[
            glyph.x, glyph.y, glyph.w, glyph.h, glyph.opacity, glyph.baseline_in_quad,
        ]);

        let cache_key = TextureCacheKey::new(glyph.key, glyph.texture_phase, glyph.run_identity);
        match texture_cache.get(&cache_key) {
            Some(texture) => {
                glyph_images.push(texture.clone());
                glyph_texture_changed.push(true);
            }
            None => {
                glyph_images.push(QImage::new(
                    qmetaobject::QSize { width: 1, height: 1 },
                    qmetaobject::ImageFormat::ARGB32_Premultiplied,
                ));
                glyph_texture_changed.push(false);
            }
        }
    }

    for slice_item in &plan.text_animation.slice_items {
        let (dst_x, dst_y, dst_w, dst_h) = slice_item.destination_viewport_rect;
        glyph_data.extend_from_slice(&[
            dst_x, dst_y, dst_w, dst_h, slice_item.opacity, dst_h * 0.8,
        ]);

        let line_key = LineSnapshotTextureKey::new(slice_item.snapshot_id);
        match texture_cache.get_line_snapshot(&line_key) {
            Some(texture) => {
                glyph_images.push(texture.clone());
                glyph_texture_changed.push(true);
            }
            None => {
                glyph_images.push(QImage::new(
                    qmetaobject::QSize { width: 1, height: 1 },
                    qmetaobject::ImageFormat::ARGB32_Premultiplied,
                ));
                glyph_texture_changed.push(false);
            }
        }
    }

    let glyph_count = glyph_data.len() / 6;
    if glyph_count > 0 && glyph_count == glyph_images.len() {
        let glyph_data_ptr = glyph_data.as_ptr();
        let image_ptrs: Vec<*const QImage> = glyph_images
            .iter()
            .map(|img| img as *const QImage)
            .collect();
        let image_ptrs_ptr = image_ptrs.as_ptr();
        let texture_changed_ptr = glyph_texture_changed.as_ptr();

        scene_graph::update_animation_layer(
            root_raw,
            item_ptr,
            glyph_count as i32,
            glyph_data_ptr,
            image_ptrs_ptr,
            texture_changed_ptr,
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
            let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(0) as f64 / 255.0;
            let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(0) as f64 / 255.0;
            let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(0) as f64 / 255.0;
            let a = if hex.len() >= 9 {
                u8::from_str_radix(&hex[7..9], 16).unwrap_or(255) as f64 / 255.0
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
