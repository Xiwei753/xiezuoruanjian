use super::input_host::is_left_button_pressed;
use super::*;
use super::transaction_key::VisualTransactionKey;
use super::transaction_queue::VisualTransactionState;
use super::visual_payload::VisualPayload;
use super::animation_mode::AnimationMode;
use super::cursor_animation::CursorBlinkMode;
use super::texture_cache::TextureCache;
use super::insert_animation;
use super::delete_animation;
use super::reflow_animation;
use std::time::Instant;

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        input::install_event_filter(obj_ptr, item_ptr);
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        self.scroll_buffer = None;
        self.recalculate_content_height_and_emit();
        self.cursor_ctrl.force_snap_next = true;
        let _ = self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    fn mouse_event(&mut self, event: QMouseEvent) -> bool {
        let pos = event.position();
        match event.event_type() {
            qmetaobject::QMouseEventType::MouseButtonPress => {
                self.click_at(pos.x as f32, pos.y as f32, false);
                let obj_ptr = self.get_cpp_object();
                input::focus_item(obj_ptr);
            }
            qmetaobject::QMouseEventType::MouseMove => {
                if is_left_button_pressed(&event) {
                    self.drag_select_at(pos.x as f32, pos.y as f32);
                }
            }
            qmetaobject::QMouseEventType::MouseButtonRelease => {}
            _ => {}
        }
        true
    }

    fn update_paint_node(
        &mut self,
        node: qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode>,
    ) -> qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode> {
        use qmetaobject::scenegraph::SGNode;

        let frame_start = Instant::now();

        self.tick_text_animations();

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };
        let root_raw = node.into_raw();

        let vp_h = self.current_viewport_height.max(1.0) as f64;
        let scroll_y = self.current_scroll_y as f64;
        let content_h = self.current_content_height as f64;

        let mut force_rebuild = false;
        if let Some(ref buf) = self.scroll_buffer {
            let revision_changed = buf.text_revision != self.text_revision;
            if revision_changed {
                force_rebuild = true;
            } else {
                let relative_src_y = scroll_y - buf.buffer_scroll_y;
                if relative_src_y < -0.1 || relative_src_y + vp_h > buf.buffer_logical_h + 0.1 {
                    force_rebuild = true;
                } else {
                    let content_changed = (content_h - buf.buffer_content_h).abs() > 1.0;
                    let dpr_changed = (dpr - buf.dpr).abs() > 0.01;
                    if content_changed || dpr_changed || !buf.contains_viewport(scroll_y, vp_h) {
                        force_rebuild = true;
                    }
                }
            }
        } else {
            force_rebuild = true;
        }

        if force_rebuild {
            self.render_dirty = true;
        }

        let final_root;

        if !root_raw.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_four_layer_nodes(root_raw, item_ptr);
        }

        if self.render_dirty {
            match self.render_to_image() {
                Some((image, buf_scroll_y, _buf_h)) => {
                    let (src_y, src_h) = if let Some(ref buf) = self.scroll_buffer {
                        buf.clamp_source_rect(scroll_y, vp_h)
                    } else {
                        (scroll_y - buf_scroll_y, vp_h)
                    };
                    let logical_img_w = image.size().width as f64 / dpr;
                    final_root = scene_graph::update_texture_node(
                        root_raw,
                        item_ptr,
                        &image,
                        0.0,
                        src_y,
                        logical_img_w,
                        src_h,
                        0.0,
                        vp_h,
                        dpr,
                    );
                    self.render_dirty = false;
                }
                None => {
                    if !root_raw.is_null() {
                        if let Some(ref buf) = self.scroll_buffer {
                            let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                            let logical_img_w = buf.image.size().width as f64 / dpr;
                            scene_graph::update_source_rect(
                                root_raw,
                                item_ptr,
                                0.0,
                                src_y,
                                logical_img_w,
                                src_h,
                                0.0,
                                vp_h,
                                dpr,
                            );
                        }
                    }
                    final_root = root_raw;
                }
            }
        } else {
            if !root_raw.is_null() {
                if let Some(ref buf) = self.scroll_buffer {
                    let (src_y, src_h) = buf.clamp_source_rect(scroll_y, vp_h);
                    let logical_img_w = buf.image.size().width as f64 / dpr;
                    scene_graph::update_source_rect(
                        root_raw,
                        item_ptr,
                        0.0,
                        src_y,
                        logical_img_w,
                        src_h,
                        0.0,
                        vp_h,
                        dpr,
                    );
                }
            }
            final_root = root_raw;
        }

        // ── Build RenderPlan from coordinator ──
        if !final_root.is_null() && !item_ptr.is_null() {
            let now = Instant::now();
            let active_txs: Vec<animation_coordinator::ActiveVisualTransaction> =
                self.animation_coordinator.vt_queue.active_transactions().to_vec();

            if active_txs.is_empty() {
                self.texture_cache.clear();
                scene_graph::clear_animation_layer(final_root, item_ptr);
            } else {
                let mut glyph_data: Vec<f64> = Vec::new();
                let mut glyph_images: Vec<qmetaobject::QImage> = Vec::new();
                let mut glyph_texture_changed: Vec<bool> = Vec::new();
                let mut txs_to_mark_rendering: Vec<VisualTransactionKey> = Vec::new();
                let mut txs_to_complete: Vec<VisualTransactionKey> = Vec::new();
                let mut keys_with_texture_failure: Vec<VisualTransactionKey> = Vec::new();

                for tx in &active_txs {
                    if tx.state == VisualTransactionState::Prepared {
                        txs_to_mark_rendering.push(tx.key);
                    }

                    let just_prepared = !tx.texture_prepared;
                    if just_prepared {
                        let mut textures: Vec<Option<qmetaobject::QImage>> = Vec::new();
                        let font_size = self.current_font_pixel_size as f64;
                        let font_family = self.current_font_family.to_string();
                        let plain_text = self.plain_text.to_string();

                        match &tx.payload {
                            VisualPayload::InsertRuns { insert_runs, reflow_runs, .. } => {
                                for run in insert_runs {
                                    let para_text = Self::extract_paragraph_for_glyph(
                                        &plain_text, run.byte_start, run.byte_end,
                                    );
                                    textures.push(self.render_snapshot_from_static_layout(
                                        &para_text, run.x, run.y, run.w, run.h,
                                        run.baseline_y, font_size, &font_family, dpr,
                                    ).or_else(|| self.render_glyph_texture_from_layout(
                                        &run.char_, run.w, run.h,
                                        run.baseline_y - run.y, dpr,
                                    )));
                                }
                                for run in reflow_runs {
                                    let para_text = Self::extract_paragraph_for_glyph(
                                        &plain_text, run.byte_start, run.byte_end,
                                    );
                                    textures.push(self.render_snapshot_from_static_layout(
                                        &para_text, run.new_x, run.new_y, run.w, run.h,
                                        run.new_baseline_y, font_size, &font_family, dpr,
                                    ).or_else(|| self.render_glyph_texture_from_layout(
                                        &run.char_, run.w, run.h,
                                        run.new_baseline_y - run.new_y, dpr,
                                    )));
                                }
                            }
                            VisualPayload::DeleteRuns { delete_runs, .. } => {
                                for run in delete_runs {
                                    let para_text = Self::extract_paragraph_for_glyph(
                                        &plain_text, run.byte_start, run.byte_end,
                                    );
                                    textures.push(self.render_snapshot_from_static_layout(
                                        &para_text, run.x, run.y, run.w, run.h,
                                        run.baseline_y, font_size, &font_family, dpr,
                                    ).or_else(|| self.render_glyph_texture_from_layout(
                                        &run.char_, run.w, run.h,
                                        run.baseline_y - run.y, dpr,
                                    )));
                                }
                            }
                            VisualPayload::ReflowRuns { insert_runs, reflow_runs, .. } => {
                                for run in insert_runs {
                                    let para_text = Self::extract_paragraph_for_glyph(
                                        &plain_text, run.byte_start, run.byte_end,
                                    );
                                    textures.push(self.render_snapshot_from_static_layout(
                                        &para_text, run.x, run.y, run.w, run.h,
                                        run.baseline_y, font_size, &font_family, dpr,
                                    ).or_else(|| self.render_glyph_texture_from_layout(
                                        &run.char_, run.w, run.h,
                                        run.baseline_y - run.y, dpr,
                                    )));
                                }
                                for run in reflow_runs {
                                    let para_text = Self::extract_paragraph_for_glyph(
                                        &plain_text, run.byte_start, run.byte_end,
                                    );
                                    textures.push(self.render_snapshot_from_static_layout(
                                        &para_text, run.new_x, run.new_y, run.w, run.h,
                                        run.new_baseline_y, font_size, &font_family, dpr,
                                    ).or_else(|| self.render_glyph_texture_from_layout(
                                        &run.char_, run.w, run.h,
                                        run.new_baseline_y - run.new_y, dpr,
                                    )));
                                }
                            }
                            VisualPayload::CursorTransition { .. } => {}
                        }

                        let any_failed = textures.iter().any(|t| t.is_none());
                        if any_failed {
                            keys_with_texture_failure.push(tx.key);
                            editor_animation_debug_log(&format!(
                                "update_paint_node: texture failed for tid={}, cancelling this transaction only",
                                tx.key.transaction_id
                            ));
                        } else {
                            self.texture_cache.insert(
                                tx.key,
                                textures.into_iter().map(|t| t.unwrap()).collect(),
                            );
                            self.animation_coordinator.vt_queue.mark_texture_prepared(tx.key);
                        }
                    }

                    let elapsed_ms = now.duration_since(tx.start_time).as_millis() as f64;
                    let progress = (elapsed_ms / tx.duration_ms as f64).min(1.0);

                    if progress >= 1.0 {
                        txs_to_complete.push(tx.key);
                        continue;
                    }

                    // Skip rendering for transactions whose texture failed
                    if keys_with_texture_failure.contains(&tx.key) {
                        continue;
                    }

                    let cached = self.texture_cache.get(&tx.key);
                    if cached.is_none() {
                        continue;
                    }
                    let cached_textures = cached.cloned().unwrap_or_default();

                    match &tx.payload {
                        VisualPayload::InsertRuns { insert_runs, reflow_runs, old_cursor_rect, .. } => {
                            let frames = animation_coordinator::insert_animation::compute_insert_animation_frame(
                                insert_runs, reflow_runs, old_cursor_rect.as_ref(), progress,
                            );
                            let mut tex_idx = 0usize;
                            for frame in &frames {
                                glyph_data.extend_from_slice(&[frame.x, frame.y, frame.w, frame.h, frame.opacity, frame.baseline_in_quad]);
                                if just_prepared && tex_idx < cached_textures.len() {
                                    glyph_images.push(cached_textures[tex_idx].clone());
                                    glyph_texture_changed.push(true);
                                } else {
                                    glyph_images.push(qmetaobject::QImage::new(
                                        qmetaobject::QSize { width: 1, height: 1 },
                                        qmetaobject::ImageFormat::ARGB32_Premultiplied,
                                    ));
                                    glyph_texture_changed.push(false);
                                }
                                tex_idx += 1;
                            }
                        }
                        VisualPayload::DeleteRuns { delete_runs, .. } => {
                            let frames = animation_coordinator::delete_animation::compute_delete_animation_frame(
                                delete_runs, progress,
                            );
                            let mut tex_idx = 0usize;
                            for frame in &frames {
                                glyph_data.extend_from_slice(&[frame.x, frame.y, frame.w, frame.h, frame.opacity, frame.baseline_in_quad]);
                                if just_prepared && tex_idx < cached_textures.len() {
                                    glyph_images.push(cached_textures[tex_idx].clone());
                                    glyph_texture_changed.push(true);
                                } else {
                                    glyph_images.push(qmetaobject::QImage::new(
                                        qmetaobject::QSize { width: 1, height: 1 },
                                        qmetaobject::ImageFormat::ARGB32_Premultiplied,
                                    ));
                                    glyph_texture_changed.push(false);
                                }
                                tex_idx += 1;
                            }
                        }
                        VisualPayload::ReflowRuns { insert_runs, reflow_runs, old_cursor_rect, .. } => {
                            let frames = animation_coordinator::reflow_animation::compute_reflow_animation_frame(
                                insert_runs, reflow_runs, old_cursor_rect.as_ref(), progress,
                            );
                            let mut tex_idx = 0usize;
                            for frame in &frames {
                                glyph_data.extend_from_slice(&[frame.x, frame.y, frame.w, frame.h, frame.opacity, frame.baseline_in_quad]);
                                if just_prepared && tex_idx < cached_textures.len() {
                                    glyph_images.push(cached_textures[tex_idx].clone());
                                    glyph_texture_changed.push(true);
                                } else {
                                    glyph_images.push(qmetaobject::QImage::new(
                                        qmetaobject::QSize { width: 1, height: 1 },
                                        qmetaobject::ImageFormat::ARGB32_Premultiplied,
                                    ));
                                    glyph_texture_changed.push(false);
                                }
                                tex_idx += 1;
                            }
                        }
                        VisualPayload::CursorTransition { .. } => {}
                    }
                }

                for key in &txs_to_mark_rendering {
                    self.animation_coordinator.vt_queue.mark_rendering(*key);
                    editor_animation_debug_log(&format!(
                        "update_paint_node: VT tid={} → Rendering", key.transaction_id
                    ));
                }

                // Per-key texture failure: cancel only the failed transaction
                for key in &keys_with_texture_failure {
                    self.animation_coordinator.cancel_by_key(*key, "texture_failed");
                    self.texture_cache.remove(key);
                    self.render_dirty = true;
                }

                let glyph_count = glyph_data.len() / 6;
                if glyph_count > 0 && glyph_count == glyph_images.len() {
                    let glyph_data_ptr = glyph_data.as_ptr();
                    let image_ptrs: Vec<*const qmetaobject::QImage> =
                        glyph_images.iter().map(|img| img as *const qmetaobject::QImage).collect();
                    let image_ptrs_ptr = image_ptrs.as_ptr();
                    let texture_changed_ptr = glyph_texture_changed.as_ptr();

                    scene_graph::update_animation_layer(
                        final_root,
                        item_ptr,
                        glyph_count as i32,
                        glyph_data_ptr,
                        image_ptrs_ptr,
                        texture_changed_ptr,
                    );
                } else if keys_with_texture_failure.is_empty() {
                    scene_graph::clear_animation_layer(final_root, item_ptr);
                }

                for key in &txs_to_complete {
                    self.animation_coordinator.finish_by_key(*key);
                    self.texture_cache.remove(key);
                    editor_animation_debug_log(&format!(
                        "update_paint_node: VT tid={}, gen={} completed (progress >= 1.0)",
                        key.transaction_id, key.generation
                    ));
                }

                if !txs_to_complete.is_empty() {
                    self.render_dirty = true;
                }

                if self.animation_coordinator.vt_queue.has_active() || !txs_to_complete.is_empty() {
                    self.request_frame_update();
                }
            }

            // ── Update cursor node from RenderPlan ──
            if self.current_editor_enabled {
                let cursor_visible = self.cursor_ctrl.visible
                    && !self.buffer.has_selection();
                let blink_mode = if self.current_coordinated_text_cursor_animation_enabled
                    && self.animation_coordinator.has_active_insert()
                {
                    animation_coordinator::CursorBlinkMode::Suppressed
                } else {
                    animation_coordinator::CursorBlinkMode::Normal
                };
                let cursor_opacity = if cursor_visible {
                    self.cursor_ctrl.cursor_blink_opacity(blink_mode)
                } else {
                    0.0
                };
                let color_bytes = self.current_cursor_color.to_string();
                let color_cstr = color_bytes.as_bytes();
                scene_graph::update_cursor_node(
                    final_root,
                    item_ptr,
                    self.cursor_ctrl.visual_x,
                    self.cursor_ctrl.visual_y,
                    2.0,
                    self.cursor_ctrl.visual_h,
                    cursor_opacity,
                    color_cstr.as_ptr(),
                    color_cstr.len(),
                );
            }
        }

        let total_elapsed = frame_start.elapsed();
        if total_elapsed.as_millis() > 4 {
            editor_debug_log(&format!(
                "sujian_update_paint_node: total_ms={}",
                total_elapsed.as_millis(),
            ));
        }

        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}

impl SujianEditorItem {
    fn extract_paragraph_for_glyph(plain_text: &str, byte_start: usize, byte_end: usize) -> String {
        let text_bytes = plain_text.as_bytes();
        let mut para_start = byte_start;
        while para_start > 0 && text_bytes[para_start - 1] != b'\n' {
            para_start -= 1;
        }
        let mut para_end = byte_end.min(text_bytes.len());
        while para_end < text_bytes.len() && text_bytes[para_end] != b'\n' {
            para_end += 1;
        }
        plain_text[para_start..para_end].to_string()
    }
}
