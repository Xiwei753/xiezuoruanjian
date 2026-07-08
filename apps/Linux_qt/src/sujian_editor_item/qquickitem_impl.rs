use super::input_host::is_left_button_pressed;
use super::*;

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
            scene_graph::ensure_single_image_node(root_raw, item_ptr);
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
