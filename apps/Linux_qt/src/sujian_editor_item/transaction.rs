use super::edit_motion::PreparedEditMotion;
use super::*;
use writer_core::editor::EditorEditResult;

impl SujianEditorItem {
    /// Issue #735: `record_transaction` 接收 `EditorEditResult` 而非从 `EditorEngine` 构造事务。
    ///
    /// Core 已删除 `EditorEngine`、`EditorVisualTransaction`。平台端从
    /// `EditorEditResult`（含 `cause`、`operation_kind`、`offset_map`、`content_delta`）
    /// 直接派生动画策略，不再经过 Core 的视觉事务工厂。
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        result: &EditorEditResult,
        emit: bool,
    ) -> Option<PreparedEditMotion> {
        let ctx = pipeline::VisualTransactionContext {
            typing_animation_enabled: self.current_typing_animation_enabled,
            smooth_cursor_enabled: self.current_smooth_cursor_enabled,
            is_scrolling: self.current_is_scrolling,
            is_loading: self.current_is_loading,
            is_applying_format: self.current_is_applying_format,
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

        // Issue #727 约束 5: smooth_cursor_enabled=false 自然意味着没有吞吐字。
        let mut motion: Option<PreparedEditMotion> = None;
        if self.current_typing_animation_enabled
            && self.current_smooth_cursor_enabled
            && !self.current_is_scrolling
        {
            motion = self.pipeline.prepare_edit_motion(
                &ctx,
                result,
                &old,
                &new,
                &self.editor_layout,
                self.cursor_ctrl.cursor_owner_epoch,
            );
        }
        // Issue #658 评论 5622188166 问题 1: 动画关闭/滚动抑制时不再走
        // fill_visual_transaction_coords_legacy 生成 old/new 动画坐标。
        // 该 legacy 路径通过 layout_snapshot_for_text 复用同一 EditorLayout，
        // 连续 snapshot 会互相清 generation，导致 caret_rect 取已失效的 generation
        // 返回 0.0，光标 x 塌缩到行首。删除 legacy 路径后，motion 的
        // old_cursor_rect/new_cursor_rect 保持 None（事务元数据仍保留，
        // 动画坐标不生成）。正常光标位置由 cursor controller / 当前正文 snapshot
        // 处理，不依赖 legacy 坐标。

        let has_motion = motion.is_some();
        self.last_event_count = if has_motion { 1 } else { 0 };
        self.last_summary = format!(
            "cause={:?};op={:?};motion={};delta_inserted={};delta_deleted={}",
            result.cause,
            result.operation_kind,
            has_motion,
            result.content_delta.inserted_chars,
            result.content_delta.deleted_chars,
        )
        .into();
        editor_animation_debug_log(&format!(
            "record_transaction: cause={:?}, op={:?}, motion={}, typing_anim_enabled={}, is_scrolling={}",
            result.cause,
            result.operation_kind,
            has_motion,
            self.current_typing_animation_enabled,
            self.current_is_scrolling,
        ));
        if emit {
            self.transaction_created();
        }
        motion
    }

    pub(crate) fn prepare_transaction_textures(&mut self, key: VisualTransactionKey) {
        self.pipeline.prepare_transaction_textures(key);
        // 纹理准备完成后，静态层裁剪区域变化，需要重建 Scene Graph。
        // 布局未变，不需要重新排版，只需要 scene rebuild。
        self.request_scene_rebuild();
    }
}
