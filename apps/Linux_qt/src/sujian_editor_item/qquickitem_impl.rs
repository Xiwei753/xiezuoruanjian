use super::input_host::is_left_button_pressed;
use super::*;

use super::render_plan::{CursorStyle, FrameContext};
use super::scene_graph_renderer::StaticTextParams;
use std::time::Instant;

impl QQuickItem for SujianEditorItem {
    fn component_complete(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        let item_ptr = self as *mut Self as *mut std::ffi::c_void;
        input::install_event_filter(obj_ptr, item_ptr);
        self.pipeline.clipboard_adapter_mut().set_item_ptr(obj_ptr);
    }

    fn geometry_changed(&mut self, _new_geometry: QRectF, _old_geometry: QRectF) {
        // 宽度变化需要重新排版 QSGTextNode
        self.invalidate_layout_cache();
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

        let _vp_h = f64::from(self.current_viewport_height.max(1.0));
        let scroll_y = f64::from(self.current_scroll_y);
        let _content_h = f64::from(self.current_content_height);

        // Issue #658: 静态正文用 QSGTextNode（Qt 6.7+ 公开 API）。
        // needs_relayout 由 render_dirty 判断：正文/字体/宽度变更时为 true，
        // 滚动时为 false（只更新位移矩阵，不重新排版）。
        let needs_relayout = self.render_dirty;
        if needs_relayout {
            self.render_dirty = false;
        }

        let final_root = root_raw;

        if !final_root.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_four_layer_nodes(final_root, item_ptr);

            let has_active_txs = !self
                .pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .is_empty();

            if !has_active_txs {
                self.pipeline.texture_cache_mut().clear();
                scene_graph::clear_animation_layer(final_root, item_ptr);
            }

            let old_cursor_rect = self
                .pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .active_transactions()
                .first()
                .and_then(|tx| tx.old_cursor_rect.clone());
            let new_cursor_rect = self
                .pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .active_transactions()
                .first()
                .and_then(|tx| tx.new_cursor_rect.clone());

            let cursor_plan = self.pipeline.animation_coordinator_mut().build_cursor_plan(
                old_cursor_rect,
                new_cursor_rect,
                self.cursor_ctrl.visual_x,
                self.cursor_ctrl.visual_y,
                self.cursor_ctrl.visual_h,
                self.current_editor_enabled,
                self.buffer.has_selection(),
                f64::from(self.current_viewport_height),
                false,
                false,
                false,
                self.current_smooth_cursor_enabled,
                self.current_cursor_animation_duration_ms,
                self.current_coordinated_text_cursor_animation_enabled,
                f64::from(self.current_scroll_y),
                self.cursor_ctrl.last_scroll_y,
                self.cursor_ctrl.visible,
                self.cursor_ctrl.blink_visible,
                self.cursor_ctrl.visual_x,
                self.cursor_ctrl.visual_y,
                self.cursor_ctrl.force_snap_next,
                self.cursor_ctrl.animation.as_ref(),
            );
            let selection_preedit = self.build_selection_preedit_plan();

            let frame_context = FrameContext {
                active_transaction_keys: Vec::new(),
                keys_to_complete: Vec::new(),
                keys_to_cancel: Vec::new(),
            };
            let cursor_style = CursorStyle {
                color: self.current_cursor_color.to_string(),
                width: 2.0,
            };

            let render_plan = self
                .pipeline
                .animation_coordinator_mut()
                .build_render_plan_full(
                    cursor_plan,
                    selection_preedit,
                    frame_context,
                    cursor_style,
                );

            // 静态正文层参数 — 交给 QSGTextNode
            // 注意 borrow 顺序：layout_snapshot() 需要 &mut self（计算后即释放），
            // 之后再取 &self 的不可变引用给 render_frame。
            let width = self.bounding_width();
            let snapshot = self.layout_snapshot(width);
            let static_text = StaticTextParams {
                layout_snapshot: Some(&snapshot),
                scroll_y,
                color: &self.current_text_color.to_string(),
                needs_relayout,
            };

            scene_graph_renderer::render_frame(
                final_root,
                item_ptr,
                &static_text,
                &render_plan,
                self.pipeline.texture_cache(),
            );

            for key in &render_plan.frame_context.keys_to_complete {
                if let Some(ids) = self
                    .pipeline
                    .animation_coordinator_mut()
                    .finish_by_key(*key)
                {
                    self.pipeline
                        .texture_cache_mut()
                        .remove_for_transaction(&ids);
                }
                editor_animation_debug_log(&format!(
                    "update_paint_node: tid={}, gen={} completed (progress >= 1.0)",
                    key.transaction_id, key.generation
                ));
            }

            for key in &render_plan.frame_context.keys_to_cancel {
                self.pipeline
                    .animation_coordinator_mut()
                    .cancel_by_key(*key, "texture_failed");
            }

            if !render_plan.frame_context.keys_to_complete.is_empty() {
                self.render_dirty = true;
            }

            if self
                .pipeline
                .animation_coordinator_mut()
                .has_prepared_or_rendering()
                || !render_plan.frame_context.keys_to_complete.is_empty()
            {
                self.request_frame_update();
            }
        }

        let total_elapsed = frame_start.elapsed();
        if total_elapsed.as_millis() > 4 {
            editor_debug_log(&format!(
                "sujian_update_paint_node: total_ms={}, needs_relayout={}, dpr={:.2}",
                total_elapsed.as_millis(),
                needs_relayout,
                dpr,
            ));
        }

        // SAFETY: final_root was obtained from QSG node allocation in the same paint call; QQuickItem::updatePaintNode contract guarantees the node is valid.
        unsafe { SGNode::<qmetaobject::scenegraph::ContainerNode>::from_raw(final_root) }
    }
}

impl SujianEditorItem {
    pub(crate) fn tick_text_animations(&mut self) {
        let now = Instant::now();
        self.pipeline.animation_coordinator_mut().tick(now);
    }
}
