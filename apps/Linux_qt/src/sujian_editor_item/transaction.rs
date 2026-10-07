use super::*;
use crate::sujian_editor_item::pipeline::VisualPrepareOutcome;
use writer_core::editor::EditorEditResult;

impl SujianEditorItem {
    /// Issue #735: `record_transaction` 接收 `EditorEditResult` 而非从 `EditorEngine` 构造事务。
    ///
    /// Core 已删除 `EditorEngine`、`EditorVisualTransaction`。平台端从
    /// `EditorEditResult`（含 `cause`、`operation_kind`、`offset_map`、`content_delta`）
    /// 直接派生动画策略，不再经过 Core 的视觉事务工厂。
    ///
    /// 返回 `prepare_edit_motion` 的明确结果，调用方不重新猜测动画跳过原因。
    pub(crate) fn record_transaction(
        &mut self,
        old: EditorSnapshot,
        new: EditorSnapshot,
        result: &EditorEditResult,
        emit: bool,
    ) -> VisualPrepareOutcome {
        let ctx = pipeline::VisualTransactionContext {
            typing_animation_enabled: self.current_typing_animation_enabled,
            smooth_cursor_enabled: self.current_smooth_cursor_enabled,
            coordinated_animation_enabled: self.current_coordinated_animation_enabled,
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

        // Issue #756: 删除"两个独立开关同时开启才走协同"的判断
        // （typing_animation_enabled && smooth_cursor_enabled）。
        // Issue #815 评论 6042062633 修改 8: 协同=一条 caret 运动轨迹 + 文字以 caret
        // 当前帧为吞吐边界；非协同时文字动画与光标动画互相独立。
        // 即 coordinated || typing || smooth 时才调用 prepare_edit_motion。
        let mut outcome: VisualPrepareOutcome = VisualPrepareOutcome::AnimationDisabled;
        let animations_requested = self.current_coordinated_animation_enabled
            || self.current_typing_animation_enabled
            || self.current_smooth_cursor_enabled;
        if animations_requested && self.current_is_scrolling {
            // Issue #815 评论 6042062633 修改 8: 滚动期间抑制动画是显式规则，但这是输入
            // 路径上"编辑发生了却完全没有动画"的第一个也是最常见的入口，必须记事件。
            editor_animation_transaction_skipped_event(&AnimationSkipFields {
                cause: "suppressed_by_scrolling",
                operation_kind: editor_operation_kind_label(result.operation_kind),
                typing_animation_enabled: self.current_typing_animation_enabled,
                smooth_cursor_enabled: self.current_smooth_cursor_enabled,
                coordinated_animation_enabled: self.current_coordinated_animation_enabled,
                old_caret_present: false,
                new_caret_present: false,
                inserted_range: None,
                unit_kinds: "",
                cursor_track_present: false,
                is_scrolling: true,
                is_loading: self.current_is_loading,
                is_applying_format: self.current_is_applying_format,
                transaction_id: None,
                generation: 0,
            });
            outcome = VisualPrepareOutcome::Skipped(
                super::edit_flow::EditVisualSkipReason::ScrollingSuppressed,
            );
            // 正文在滚动期间不创建动画。正文真的变化时丢弃旧过渡；否则滚动结束后
            // 旧 target 会继续在新正文上播放。光标/选区-only 保留暂停状态。
            // 光标/选区-only（正文没变）不动画，保留真 pause，恢复后继续。
            if old.text != new.text
                && self
                    .pipeline
                    .animation_coordinator_mut()
                    .finish_paused_text_animation_to_canonical()
            {
                // 收口后 active ids 为空 -> retain 把旧动画纹理全部释放，
                // 让 suppressed edit 的语义与「这一笔不动画」一致。
                let active_ids = self
                    .pipeline
                    .animation_coordinator_mut()
                    .collect_active_snapshot_ids();
                self.pipeline
                    .texture_cache_mut()
                    .retain_active_snapshot_ids(&active_ids);
            }
        } else if animations_requested {
            // Issue #819 评论 5968240881 问题 2：`prepare_edit_motion` 返回
            // `VisualPrepareOutcome`，直接透传，不再重新猜 skip reason。
            outcome =
                self.pipeline
                    .prepare_edit_motion(&ctx, result, &old, &new, &self.editor_layout);
            // Issue #815 评论 6042062633 修改 8: prepare_edit_motion 内部的每一个跳过点
            // （stale canonical / canonical invariant / caret 几何缺失 / 协同拿不到
            // cursor track / builder 空事务）都已经记过正式事件，这里不再重复报。
        }
        // Issue #658 评论 5622188166 问题 1: 动画关闭/滚动抑制时不再走
        // fill_visual_transaction_coords_legacy 生成 old/new 动画坐标。
        // 该 legacy 路径通过 layout_snapshot_for_text 复用同一 EditorLayout，
        // 连续 snapshot 会互相清 generation，导致 caret_rect 取已失效的 generation
        // 返回 0.0，光标 x 塌缩到行首。删除 legacy 路径后，motion 的
        // old_cursor_rect/new_cursor_rect 保持 None（事务元数据仍保留，
        // 动画坐标不生成）。正常光标位置由 cursor controller / 当前正文 snapshot
        // 处理，不依赖 legacy 坐标。

        let has_motion = matches!(outcome, VisualPrepareOutcome::Created { .. });
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
        outcome
    }

    /// 为当前视觉过渡准备行纹理，并请求 Scene Graph 处理 ownership 更新。
    pub(crate) fn prepare_visual_edit_textures(
        &mut self,
        base_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    ) {
        self.pipeline.prepare_visual_edit_textures(base_snapshot);
        self.request_scene_rebuild();
    }
}

/// Issue #815 评论 6042062633 修改 8: Core 编辑操作类型 -> 动画诊断里的 operation_kind 短名。
/// Issue #819 评论 5968931455 问题 3: 改成 pub(crate) 供 edit_flow.rs 复用。
pub(crate) fn editor_operation_kind_label(
    kind: writer_core::editor::EditorOperationKind,
) -> &'static str {
    match kind {
        writer_core::editor::EditorOperationKind::Insert => "Insert",
        writer_core::editor::EditorOperationKind::Delete => "Delete",
        writer_core::editor::EditorOperationKind::Replace => "Replace",
        writer_core::editor::EditorOperationKind::CursorOnly => "CursorOnly",
        writer_core::editor::EditorOperationKind::CompositionUpdate => "CompositionUpdate",
        writer_core::editor::EditorOperationKind::CompositionCommit => "CompositionCommit",
        writer_core::editor::EditorOperationKind::CompositionCancel => "CompositionCancel",
        writer_core::editor::EditorOperationKind::Load => "Load",
        writer_core::editor::EditorOperationKind::Format => "Format",
    }
}
