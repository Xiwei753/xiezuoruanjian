use super::*;

// ── IME 输入处理模块 ──
//
// 将 Qt IME 事件翻译为 Core 编辑命令。核心交互流程：
// 1. QInputMethodEvent → input_set_preedit / input_set_preedit_with_attrs
// 2. Core CompositionSession 维护虚拟文本（committed + preedit）
// 3. IME commit → input_replace_and_insert 将 preedit 写入正文
//
// 坐标空间约定：
// - Core 层统一使用 UTF-8 byte offset
// - Qt IME 协议使用 UTF-16 code unit（QChar index）
// - 本模块在调用 Core 前完成坐标转换

pub(crate) fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
}

impl SujianEditorItem {
    /// 确保 composition session 存在。使用 `self.buffer.cursor` 而非
    /// `self.pipeline.cursor()`，因为 buffer 是当前已提交文本的光标位置，
    /// pipeline 可能包含未提交的 preedit 状态。
    fn ensure_composition_session(&mut self) {
        if self.pipeline.composition().composition_session.is_none() {
            let cursor = self.buffer.cursor;
            self.pipeline.composition_mut().composition_session = Some(CompositionSession::new(
                self.pipeline.text_revision(),
                self.pipeline.visual_revision(),
                self.buffer.text.clone(),
                cursor,
            ));
        }
    }

    pub(crate) fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        if let Some(ref session) = self.pipeline.composition().composition_session {
            session.preedit_byte_range_in_virtual_text()
        } else {
            (self.buffer.cursor, self.buffer.cursor)
        }
    }

    /// 准备 composition 更新数据。`cursor` 为 preedit 内部 UTF-8 byte offset，
    /// 指向 preedit 文本中的光标位置（非 committed 正文坐标）。
    fn prepare_composition_update(
        &mut self,
        text: String,
        cursor: usize,
    ) -> Option<CompositionUpdateData> {
        self.ensure_composition_session();

        let session = self
            .pipeline
            .composition_mut()
            .composition_session
            .as_mut()?;
        let old_preedit = session.preedit_text.clone();
        session.update_preedit(text, cursor);
        let generation = session.last_submitted_generation.value();
        let (composition_byte_start, composition_byte_end) =
            session.preedit_byte_range_in_virtual_text();
        let virtual_text = session.virtual_text();

        Some(CompositionUpdateData {
            old_preedit,
            generation,
            composition_byte_start,
            composition_byte_end,
            virtual_text,
        })
    }
}

/// Composition 更新数据，传递给动画协调器。
///
/// 坐标空间：
/// - `composition_byte_start`/`composition_byte_end`：virtualText UTF-8 byte offset（半开区间）
/// - `generation`：composition session 代数，用于过期检测
struct CompositionUpdateData {
    old_preedit: String,
    generation: u64,
    composition_byte_start: usize,
    composition_byte_end: usize,
    virtual_text: String,
}

impl EditorInputHost for SujianEditorItem {
    fn input_enabled(&self) -> bool {
        self.current_editor_enabled
    }

    fn input_emit_explicit_clear_requested(&mut self) {
        self.explicit_clear_requested();
    }

    fn input_clipboard_copy(&mut self) -> bool {
        self.clipboard_copy()
    }

    fn input_clipboard_paste(&mut self) {
        self.clipboard_paste();
    }

    fn input_undo(&mut self) {
        self.undo();
    }

    fn input_redo(&mut self) {
        self.redo();
    }

    fn input_select_all(&mut self) {
        self.select_all();
    }

    fn input_delete_selection(&mut self) {
        self.delete_selection();
    }

    fn input_delete_backward(&mut self) {
        self.delete_backward();
    }

    fn input_delete_forward(&mut self) {
        self.delete_forward();
    }

    fn input_insert_text(&mut self, text: String) {
        self.insert_text(text.into());
    }

    /// 替换指定范围并插入文本（IME commit 场景）。
    /// `replace_start`/`replace_length` 为 UTF-16 code unit 坐标（Qt IME 协议），
    /// 内部由 `ime_replace_and_insert` 转换为 UTF-8 byte offset 后调用 Core。
    fn input_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String) {
        self.ime_replace_and_insert(replace_start, replace_length, text);
    }

    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
        self.move_cursor_horizontal(forward, extend);
    }

    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool) {
        self.move_cursor_vertical(down, extend);
    }

    fn input_move_to_line_edge(&mut self, end: bool, extend: bool) {
        self.move_to_line_edge(end, extend);
    }

    /// 清除预输入文本。设计意图：
    /// 1. 保留 preedit 光标矩形供后续动画使用（pending_preedit_cursor_rect）
    /// 2. 若动画开启，构建新旧快照并触发 commit/cancel 动画过渡
    /// 3. 动画完成后由协调器自动清除 preedit 状态
    fn input_clear_preedit(&mut self) {
        if !self.pipeline.composition().preedit_text.is_empty()
            || self.pipeline.composition().composition_session.is_some()
        {
            self.pipeline.composition_mut().pending_preedit_cursor_rect =
                self.pipeline.composition().preedit_cursor_rect.clone();

            if self.typing_animation_enabled {
                let (composition_byte_start, composition_byte_end) =
                    self.preedit_byte_range_in_virtual_text();
                let width = self.bounding_width();
                let old_cursor_rect = self
                    .pipeline
                    .composition()
                    .preedit_cursor_rect
                    .as_ref()
                    .map(|c| CursorRect {
                        x: c.x,
                        top: c.top,
                        bottom: c.bottom,
                        baseline_y: c.baseline_y,
                    });
                let new_cursor_rect = self
                    .pipeline
                    .current_layout_snapshot()
                    .as_ref()
                    .and_then(|s| s.caret_rect.as_ref())
                    .map(|c| CursorRect {
                        x: c.x,
                        top: c.y,
                        bottom: c.y + c.h,
                        baseline_y: c.y + c.h * 0.8,
                    });

                let old_snapshot = self
                    .pipeline
                    .animation_coordinator()
                    .active_composition_new_snapshot()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.pipeline
                            .current_layout_snapshot()
                            .clone()
                            .unwrap_or_else(|| self.build_editor_layout_snapshot(width))
                    });
                let new_snapshot = self.build_editor_layout_snapshot(width);

                self.pipeline
                    .animation_coordinator_mut()
                    .cancel_active_composition("clear_preedit");
                self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_commit_or_cancel(
                        u64::from(self.current_typing_animation_duration_ms),
                        &old_snapshot,
                        &new_snapshot,
                        composition_byte_start,
                        composition_byte_end,
                        false,
                        false,
                        composition_byte_start,
                        composition_byte_start,
                        composition_byte_start,
                        composition_byte_start,
                        old_cursor_rect,
                        new_cursor_rect,
                    );
            }
        }
        self.pipeline.composition_mut().clear();
        self.update_ime_cursor_for_preedit();
    }

    /// 设置预输入文本。`cursor` 为 preedit 内部 UTF-8 byte offset，
    /// 指向 preedit 文本中的光标位置。动画开启时构建新旧快照并触发 composition update 动画。
    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.pipeline.composition_mut().preedit_old_text =
            self.pipeline.composition().preedit_text.clone();
        self.pipeline.composition_mut().preedit_text = text.clone();
        self.pipeline.composition_mut().preedit_cursor = cursor;
        self.pipeline.composition_mut().preedit_attributes.clear();

        if self.typing_animation_enabled && !text.is_empty() {
            if let Some(data) = self.prepare_composition_update(text, cursor) {
                let width = self.bounding_width();

                let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                    self.pipeline
                        .current_layout_snapshot()
                        .clone()
                        .unwrap_or_else(|| self.build_editor_layout_snapshot(width))
                } else {
                    self.pipeline
                        .animation_coordinator()
                        .active_composition_new_snapshot()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.pipeline
                                .current_layout_snapshot()
                                .clone()
                                .unwrap_or_else(|| self.build_editor_layout_snapshot(width))
                        })
                };

                let new_snapshot = self.build_virtual_layout_snapshot(&data.virtual_text, width);

                let old_cursor_rect = self
                    .pipeline
                    .current_layout_snapshot()
                    .as_ref()
                    .and_then(|s| s.caret_rect.as_ref())
                    .map(|c| CursorRect {
                        x: c.x,
                        top: c.y,
                        bottom: c.y + c.h,
                        baseline_y: c.y + c.h * 0.8,
                    });
                let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.y + c.h * 0.8,
                });

                self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_update(
                        u64::from(self.current_typing_animation_duration_ms),
                        &old_snapshot,
                        &new_snapshot,
                        data.composition_byte_start,
                        data.composition_byte_end,
                        old_cursor_rect,
                        new_cursor_rect,
                    );
            } else {
                self.update_preedit_visual_state();
            }
        } else {
            self.update_preedit_visual_state();
        }

        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    /// 设置预输入文本（带格式属性）。与 `input_set_preedit` 逻辑相同，
    /// 但额外保留 IME 格式属性（下划线、背景色等）供平台渲染 preedit 装饰。
    fn input_set_preedit_with_attrs(
        &mut self,
        text: String,
        cursor: usize,
        attributes: Vec<PreeditAttribute>,
    ) {
        self.pipeline.composition_mut().preedit_old_text =
            self.pipeline.composition().preedit_text.clone();
        self.pipeline.composition_mut().preedit_text = text.clone();
        self.pipeline.composition_mut().preedit_cursor = cursor;
        self.pipeline.composition_mut().preedit_attributes = attributes;

        if self.typing_animation_enabled && !text.is_empty() {
            if let Some(data) = self.prepare_composition_update(text, cursor) {
                let width = self.bounding_width();

                let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                    self.pipeline
                        .current_layout_snapshot()
                        .clone()
                        .unwrap_or_else(|| self.build_editor_layout_snapshot(width))
                } else {
                    self.pipeline
                        .animation_coordinator()
                        .active_composition_new_snapshot()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.pipeline
                                .current_layout_snapshot()
                                .clone()
                                .unwrap_or_else(|| self.build_editor_layout_snapshot(width))
                        })
                };

                let new_snapshot = self.build_virtual_layout_snapshot(&data.virtual_text, width);

                let old_cursor_rect = self
                    .pipeline
                    .current_layout_snapshot()
                    .as_ref()
                    .and_then(|s| s.caret_rect.as_ref())
                    .map(|c| CursorRect {
                        x: c.x,
                        top: c.y,
                        bottom: c.y + c.h,
                        baseline_y: c.y + c.h * 0.8,
                    });
                let new_cursor_rect = new_snapshot.caret_rect.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.y + c.h * 0.8,
                });

                self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_update(
                        u64::from(self.current_typing_animation_duration_ms),
                        &old_snapshot,
                        &new_snapshot,
                        data.composition_byte_start,
                        data.composition_byte_end,
                        old_cursor_rect,
                        new_cursor_rect,
                    );
            } else {
                self.update_preedit_visual_state();
            }
        } else {
            self.update_preedit_visual_state();
        }

        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    fn input_set_suppress_next_ime_commit(&mut self, value: bool) {
        self.pipeline.composition_mut().suppress_next_ime_commit = value;
    }

    fn input_take_suppress_next_ime_commit(&mut self) -> bool {
        let v = self.pipeline.composition().suppress_next_ime_commit;
        self.pipeline.composition_mut().suppress_next_ime_commit = false;
        v
    }

    fn input_request_repaint(&mut self) {
        self.request_static_repaint();
    }
}
