use super::input_host::is_left_button_pressed;
use super::*;
use super::transaction_key::VisualTransactionKey;
use super::transaction_queue::VisualTransactionState;

use super::render_plan::{FrameContext, CursorStyle};
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

        if !final_root.is_null() && !item_ptr.is_null() {
            let active_txs: Vec<animation_coordinator::ActiveVisualTransaction> =
                self.animation_coordinator.vt_queue.active_transactions().to_vec();

            if active_txs.is_empty() {
                self.texture_cache.clear();
                scene_graph::clear_animation_layer(final_root, item_ptr);
            }

            let old_cursor_rect = active_txs.first().and_then(|tx| tx.payload.cursor_rects().0.cloned());
            let new_cursor_rect = active_txs.first().and_then(|tx| tx.payload.cursor_rects().1.cloned());
            let has_active_txs = !active_txs.is_empty();

            let cursor_plan = self.animation_coordinator.build_cursor_plan(
                old_cursor_rect,
                new_cursor_rect,
                self.cursor_ctrl.visual_x,
                self.cursor_ctrl.visual_y,
                self.cursor_ctrl.visual_h,
                self.current_editor_enabled,
                self.buffer.has_selection(),
                self.current_viewport_height as f64,
                false,
                false,
                false,
                self.current_smooth_cursor_enabled,
                self.current_cursor_animation_duration_ms,
                self.current_coordinated_text_cursor_animation_enabled,
                self.current_scroll_y as f64,
                self.cursor_ctrl.last_scroll_y,
                self.cursor_ctrl.visible,
                self.cursor_ctrl.blink_visible,
                self.cursor_ctrl.visual_x,
                self.cursor_ctrl.visual_y,
                self.cursor_ctrl.force_snap_next,
                self.cursor_ctrl.animation.as_ref(),
            );
            let ime_plan = self.animation_coordinator.build_ime_plan(
                has_active_txs,
                false,
            );
            let selection_preedit = self.build_selection_preedit_plan();

            let frame_context = FrameContext {
                viewport_height: vp_h,
                scroll_offset_y: scroll_y,
                dpr,
                active_transaction_keys: Vec::new(),
                keys_to_complete: Vec::new(),
                keys_to_cancel: Vec::new(),
            };
            let cursor_style = CursorStyle {
                color: self.current_cursor_color.to_string(),
                width: 2.0,
            };

            let render_plan = self.animation_coordinator.build_render_plan_full(
                cursor_plan, ime_plan, selection_preedit,
                frame_context, cursor_style,
            );

            scene_graph_renderer::render_frame(
                final_root,
                item_ptr,
                &render_plan,
                &self.texture_cache,
            );

            let mut txs_to_mark_rendering: Vec<VisualTransactionKey> = Vec::new();
            for tx in active_txs {
                if tx.state == VisualTransactionState::Prepared {
                    txs_to_mark_rendering.push(tx.key);
                }
            }

            for key in &render_plan.frame_context.keys_to_complete {
                self.animation_coordinator.finish_by_key(*key);
                self.texture_cache.remove_for_transaction(key);
                editor_animation_debug_log(&format!(
                    "update_paint_node: VT tid={}, gen={} completed (progress >= 1.0)",
                    key.transaction_id, key.generation
                ));
            }

            for key in &render_plan.frame_context.keys_to_cancel {
                self.animation_coordinator.cancel_by_key(*key, "texture_failed");
                self.texture_cache.remove_for_transaction(key);
            }

            for key in &txs_to_mark_rendering {
                self.animation_coordinator.vt_queue.mark_rendering(*key);
                editor_animation_debug_log(&format!(
                    "update_paint_node: VT tid={} → Rendering", key.transaction_id
                ));
            }

            if !render_plan.frame_context.keys_to_complete.is_empty() {
                self.render_dirty = true;
            }

            if self.animation_coordinator.vt_queue.has_active()
                || !render_plan.frame_context.keys_to_complete.is_empty()
            {
                self.request_frame_update();
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
    pub(crate) fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        let dpr = {
            let item_ptr = self.get_cpp_object();
            if !item_ptr.is_null() {
                renderer::sujian_item_dpr(item_ptr)
            } else {
                1.0
            }
        };

        let font_size = self.current_font_pixel_size as f64;
        let font_family = self.current_font_family.to_string();

        let (runs, operation_kind, cache_keys) = {
            let tx = self.animation_coordinator.vt_queue.active_transactions()
                .iter()
                .find(|t| t.key == key);
            match tx {
                Some(t) => {
                    let runs: Vec<super::shaped_visual_run::ShapedVisualRun> = t.payload.shaped_runs_for_texture().into_iter().cloned().collect();
                    let op_kind = t.operation_kind;
                    let keys = t.payload.texture_cache_keys(key, op_kind);
                    (runs, op_kind, keys)
                }
                None => return,
            }
        };

        if runs.is_empty() {
            self.animation_coordinator.vt_queue.mark_texture_prepared(key);
            return;
        }

        let mut textures: Vec<Option<qmetaobject::QImage>> = Vec::with_capacity(runs.len());
        for run in &runs {
            textures.push(self.render_shaped_run_texture_via_glyph_run(run, font_size, &font_family, dpr));
        }

        let any_failed = textures.iter().any(|t| t.is_none());
        if any_failed {
            editor_animation_debug_log(&format!(
                "prepare_transaction_textures: texture failed for tid={}, cancelling",
                key.transaction_id
            ));
            self.animation_coordinator.cancel_by_key(key, "texture_failed");
            self.render_dirty = true;
        } else {
            let ready_textures: Vec<qmetaobject::QImage> = textures.into_iter().map(|t| t.unwrap()).collect();
            self.texture_cache.insert_batch(cache_keys, ready_textures);
            self.animation_coordinator.vt_queue.mark_texture_prepared(key);
        }
    }

    fn render_shaped_run_texture_via_glyph_run(
        &mut self,
        run: &super::shaped_visual_run::ShapedVisualRun,
        font_size: f64,
        _font_family: &str,
        dpr: f64,
    ) -> Option<qmetaobject::QImage> {
        let para_text = match &run.para_text {
            Some(t) if !t.is_empty() => t.clone(),
            _ => return None,
        };
        let qtextline_idx = run.qtextline_idx.unwrap_or(0);
        let wrap_w = run.paragraph_wrap_w.unwrap_or(99999.0);
        let indent_w = run.para_indent.unwrap_or(0.0);
        let run_family = run.raw_font_key.raw_font_family_parsed();

        crate::editor::layout::render_glyph_run_texture(
            &para_text,
            font_size,
            run_family,
            wrap_w,
            indent_w,
            qtextline_idx,
            run.qglyphrun_index,
            0.0,
            0.0,
            run.visual_w,
            run.visual_h,
            run.texture_translate_x,
            run.texture_translate_y,
            dpr,
            &self.current_text_color.to_string(),
        )
    }
}
