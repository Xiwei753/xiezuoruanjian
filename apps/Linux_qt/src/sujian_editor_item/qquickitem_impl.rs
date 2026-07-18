use super::input_host::is_left_button_pressed;
use super::*;
use super::transaction_key::VisualTransactionKey;

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
        self.clipboard_adapter.set_item_ptr(obj_ptr);
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
            let has_active_txs = !self.animation_coordinator.prepared_queue.is_empty();

            if !has_active_txs {
                self.pipeline.texture_cache_mut().clear();
                scene_graph::clear_animation_layer(final_root, item_ptr);
            }

            let old_cursor_rect = self.animation_coordinator.prepared_queue.active_transactions()
                .first()
                .and_then(|tx| tx.old_cursor_rect.clone());
            let new_cursor_rect = self.animation_coordinator.prepared_queue.active_transactions()
                .first()
                .and_then(|tx| tx.new_cursor_rect.clone());

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
                &self.pipeline.texture_cache(),
            );

            for key in &render_plan.frame_context.keys_to_complete {
                if let Some(ids) = self.animation_coordinator.finish_by_key(*key) {
                    self.pipeline.texture_cache_mut().remove_for_transaction(&ids);
                }
                editor_animation_debug_log(&format!(
                    "update_paint_node: tid={}, gen={} completed (progress >= 1.0)",
                    key.transaction_id, key.generation
                ));
            }

            for key in &render_plan.frame_context.keys_to_cancel {
                self.animation_coordinator.cancel_by_key(*key, "texture_failed");
            }

            if !render_plan.frame_context.keys_to_complete.is_empty() {
                self.render_dirty = true;
            }

            if self.animation_coordinator.has_prepared_or_rendering()
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

        // SAFETY: final_root was obtained from QSG node allocation in the same paint call; QQuickItem::updatePaintNode contract guarantees the node is valid.
        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}

impl SujianEditorItem {
    pub(crate) fn tick_text_animations(&mut self) {
        let now = Instant::now();
        self.animation_coordinator.tick(now);
    }
}
