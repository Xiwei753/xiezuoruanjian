use super::*;
use crate::editor::input::events::ImeReplaceEvent;

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

impl SujianEditorItem {
    /// 确保 composition session 存在。使用 `self.pipeline.cursor()` 读取
    /// 当前已提交文本的光标位置（CommittedTextMirror 只读投影）。
    ///
    /// Issue #701 评论 5702214893: 若开始 composition 时已有选区
    /// （`has_selection()`），用选区范围 `(start, end)` 作为 session 的
    /// replace range（`new_with_replace_range`），对应 Qt 官方
    /// `QInputMethodEvent` 语义"先删除当前 selection，再处理 replacement"。
    /// 无选区时退化为零长度插入 `(cursor, cursor)`（`new`）。
    ///
    /// Issue #826: session 只维护虚拟文本投影，供 `preedit_byte_range_in_virtual_text`
    /// 算出 preedit 在正文坐标系里的范围。preedit 是独立临时显示层。
    pub(crate) fn ensure_composition_session(&mut self) {
        if self.pipeline.composition().composition_session.is_none() {
            let cursor = self.pipeline.cursor();
            let text = self.pipeline.committed_text().to_string();
            let session = if self.pipeline.has_selection() {
                let (start, end) = self.pipeline.selection_range();
                super::edit_motion::CompositionSession::new_with_replace_range(text, start, end)
            } else {
                super::edit_motion::CompositionSession::new(text, cursor)
            };
            self.pipeline.composition_mut().composition_session = Some(session);
        }
    }

    pub(crate) fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        if let Some(ref session) = self.pipeline.composition().composition_session {
            session.preedit_byte_range_in_virtual_text()
        } else {
            (self.pipeline.cursor(), self.pipeline.cursor())
        }
    }

    /// Issue #701 评论 5699569220: 暴露 IME replacement 换算所需的 composition
    /// session 上下文，供 `platform_ime` 把 Qt 的 replacementStart/
    /// replacementLength（UTF-16 QChar 偏移）解析成 committed text 的 byte range。
    ///
    /// 返回 `(session_replace_start, session_replace_end, committed_text)`：
    /// - `session_replace_start`/`session_replace_end`：composition session 记录的
    ///   preedit 在 committed text 中的 byte range（半开区间，UTF-8）。
    ///   无活跃 session 时退化为 `(cursor, cursor)`。
    /// - `committed_text`：当前 committed 正文（= `self.pipeline.committed_text()`，不含 preedit）。
    ///
    /// 所有 UTF-16→UTF-8 坐标换算只在 `platform_ime` 调用此方法后做一次，
    /// `editing.rs` 不再二次换算。
    /// Issue #701 评论 5703179127: 暴露 IME replacement 换算所需的 composition
    /// session 上下文，供 `platform_ime` 把 Qt 的 replacementStart/
    /// replacementLength（UTF-16 QChar 偏移）解析成 committed text 的 byte range。
    ///
    /// 返回 `(session_replace_start, session_replace_end, committed_text)`：
    /// - `session_replace_start`/`session_replace_end`：composition session 记录的
    ///   preedit 在 committed text 中的 byte range（半开区间，UTF-8）。
    ///   有活跃 session 时用 session 的 replace range；
    ///   无 session 但有选区时直接返回 `pipeline.selection_range()`（Qt 规则：直接
    ///   commit 也应先删除当前 selection）；
    ///   无 session 无选区时退化为 `(cursor, cursor)`。
    /// - `committed_text`：当前 committed 正文（= `self.pipeline.committed_text()`，不含 preedit）。
    pub(crate) fn ime_replacement_context(&self) -> (usize, usize, String) {
        let (rs, re) = if self.pipeline.composition().composition_session.is_some() {
            self.pipeline
                .composition()
                .session_replace_range(self.pipeline.cursor())
        } else if self.pipeline.has_selection() {
            self.pipeline.selection_range()
        } else {
            (self.pipeline.cursor(), self.pipeline.cursor())
        };
        (rs, re, self.pipeline.committed_text().to_string())
    }
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

    /// IME commit/replace — 接收 Qt 两步语义的 `ImeReplaceEvent`。
    ///
    /// 事件由 `platform_ime` 结合当前 `CompositionSession` 把 Qt 的
    /// `replacementStart`/`replacementLength`（UTF-16 QChar 偏移）解析后构造。
    /// 进入此方法后不再携带任何 Qt 坐标，直接委托给 `ime_replace_and_insert`。
    fn input_ime_replace_and_commit(&mut self, event: ImeReplaceEvent) {
        self.ime_replace_and_insert(event);
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

    /// 清除预输入临时层，不改变正文过渡。
    fn input_clear_preedit(&mut self) {
        // 取消 preedit 只撤掉临时显示层；正文没有 Core edit，当前过渡继续独立运行。
        self.pipeline.composition_mut().clear();
        self.update_preedit_visual_state();
        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    /// Issue #704: 用户按 ESC 请求取消当前输入法组合态。
    ///
    /// 先判断当前是否存在活跃 composition / 非空 preedit（`is_composing()`）。
    /// 没有 composition 时直接 return，不调用 `input_clear_preedit()`，也不碰
    /// 已有 `suppress_next_ime_commit` guard。否则 `input_clear_preedit()` →
    /// `CompositionState::clear()` 会把等待迟到 commit 的 guard 清成 false，
    /// 导致迟到 commit 不再被抑制。
    /// 只有确实取消过真实 composition 时，才沿用 `input_clear_preedit` 撤掉 preedit，
    /// 并武装一次 `suppress_next_ime_commit`（用于忽略该
    /// composition 可能迟到的一次 commit）。
    fn input_cancel_preedit_for_escape(&mut self) {
        // Issue #704 评论 5711047799: 没有 composition 时直接 return，
        // 不调用 input_clear_preedit()，也不碰已有 guard。
        if !self.pipeline.composition().is_composing() {
            return;
        }
        // 确实存在活跃 composition：撤掉 preedit 临时层
        self.input_clear_preedit();
        // 取消过真实 composition 后武装一次 late-commit guard，
        // 用于忽略该 composition 可能迟到的一次 commit
        self.pipeline.composition_mut().suppress_next_ime_commit = true;
    }

    /// Issue #826: 设置预输入文本。`cursor` 为 preedit 内部 UTF-8 byte offset。
    ///
    /// preedit 是**独立临时显示层**：只更新投影 + 刷新 preedit 显示，
    /// 不创建正文动画，也不改变当前正文过渡。
    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.pipeline.composition_mut().preedit_old_text =
            self.pipeline.composition().preedit_text.clone();
        self.pipeline.composition_mut().preedit_text = text.clone();
        self.pipeline.composition_mut().preedit_cursor = cursor;
        self.pipeline.composition_mut().preedit_attributes.clear();

        // Issue #826: composition session 只维护虚拟文本投影，让
        // `preedit_byte_range_in_virtual_text()` 能算出 preedit 在正文坐标系里的范围
        // （IME commit 时要用）。preedit 本身是独立临时显示层，不参与动画。
        self.ensure_composition_session();
        if let Some(session) = self.pipeline.composition_mut().composition_session.as_mut() {
            session.update_preedit(&text);
        }

        self.pipeline
            .animation_coordinator_mut()
            .handle_composition_update();
        self.update_preedit_visual_state();
        self.update_ime_cursor_for_preedit();
        self.request_static_repaint();
    }

    /// Issue #826: 设置预输入文本（带格式属性）。与 `input_set_preedit` 同样只更新
    /// 临时显示层，额外保留 IME 格式属性（下划线、背景色等）供平台渲染 preedit 装饰。
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

        // Issue #826: 同 `input_set_preedit`，同步虚拟文本投影。
        self.ensure_composition_session();
        if let Some(session) = self.pipeline.composition_mut().composition_session.as_mut() {
            session.update_preedit(&text);
        }

        // Issue #658: 读取 preedit_attributes 字段用于 debug 日志，
        // 确保 IME 属性（start/length/kind）被实际消费而非 dead code。
        if std::env::var("SUJIAN_EDITOR_DEBUG").is_ok() {
            for attr in &self.pipeline.composition().preedit_attributes {
                eprintln!(
                    "[preedit_attr] start={}, length={}, kind={:?}",
                    attr.start, attr.length, attr.kind
                );
            }
        }

        self.pipeline
            .animation_coordinator_mut()
            .handle_composition_update();
        self.update_preedit_visual_state();
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
