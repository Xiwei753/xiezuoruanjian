use super::input_host::is_left_button_pressed;
use super::*;

use super::render_plan::{CursorStyle, FrameContext, SelectionPreeditStyle};
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
        // request_static_repaint 会在 GUI 线程预计算 snapshot
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
        mut node: qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode>,
    ) -> qmetaobject::scenegraph::SGNode<qmetaobject::scenegraph::ContainerNode> {
        let frame_start = Instant::now();

        let animation_set_changed = self.tick_text_animations();
        if animation_set_changed {
            self.scene_dirty = true;
        }

        let item_ptr = self.get_cpp_object();
        let dpr = if !item_ptr.is_null() {
            renderer::sujian_item_dpr(item_ptr)
        } else {
            1.0
        };

        let _vp_h = f64::from(self.current_viewport_height.max(1.0));
        let scroll_y = f64::from(self.current_scroll_y);
        let _content_h = f64::from(self.current_content_height);

        // Issue #658: 静态正文用 QSGTextNode（Qt 6.7+ 公开 API）。
        // needs_relayout 由 layout_dirty 控制：正文/字体/宽度变更时为 true，
        // 滚动时为 false（只更新位移矩阵，不重新排版）。
        // scene_dirty 为 true 时强制重建 Scene Graph 节点（如动画裁剪变化）。
        let needs_relayout = self.layout_dirty || self.scene_dirty;
        if self.layout_dirty {
            self.layout_dirty = false;
        }
        if self.scene_dirty {
            self.scene_dirty = false;
        }

        // Issue #677 评论 5653315696: 先取得或创建 editor root。
        // 第一帧旧节点为 null 时，ensure_editor_root 创建新的 QSGTransformNode 根节点，
        // 不再因为 root 为空就整帧跳过渲染。这把"根节点是否为空"的判断从
        // ensure_four_layer_nodes 收口到 ensure_editor_root。
        // Issue #677 评论 5653790560: 不再通过 into_raw / from_raw 做所有权往返，
        // 直接读写 node.raw（SGNode.raw 是 pub 字段）。第一帧 node.raw 为 null 时
        // ensure_editor_root 创建根节点并写回 node.raw；后续渲染统一用 node.raw。
        node.raw = scene_graph::ensure_editor_root(node.raw);
        let editor_root = node.raw;

        if !editor_root.is_null() && !item_ptr.is_null() {
            scene_graph::ensure_four_layer_nodes(editor_root, item_ptr);

            let has_active_txs = !self
                .pipeline
                .animation_coordinator_mut()
                .prepared_queue
                .is_empty();

            if !has_active_txs {
                self.pipeline.texture_cache_mut().clear();
                scene_graph::clear_animation_layer(editor_root, item_ptr);
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

            // Issue #677 评论 5653944889: render thread 只读 GUI 侧准备好的
            // `PreparedEditorFrame`，不再调用 `build_selection_preedit_plan()` /
            // `layout_snapshot()`，避免进入排版生命周期
            // （`EditorLayout::snapshot()` / `begin_layout_generation()` /
            // `clear_layout_generation()`）。
            // `prepared_frame = None` 时跳过静态正文渲染并请求下一次 GUI 帧准备。
            let prepared_frame = self.prepared_frame.as_ref();
            let selection_preedit = match prepared_frame {
                Some(frame) => frame.selection_preedit.clone(),
                None => animation_coordinator::SelectionPreeditPlan::default(),
            };

            let frame_context = FrameContext {
                active_transaction_keys: Vec::new(),
                keys_to_complete: Vec::new(),
                keys_to_cancel: Vec::new(),
            };
            let cursor_style = CursorStyle {
                color: self.current_cursor_color.to_string(),
                width: 2.0,
            };
            // Issue #677 评论 5654174714: selection/preedit 颜色是每帧轻量状态，
            // 不进入 PreparedEditorFrame；由 update_paint_node() 读取当前
            // current_selection_color 后构造 SelectionPreeditStyle 传给 RenderPlan。
            // preedit 的透明度（0x1A）由 renderer 在绘制时计算。
            let selection_preedit_style = SelectionPreeditStyle {
                selection_color: self.current_selection_color.to_string(),
            };

            let render_plan = self
                .pipeline
                .animation_coordinator_mut()
                .build_render_plan_full(
                    cursor_plan,
                    selection_preedit,
                    frame_context,
                    cursor_style,
                    selection_preedit_style,
                );

            // Issue #658: 静态正文层参数 — 读取 GUI 线程预计算的快照。
            // Issue #677 评论 5653944889: 快照和选区/preedit 几何都来自
            // `PreparedEditorFrame`，render thread 不再自行排版。
            // `prepared_frame = None` 时不排版，请求下一次 GUI 侧准备。
            let has_snapshot = prepared_frame.is_some();
            let static_text = StaticTextParams {
                layout_snapshot: prepared_frame.map(|f| &f.layout_snapshot),
                scroll_y,
                color: &self.current_text_color.to_string(),
                needs_relayout,
            };

            // Issue #668 评论 5646458592 问题 1: 接住静态正文 rebuild 的成功/失败结果。
            // rebuild 失败（某个必需 layout 缺失）时不能把本次静态正文更新当成已经完成；
            // 保留 layout_dirty / scene_dirty，下一次 update_paint_node 仍需要继续
            // 处理正确的 snapshot/generation。同时请求下一帧更新，让 GUI 线程
            // prepare_editor_frame 重新排版（snapshot() 会发现
            // cache 无效而分配新 generation 重新排版）。
            let static_rebuild_ok = scene_graph_renderer::render_frame(
                editor_root,
                item_ptr,
                &static_text,
                &render_plan,
                self.pipeline.texture_cache(),
            );
            if !static_rebuild_ok && needs_relayout {
                self.layout_dirty = true;
                self.scene_dirty = true;
                // Issue #677 评论 5653790560: render thread 不再反向排 GUI 线程补建。
                // snapshot/generation 生命周期在 GUI 侧一次收口：这里只保留当前静态
                // 正文节点并记录失败（layout_dirty / scene_dirty 置位），下一帧的
                // snapshot 准备由 GUI 侧的 invalidate_layout_cache() /
                // request_static_repaint() 在正常编辑路径中完成。删除
                // static_snapshot_reprepare_pending 标记和
                // schedule_static_snapshot_reprepare_on_gui_thread 调用，避免
                // render thread -> GUI thread queued 跨线程重排旧链。
            }

            // 没有准备好的 frame 时，跳过静态正文渲染并请求下一次 GUI 帧准备。
            // 放在 render_frame 之后，避免与 static_text 的不可变借用冲突。
            if !has_snapshot && needs_relayout {
                self.request_frame_update();
            }

            for key in &render_plan.frame_context.keys_to_complete {
                if let Some(ids) = self
                    .pipeline
                    .animation_coordinator_mut()
                    .finish_by_key(*key)
                {
                    self.pipeline
                        .texture_cache_mut()
                        .remove_for_transaction(&ids);
                    // Issue #658 评论 5630650436: GPU texture cache 现在由 AnimationLayerNode
                    // 自身持有，不再需要手动 release。sweep 在每帧 update_animation_layer 时运行。
                }
                editor_animation_debug_log(&format!(
                    "update_paint_node: tid={}, gen={} completed (progress >= 1.0)",
                    key.transaction_id, key.generation
                ));
            }

            let mut transaction_set_changed = false;
            for key in &render_plan.frame_context.keys_to_cancel {
                if self
                    .pipeline
                    .animation_coordinator_mut()
                    .cancel_by_key(*key, "texture_failed")
                {
                    transaction_set_changed = true;
                }
            }

            if !render_plan.frame_context.keys_to_complete.is_empty() || transaction_set_changed {
                self.scene_dirty = true;
            }

            if self
                .pipeline
                .animation_coordinator_mut()
                .has_prepared_or_rendering()
                || !render_plan.frame_context.keys_to_complete.is_empty()
                || transaction_set_changed
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

        // Issue #677 评论 5653790560: node.raw 已在函数开头被 ensure_editor_root 写回，
        // 直接返回 node，不再通过 from_raw(editor_root) 重建 wrapper。
        node
    }
}

impl SujianEditorItem {
    pub(crate) fn tick_text_animations(&mut self) -> bool {
        let now = Instant::now();
        self.pipeline.animation_coordinator_mut().tick(now)
    }
}
