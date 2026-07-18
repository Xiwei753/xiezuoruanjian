use super::*;

impl SujianEditorItem {
    pub(crate) fn bounding_width(&self) -> f64 {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return 800.0;
        }
        let item = self as &dyn QQuickItem;
        item.bounding_rect().width.max(1.0)
    }

    pub(crate) fn plain_text(&self) -> QString {
        self.buffer.text.clone().into()
    }

    pub(crate) fn set_plain_text(&mut self, text: QString) {
        self.set_plain_text_from_qml(text);
    }

    pub(crate) fn set_plain_text_from_qml(&mut self, text: QString) {
        let normalized = normalize_plain_text(&text.to_string());
        if self.buffer.text == normalized {
            return;
        }
        let old = self.buffer.snapshot();
        self.pipeline.load_text(normalized.clone(), 0);
        self.sync_buffer_from_pipeline();
        self.buffer.undo_stack.clear();
        self.buffer.redo_stack.clear();
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();
        self.previous_canonical_snapshot = None;
        self.record_transaction(old, new, EditorTransactionCause::Load, false);
        self.pipeline.composition_mut().clear();
        self.clear_active_text_animations();
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.emit_content_changed();
    }

    pub(crate) fn reload_plain_text(&mut self, text: QString) {
        let normalized = normalize_plain_text(&text.to_string());
        if self.buffer.text == normalized {
            return;
        }
        let old = self.buffer.snapshot();
        let old_cursor = self.buffer.cursor;
        let old_anchor = self.buffer.selection_anchor;
        self.pipeline.load_text(normalized.clone(), old_cursor);
        if self.pipeline.mirror().cursor() != clamp_to_char_boundary(&self.pipeline.mirror().text(), old_cursor) {
            let _ = self.pipeline.set_selection(
                clamp_to_char_boundary(self.pipeline.mirror().text(), old_anchor),
                clamp_to_char_boundary(self.pipeline.mirror().text(), old_cursor),
            );
        }
        self.sync_buffer_from_pipeline();
        self.adjust_affinity_at_wrap_boundary();
        let new = self.buffer.snapshot();
        self.previous_canonical_snapshot = None;
        self.record_transaction(old, new, EditorTransactionCause::Load, false);
        self.pipeline.composition_mut().clear();
        self.clear_active_text_animations();
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.emit_content_changed();
    }

    pub(crate) fn get_plain_text(&self) -> QString {
        self.buffer.text.clone().into()
    }

    pub(crate) fn content_height(&self) -> f32 {
        self.current_content_height
    }

    pub(crate) fn cursor_position(&self) -> u32 {
        byte_to_char_index(&self.buffer.text, self.buffer.cursor) as u32
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.buffer.has_selection()
    }

    pub(crate) fn editor_enabled(&self) -> bool {
        self.current_editor_enabled
    }

    pub(crate) fn set_editor_enabled(&mut self, value: bool) {
        if self.current_editor_enabled == value {
            return;
        }
        self.current_editor_enabled = value;
        self.editor_enabled_changed();
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn font_pixel_size(&self) -> f32 {
        self.current_font_pixel_size
    }

    pub(crate) fn set_font_pixel_size(&mut self, value: f32) {
        if (self.current_font_pixel_size - value).abs() <= 0.1 {
            return;
        }
        self.current_font_pixel_size = value.max(8.0);
        self.visual_changed();
    }

    pub(crate) fn font_family(&self) -> QString {
        self.current_font_family.clone()
    }

    pub(crate) fn set_font_family(&mut self, value: QString) {
        if self.current_font_family.to_string() == value.to_string() {
            return;
        }
        self.current_font_family = value;
        self.visual_changed();
    }

    pub(crate) fn line_spacing(&self) -> f32 {
        self.current_line_spacing
    }

    pub(crate) fn set_line_spacing(&mut self, value: f32) {
        if (self.current_line_spacing - value).abs() <= 0.01 {
            return;
        }
        self.current_line_spacing = value.max(1.0);
        self.visual_changed();
    }

    pub(crate) fn text_indent(&self) -> f32 {
        self.current_text_indent
    }

    pub(crate) fn set_text_indent(&mut self, value: f32) {
        if (self.current_text_indent - value).abs() <= 0.1 {
            return;
        }
        self.current_text_indent = value.max(0.0);
        self.visual_changed();
    }

    pub(crate) fn padding(&self) -> f32 {
        self.current_padding
    }

    pub(crate) fn set_padding(&mut self, value: f32) {
        if (self.current_padding - value).abs() <= 0.1 {
            return;
        }
        self.current_padding = value.max(0.0);
        self.visual_changed();
    }

    pub(crate) fn text_color(&self) -> QString {
        self.current_text_color.clone()
    }

    pub(crate) fn set_text_color(&mut self, value: QString) {
        let v = value.to_string();
        if self.current_text_color.to_string().eq_ignore_ascii_case(&v) {
            return;
        }
        editor_debug_log(&format!(
                "[editor] text_color changed old={} new={}",
                self.current_text_color, v
            ));
        self.current_text_color = value;
        self.visual_changed();
    }

    pub(crate) fn selection_color(&self) -> QString {
        self.current_selection_color.clone()
    }

    pub(crate) fn set_selection_color(&mut self, value: QString) {
        let v = value.to_string();
        if self
            .current_selection_color
            .to_string()
            .eq_ignore_ascii_case(&v)
        {
            return;
        }
        editor_debug_log(&format!(
                "[editor] selection_color changed old={} new={}",
                self.current_selection_color, v
            ));
        self.current_selection_color = value;
        self.visual_changed();
    }

    pub(crate) fn selected_text_color(&self) -> QString {
        self.current_selected_text_color.clone()
    }

    pub(crate) fn set_selected_text_color(&mut self, value: QString) {
        let v = value.to_string();
        if self
            .current_selected_text_color
            .to_string()
            .eq_ignore_ascii_case(&v)
        {
            return;
        }
        editor_debug_log(&format!(
                "[editor] selected_text_color changed old={} new={}",
                self.current_selected_text_color, v
            ));
        self.current_selected_text_color = value;
        self.visual_changed();
    }

    pub(crate) fn cursor_color(&self) -> QString {
        self.current_cursor_color.clone()
    }

    pub(crate) fn set_cursor_color(&mut self, value: QString) {
        let v = value.to_string();
        if self
            .current_cursor_color
            .to_string()
            .eq_ignore_ascii_case(&v)
        {
            return;
        }
        editor_debug_log(&format!(
                "[editor] cursor_color changed old={} new={}",
                self.current_cursor_color, v
            ));
        self.current_cursor_color = value;
        self.visual_changed();
    }

    pub(crate) fn smooth_cursor_enabled(&self) -> bool {
        self.current_smooth_cursor_enabled
    }

    pub(crate) fn set_smooth_cursor_enabled(&mut self, value: bool) {
        if self.current_smooth_cursor_enabled == value {
            return;
        }
        self.current_smooth_cursor_enabled = value;
        if !value {
            self.cursor_ctrl.animation = None;
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
        }
        self.visual_settings_changed();
    }

    pub(crate) fn cursor_animation_duration_ms(&self) -> u32 {
        self.current_cursor_animation_duration_ms
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, value: u32) {
        let clamped = value.max(30).min(1000);
        self.current_cursor_animation_duration_ms = clamped;
        self.visual_settings_changed();
    }

    pub(crate) fn typing_animation_enabled(&self) -> bool {
        self.current_typing_animation_enabled
    }

    pub(crate) fn set_typing_animation_enabled(&mut self, value: bool) {
        if self.current_typing_animation_enabled == value {
            return;
        }
        self.current_typing_animation_enabled = value;
        if !value {
            self.clear_active_text_animations();
            self.request_static_repaint();
        }
        editor_animation_debug_log(&format!("typing_animation_enabled_changed: {}", value));
        self.visual_settings_changed();
    }

    pub(crate) fn typing_animation_duration_ms(&self) -> u32 {
        self.current_typing_animation_duration_ms
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, value: u32) {
        let clamped = value.max(30).min(1000);
        if self.current_typing_animation_duration_ms == clamped {
            return;
        }
        self.current_typing_animation_duration_ms = clamped;
        self.pipeline.set_typing_animation_duration_ms(clamped);
        editor_animation_debug_log(&format!("typing_animation_duration_ms_changed: {}", clamped));
        self.visual_settings_changed();
    }

    pub(crate) fn coordinated_text_cursor_animation_enabled(&self) -> bool {
        self.current_coordinated_text_cursor_animation_enabled
    }

    pub(crate) fn set_coordinated_text_cursor_animation_enabled(&mut self, value: bool) {
        if self.current_coordinated_text_cursor_animation_enabled == value {
            return;
        }
        self.current_coordinated_text_cursor_animation_enabled = value;
        self.visual_settings_changed();
    }

    pub(crate) fn scroll_y(&self) -> f32 {
        self.current_scroll_y
    }

    pub(crate) fn set_scroll_y(&mut self, value: f32) {
        if (self.current_scroll_y - value).abs() < 0.5 {
            return;
        }
        self.current_scroll_y = value;
        self.clear_active_text_animations();
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn viewport_height(&self) -> f32 {
        self.current_viewport_height
    }

    pub(crate) fn set_viewport_height(&mut self, value: f32) {
        if (self.current_viewport_height - value).abs() < 0.5 {
            return;
        }
        self.current_viewport_height = value;
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn is_scrolling(&self) -> bool {
        self.current_is_scrolling
    }

    pub(crate) fn set_is_scrolling(&mut self, value: bool) {
        if self.current_is_scrolling == value {
            return;
        }
        self.current_is_scrolling = value;
        if value {
            self.pipeline.animation_coordinator_mut().pause_all();
            self.cursor_ctrl.animation = None;
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
            return;
        }
        if !value {
            self.pipeline.animation_coordinator_mut().resume_all();
            self.cursor_ctrl.force_snap_next = true;
            self.update_cursor_visual_position();
            self.request_static_repaint();
        }
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.current_is_loading
    }

    pub(crate) fn set_is_loading(&mut self, value: bool) {
        if self.current_is_loading == value {
            return;
        }
        self.current_is_loading = value;
        if value {
            self.clear_active_text_animations();
            self.cursor_ctrl.animation = None;
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
            return;
        }
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn is_applying_format(&self) -> bool {
        self.current_is_applying_format
    }

    pub(crate) fn set_is_applying_format(&mut self, value: bool) {
        if self.current_is_applying_format == value {
            return;
        }
        self.current_is_applying_format = value;
        if value {
            self.clear_active_text_animations();
            self.cursor_ctrl.animation = None;
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
            return;
        }
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn is_applying_settings(&self) -> bool {
        self.current_is_applying_settings
    }

    pub(crate) fn set_is_applying_settings(&mut self, value: bool) {
        if self.current_is_applying_settings == value {
            return;
        }
        self.current_is_applying_settings = value;
        if value {
            self.clear_active_text_animations();
            self.cursor_ctrl.animation = None;
            self.cursor_ctrl.force_snap_next = true;
            self.request_static_repaint();
            return;
        }
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }

    pub(crate) fn last_transaction_summary(&self) -> QString {
        self.last_summary.clone()
    }

    pub(crate) fn last_animation_event_count(&self) -> u32 {
        self.last_event_count
    }

    pub(crate) fn verify_animation_signal_meta_object(&self) -> bool {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return false;
        }
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        cpp!(unsafe [obj as "QObject*"] -> bool as "bool" {
            const QMetaObject* meta = obj->metaObject();
            return meta != nullptr;
        })
    }

    #[cfg(test)]
    pub(crate) fn debug_meta_object_animation_signals(&self) -> QString {
        let obj = self.get_cpp_object();
        if obj.is_null() {
            return "<null>".into();
        }
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        cpp!(unsafe [obj as "QObject*"] -> QString as "QString" {
            const QMetaObject* meta = obj->metaObject();
            QStringList signal_list;
            for (int i = 0; meta && i < meta->methodCount(); ++i) {
                QMetaMethod method = meta->method(i);
                if (method.methodType() != QMetaMethod::Signal) continue;
                const QByteArray sig = method.methodSignature();
                if (sig.contains("visual") || sig.contains("transaction") || sig.contains("explicit_clear")) {
                    signal_list << QString::fromLatin1(sig);
                }
            }
            return QStringLiteral("class=%1 signals=%2")
                .arg(meta ? QString::fromLatin1(meta->className()) : QStringLiteral("<null>"))
                .arg(signal_list.join(QStringLiteral(",")));
        })
    }

    pub(crate) fn cursor_rect_x(&self) -> f32 {
        if !self.pipeline.composition().preedit_text.is_empty() {
            if let Some(ref r) = self.pipeline.composition().preedit_cursor_rect {
                return r.x as f32;
            }
        }
        if self.current_smooth_cursor_enabled && self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.visual_x as f32
        } else {
            self.cursor_ctrl.target_x as f32
        }
    }

    pub(crate) fn cursor_rect_y(&self) -> f32 {
        if !self.pipeline.composition().preedit_text.is_empty() {
            if let Some(ref r) = self.pipeline.composition().preedit_cursor_rect {
                return r.top as f32;
            }
        }
        if self.current_smooth_cursor_enabled && self.cursor_ctrl.animation.is_some() {
            self.cursor_ctrl.visual_y as f32
        } else {
            self.cursor_ctrl.target_y as f32
        }
    }

    pub(crate) fn cursor_rect_width(&self) -> f32 {
        2.0
    }

    pub(crate) fn cursor_rect_height(&self) -> f32 {
        self.cursor_ctrl.ime_cursor_rect_h as f32
    }

    pub(crate) fn anchor_rect_x(&self) -> f32 {
        if !self.buffer.has_selection() {
            return self.cursor_rect_x();
        }
        self.cursor_ctrl.anchor_visual_x.unwrap_or(self.cursor_ctrl.target_x) as f32
    }

    pub(crate) fn anchor_rect_y(&self) -> f32 {
        if !self.buffer.has_selection() {
            return self.cursor_rect_y();
        }
        self.cursor_ctrl.anchor_visual_y.unwrap_or(self.cursor_ctrl.target_y) as f32
    }

    pub(crate) fn anchor_rect_width(&self) -> f32 {
        2.0
    }

    pub(crate) fn anchor_rect_height(&self) -> f32 {
        self.cursor_ctrl.ime_cursor_rect_h as f32
    }

    pub(crate) fn anchor_position(&self) -> u32 {
        byte_to_char_index(&self.buffer.text, self.buffer.selection_anchor) as u32
    }

    pub(crate) fn cursor_visible(&self) -> bool {
        self.cursor_ctrl.visible
    }

    pub(crate) fn cursor_blink_visible(&self) -> bool {
        if !self.cursor_ctrl.visible {
            return false;
        }
        self.cursor_ctrl.blink_visible
    }

    pub(crate) fn cursor_should_be_visible(&self) -> bool {
        self.cursor_ctrl.cursor_should_be_visible()
    }

    pub(crate) fn cursor_blink_opacity(&self) -> f32 {
        use animation_coordinator::CursorBlinkMode;
        let blink_mode = if self.current_coordinated_text_cursor_animation_enabled
            && self.pipeline.animation_coordinator().has_active_insert()
        {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };
        self.cursor_ctrl.cursor_blink_opacity(blink_mode) as f32
    }

    pub(crate) fn current_selection_text(&self) -> QString {
        self.buffer.selected_text().into()
    }

    pub(crate) fn request_text_input_focus(&mut self) {
        let obj_ptr = self.get_cpp_object();
        if obj_ptr.is_null() {
            return;
        }
        input::focus_item(obj_ptr);
    }

    pub(crate) fn snap_next_cursor_update(&mut self) {
        self.cursor_ctrl.force_snap_next = true;
        self.update_cursor_visual_position();
        self.request_frame_update();
    }

    pub(crate) fn visual_changed(&mut self) {
        self.invalidate_layout_cache();
        self.bump_visual_revision();
        self.clear_active_text_animations();
        self.previous_canonical_snapshot = None;
        self.cursor_ctrl.animation = None;
        self.cursor_ctrl.force_snap_next = true;
        self.recalculate_content_height_and_emit();
        self.visual_settings_changed();
    }

    pub(crate) fn emit_content_changed(&mut self) {
        if self.cursor_ctrl.force_snap_next {
            self.cursor_ctrl.animation = None;
        }
        self.pipeline.bump_text_revision();
        self.recalculate_content_height_and_emit();
        self.plain_text_changed();
        self.text_changed();
        self.cursor_position_changed();
        self.selection_changed();
        self.update_cursor_visual_position();
        self.request_static_repaint();
    }
}
