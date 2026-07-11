use crate::editor::scene_graph;
use super::render_plan::RenderPlan;
use super::texture_cache::TextureCache;
use super::transaction_key::VisualTransactionKey;
use qmetaobject::QImage;

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
    dpr: f64,
) {
    if root_raw.is_null() || item_ptr.is_null() {
        return;
    }

    render_text_animation_layer(root_raw, item_ptr, plan, texture_cache, dpr);
    render_cursor_layer(root_raw, item_ptr, plan);
}

fn render_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
    _dpr: f64,
) {
    if plan.text_animation.glyphs.is_empty() {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
        return;
    }

    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<QImage> = Vec::new();
    let mut glyph_texture_changed: Vec<bool> = Vec::new();

    let mut current_key: Option<VisualTransactionKey> = None;
    let mut tex_idx_start: usize = 0;

    for glyph in &plan.text_animation.glyphs {
        glyph_data.extend_from_slice(&[
            glyph.x, glyph.y, glyph.w, glyph.h, glyph.opacity, glyph.baseline_in_quad,
        ]);

        if current_key != Some(glyph.key) {
            current_key = Some(glyph.key);
            tex_idx_start = 0;
        }

        let cached = texture_cache.get(&glyph.key);
        match cached {
            Some(textures) if tex_idx_start < textures.len() => {
                glyph_images.push(textures[tex_idx_start].clone());
                glyph_texture_changed.push(true);
            }
            _ => {
                glyph_images.push(QImage::new(
                    qmetaobject::QSize { width: 1, height: 1 },
                    qmetaobject::ImageFormat::ARGB32_Premultiplied,
                ));
                glyph_texture_changed.push(false);
            }
        }
        tex_idx_start += 1;
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
    if !cursor_plan.should_be_visible {
        scene_graph::update_cursor_node(
            root_raw,
            item_ptr,
            cursor_plan.cursor_x,
            cursor_plan.cursor_y,
            2.0,
            cursor_plan.cursor_h,
            0.0,
            "#000000".as_ptr(),
            7,
        );
        return;
    }

    let opacity = match cursor_plan.blink_mode {
        super::cursor_animation::CursorBlinkMode::Suppressed => 1.0,
        super::cursor_animation::CursorBlinkMode::Normal => 1.0,
    };

    let color_str = "#006497";
    scene_graph::update_cursor_node(
        root_raw,
        item_ptr,
        cursor_plan.cursor_x,
        cursor_plan.cursor_y,
        2.0,
        cursor_plan.cursor_h,
        opacity,
        color_str.as_ptr(),
        color_str.len(),
    );
}
