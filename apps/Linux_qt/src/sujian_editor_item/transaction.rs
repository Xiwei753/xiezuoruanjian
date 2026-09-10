use super::*;

impl SujianEditorItem {
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        cause: EditorTransactionCause,
        emit: bool,
    ) -> Option<EditorVisualTransaction> {
        let ctx = pipeline::VisualTransactionContext {
            typing_animation_enabled: self.current_typing_animation_enabled,
            is_scrolling: self.current_is_scrolling,
            is_loading: self.current_is_loading,
            is_applying_format: self.current_is_applying_format,
            is_applying_settings: self.current_is_applying_settings,
            bounding_width: self.bounding_width(),
            font_pixel_size: f64::from(self.current_font_pixel_size),
            font_family: self.current_font_family.to_string(),
            scroll_y: f64::from(self.current_scroll_y),
            viewport_height: f64::from(self.current_viewport_height.max(1.0)),
            text_indent: f64::from(self.current_text_indent),
            line_spacing: f64::from(self.current_line_spacing),
            padding: f64::from(self.current_padding),
            text_color: self.current_text_color.to_string(),
            dpr: {
                let item_ptr = self.get_cpp_object();
                if !item_ptr.is_null() {
                    crate::editor::renderer::sujian_item_dpr(item_ptr)
                } else {
                    1.0
                }
            },
        };

        let transaction = self.pipeline.engine().create_transaction(
            &old.text,
            &new.text,
            EditorSelection {
                anchor: EditorCursor::new(&old.text, old.selection_anchor),
                head: EditorCursor::new(&old.text, old.cursor),
            },
            EditorSelection {
                anchor: EditorCursor::new(&new.text, new.selection_anchor),
                head: EditorCursor::new(&new.text, new.cursor),
            },
            cause,
        );
        let mut vt = self.pipeline.engine_mut().visual_transaction(&transaction);

        if self.current_typing_animation_enabled && vt.is_some() && !self.current_is_scrolling {
            vt = self
                .pipeline
                .record_visual_transaction(&ctx, &old, &new, cause);
        }
        // Issue #658 评论 5622188166 问题 1: 动画关闭/滚动抑制时不再走
        // fill_visual_transaction_coords_legacy 生成 old/new 动画坐标。
        // 该 legacy 路径通过 layout_snapshot_for_text 复用同一 EditorLayout，
        // 连续 snapshot 会互相清 generation，导致 caret_rect 取已失效的 generation
        // 返回 0.0，光标 x 塌缩到行首。删除 legacy 路径后，vt 的
        // old_cursor_rect/new_cursor_rect 保持 None（事务元数据仍保留，
        // 动画坐标不生成）。正常光标位置由 cursor controller / 当前正文 snapshot
        // 处理，不依赖 legacy 坐标。

        self.last_event_count = if vt.is_some() { 1 } else { 0 };
        self.last_summary = format!(
            "cause={:?};changes={};vt={};animate={}",
            transaction.cause,
            transaction.changes.len(),
            vt.is_some(),
            transaction.should_animate
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_transaction: cause={:?}, changes={}, vt={}, animate={}, typing_anim_enabled={}, is_scrolling={}",
            transaction.cause,
            transaction.changes.len(),
            vt.is_some(),
            transaction.should_animate,
            self.current_typing_animation_enabled,
            self.current_is_scrolling,
        ));
        if emit {
            self.transaction_created();
        }
        vt
    }

    pub(crate) fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        self.pipeline.prepare_transaction_textures(key);
        // 纹理准备完成后，静态层裁剪区域变化，需要重建 Scene Graph。
        // 布局未变，不需要重新排版，只需要 scene rebuild。
        self.request_scene_rebuild();
    }
}
