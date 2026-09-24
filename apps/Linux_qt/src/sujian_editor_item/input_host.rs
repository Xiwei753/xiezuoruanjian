use super::animation::find_line_geometry_in_snapshot;
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

pub(crate) fn is_left_button_pressed(event: &QMouseEvent) -> bool {
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [event as "const QMouseEvent*"] -> bool as "bool" {
        return event ? (event->buttons() & Qt::LeftButton) : false;
    })
}

impl SujianEditorItem {
    /// 确保 composition session 存在。使用 `self.pipeline.cursor()` 读取
    /// 当前已提交文本的光标位置（CommittedTextMirror 只读投影）。
    ///
    /// Issue #701 评论 5702214893: 若开始 composition 时已有选区
    /// （`has_selection()`），用选区范围 `(start, end)` 作为 session 的
    /// replace range（`new_with_replace_range`），对应 Qt 官方
    /// `QInputMethodEvent` 语义"先删除当前 selection，再处理 replacement"。
    /// 无选区时退化为零长度插入 `(cursor, cursor)`（`new`）。
    fn ensure_composition_session(&mut self) {
        if self.pipeline.composition().composition_session.is_none() {
            let cursor = self.pipeline.cursor();
            let text_rev = self.pipeline.text_revision();
            let vis_rev = self.pipeline.visual_revision();
            let text = self.pipeline.committed_text().to_string();
            let session = if self.pipeline.has_selection() {
                let (start, end) = self.pipeline.selection_range();
                CompositionSession::new_with_replace_range(text_rev, vis_rev, text, start, end)
            } else {
                CompositionSession::new(text_rev, vis_rev, text, cursor)
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
        // 在 update_preedit 之前取 old virtualText 坐标系的 preedit range。
        let (old_preedit_byte_start, old_preedit_byte_end) =
            session.preedit_byte_range_in_virtual_text();
        session.update_preedit(&text, cursor);
        let generation = session.last_submitted_generation_value();
        // 更新之后取 new virtualText 坐标系的 preedit range。
        let (new_preedit_byte_start, new_preedit_byte_end) =
            session.preedit_byte_range_in_virtual_text();
        let virtual_text = session.virtual_text();

        Some(CompositionUpdateData {
            old_preedit,
            generation,
            old_preedit_byte_start,
            old_preedit_byte_end,
            new_preedit_byte_start,
            new_preedit_byte_end,
            virtual_text,
        })
    }
}

/// Composition 更新数据，传递给动画协调器。
///
/// 坐标空间：
/// - `old_preedit_byte_start`/`old_preedit_byte_end`：update_preedit 之前的 old
///   virtualText 坐标系 UTF-8 byte offset（半开区间）
/// - `new_preedit_byte_start`/`new_preedit_byte_end`：update_preedit 之后的 new
///   virtualText 坐标系 UTF-8 byte offset（半开区间）
/// - `generation`：composition session 代数，用于过期检测
struct CompositionUpdateData {
    old_preedit: String,
    generation: u64,
    old_preedit_byte_start: usize,
    old_preedit_byte_end: usize,
    new_preedit_byte_start: usize,
    new_preedit_byte_end: usize,
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

            // Issue #756 评论 5821042551: composition 动画进入条件 = coordinated || typing || smooth
            //（任意一种动画需要这笔 composition 事务时即进入）。
            if self.current_coordinated_animation_enabled
                || self.current_typing_animation_enabled
                || self.current_smooth_cursor_enabled
            {
                let (composition_byte_start, composition_byte_end) =
                    self.preedit_byte_range_in_virtual_text();
                // Issue #710 评论 5734666497: cancel 的 new-side 受影响范围是原 session replace range
                // （cancel 后 new = committed text，坐标一致），不是 old preedit range。
                // 在清 session 之前取，清完 session 就取不到了。
                let (committed_replace_start, committed_replace_end) = self
                    .pipeline
                    .composition()
                    .session_replace_range(self.pipeline.cursor());
                // Issue #710 评论 5735006606: snapshot 的视觉提取范围不能直接等于 raw edit range。
                // cancel 的 session_replace_range 可以是零长度 (cursor, cursor)（无 selection 的
                // 普通 composition ESC），零长度时 build_editor_layout_snapshot 的
                // `if affected_start < affected_end` 为 false，不生成任何动画视觉资源。
                // 用 compute_affected_paragraph_ranges 把 raw edit range 扩展到所在段落边界。
                // old 文本 = session 的 virtual_text（清 session 前取），new 文本 = committed text（cancel 恢复原文）。
                let committed_text = self.pipeline.committed_text().to_string();
                let old_virtual_text = self
                    .pipeline
                    .composition()
                    .composition_session
                    .as_ref()
                    .map(|s| s.virtual_text())
                    .unwrap_or_else(|| committed_text.clone());
                let (old_affected_start, old_affected_end, new_affected_start, new_affected_end) =
                    crate::editor::layout::compute_affected_paragraph_ranges(
                        &old_virtual_text,
                        &committed_text,
                        (composition_byte_start, composition_byte_end),
                        (committed_replace_start, committed_replace_end),
                    );
                let width = self.bounding_width();
                // Issue #722 评论 5749791161 问题2+3: IME cancel 路径使用文档坐标的
                // caret_rect_doc，不再用 viewport 坐标的 caret_rect/preedit_cursor_rect。
                // Issue #722 评论 5750218208 问题3: old/new caret 都从对应 snapshot 的
                // caret_rect_doc 取，不再统一从 current_layout_snapshot 取。

                let old_snapshot = self
                    .pipeline
                    .animation_coordinator()
                    .active_composition_new_snapshot()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.pipeline
                            .current_layout_snapshot()
                            .clone()
                            .unwrap_or_else(|| {
                                self.build_editor_layout_snapshot(
                                    width,
                                    false,
                                    Some((old_affected_start, old_affected_end)),
                                )
                            })
                    });
                let new_snapshot = self.build_editor_layout_snapshot(
                    width,
                    false,
                    Some((new_affected_start, new_affected_end)),
                );

                // Issue #722 评论 5750218208 问题3: old/new caret 都从对应 snapshot 的
                // caret_rect_doc 取，不再统一从 current_layout_snapshot 取——cancel 恢复
                // committed 布局后 caret 行/x/y 可能变化，old/new 必须分别反映
                // preedit 态和 committed 态的 caret 位置。
                let old_cursor_rect = old_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let old_cursor_visual_line_id = old_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);
                let new_cursor_rect = new_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let new_cursor_visual_line_id = new_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);

                // Issue #722 评论 5749791161: 从 snapshot 的 line_snapshots 中查找行几何。
                let (old_line_top, old_line_bottom) =
                    find_line_geometry_in_snapshot(&old_snapshot, old_cursor_visual_line_id);
                let (new_line_top, new_line_bottom) =
                    find_line_geometry_in_snapshot(&new_snapshot, new_cursor_visual_line_id);

                // Issue #756 评论 5821042551: 算出 text/caret/coordinated 三个开关传入 composition 路径。
                // text = coordinated || typing；caret = coordinated || smooth。
                let coordinated_anim = self.current_coordinated_animation_enabled;
                let text_anim = coordinated_anim || self.current_typing_animation_enabled;
                let caret_anim = coordinated_anim || self.current_smooth_cursor_enabled;
                self.pipeline
                    .animation_coordinator_mut()
                    .cancel_active_composition("clear_preedit");
                let layout_basis_revision = self.pipeline.layout_revision();
                self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_commit_or_cancel(
                        &old_snapshot,
                        &new_snapshot,
                        composition_byte_start,
                        composition_byte_end,
                        false,
                        false,
                        composition_byte_start,
                        composition_byte_start,
                        committed_replace_start,
                        committed_replace_end,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_line_top,
                        old_line_bottom,
                        new_line_top,
                        new_line_bottom,
                        self.cursor_ctrl.cursor_owner_epoch,
                        layout_basis_revision,
                        std::time::Instant::now(),
                        None,
                        text_anim,
                        caret_anim,
                        coordinated_anim,
                    );
            }
        }
        self.pipeline.composition_mut().clear();
        self.update_ime_cursor_for_preedit();
    }

    /// Issue #704: 用户按 ESC 请求取消当前输入法组合态。
    ///
    /// 先判断当前是否存在活跃 composition / 非空 preedit（`is_composing()`）。
    /// 没有 composition 时直接 return，不调用 `input_clear_preedit()`，也不碰
    /// 已有 `suppress_next_ime_commit` guard。否则 `input_clear_preedit()` →
    /// `CompositionState::clear()` 会把等待迟到 commit 的 guard 清成 false，
    /// 导致迟到 commit 不再被抑制。
    /// 只有确实取消过真实 composition 时，才沿用 `input_clear_preedit` 的动画
    /// 清理逻辑执行取消，并武装一次 `suppress_next_ime_commit`（用于忽略该
    /// composition 可能迟到的一次 commit）。
    fn input_cancel_preedit_for_escape(&mut self) {
        // Issue #704 评论 5711047799: 没有 composition 时直接 return，
        // 不调用 input_clear_preedit()，也不碰已有 guard。
        if !self.pipeline.composition().is_composing() {
            return;
        }
        // 确实存在活跃 composition：沿用现有动画清理逻辑执行取消
        self.input_clear_preedit();
        // 取消过真实 composition 后武装一次 late-commit guard，
        // 用于忽略该 composition 可能迟到的一次 commit
        self.pipeline.composition_mut().suppress_next_ime_commit = true;
    }

    /// 设置预输入文本。`cursor` 为 preedit 内部 UTF-8 byte offset，
    /// 指向 preedit 文本中的光标位置。动画开启时构建新旧快照并触发 composition update 动画。
    fn input_set_preedit(&mut self, text: String, cursor: usize) {
        self.pipeline.composition_mut().preedit_old_text =
            self.pipeline.composition().preedit_text.clone();
        self.pipeline.composition_mut().preedit_text = text.clone();
        self.pipeline.composition_mut().preedit_cursor = cursor;
        self.pipeline.composition_mut().preedit_attributes.clear();

        // Issue #756 评论 5821042551: composition 动画进入条件 = coordinated || typing || smooth
        //（任意一种动画需要这笔 composition 事务时即进入）。
        if (self.current_coordinated_animation_enabled
            || self.current_typing_animation_enabled
            || self.current_smooth_cursor_enabled)
            && !text.is_empty()
        {
            if let Some(data) = self.prepare_composition_update(text, cursor) {
                let width = self.bounding_width();
                // Issue #710 评论 5734282079: old/new preedit range 分属不同坐标系，
                // old_snapshot 用 old range，new_snapshot 用 new range。
                let old_composition_range =
                    Some((data.old_preedit_byte_start, data.old_preedit_byte_end));
                let new_composition_range =
                    Some((data.new_preedit_byte_start, data.new_preedit_byte_end));

                let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                    self.pipeline
                        .current_layout_snapshot()
                        .clone()
                        .unwrap_or_else(|| {
                            self.build_editor_layout_snapshot(width, false, old_composition_range)
                        })
                } else {
                    self.pipeline
                        .animation_coordinator()
                        .active_composition_new_snapshot()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.pipeline
                                .current_layout_snapshot()
                                .clone()
                                .unwrap_or_else(|| {
                                    self.build_editor_layout_snapshot(
                                        width,
                                        false,
                                        old_composition_range,
                                    )
                                })
                        })
                };

                let new_snapshot = self.build_virtual_layout_snapshot(
                    &data.virtual_text,
                    width,
                    new_composition_range,
                );

                // Issue #722 评论 5749791161 问题2+3: IME 路径使用文档坐标的 caret_rect_doc，
                // 不再用 viewport 坐标的 caret_rect（避免重复减 scroll_y）。
                // 同时传递真实 visual_line_id，不再传 None。
                // Issue #722 评论 5750218208 问题2: old caret 从 old_snapshot 取，
                // 不再从 current_layout_snapshot 取——old_snapshot 才是 preedit 态
                // 的布局快照，current_layout_snapshot 可能已是新布局。
                let old_cursor_rect = old_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let old_cursor_visual_line_id = old_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);
                let new_cursor_rect = new_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let new_cursor_visual_line_id = new_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);

                // Issue #722 评论 5749791161: 从 snapshot 的 line_snapshots 中查找行几何。
                let (old_line_top, old_line_bottom) =
                    find_line_geometry_in_snapshot(&old_snapshot, old_cursor_visual_line_id);
                let (new_line_top, new_line_bottom) =
                    find_line_geometry_in_snapshot(&new_snapshot, new_cursor_visual_line_id);

                // Issue #756 评论 5821042551: 算出 text/caret/coordinated 三个开关传入 composition 路径。
                // text = coordinated || typing；caret = coordinated || smooth。
                let coordinated_anim = self.current_coordinated_animation_enabled;
                let text_anim = coordinated_anim || self.current_typing_animation_enabled;
                let caret_anim = coordinated_anim || self.current_smooth_cursor_enabled;
                let layout_basis_revision = self.pipeline.layout_revision();
                let anim_key = self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_update(
                        &old_snapshot,
                        &new_snapshot,
                        data.old_preedit_byte_start,
                        data.old_preedit_byte_end,
                        data.new_preedit_byte_start,
                        data.new_preedit_byte_end,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_line_top,
                        old_line_bottom,
                        new_line_top,
                        new_line_bottom,
                        self.cursor_ctrl.cursor_owner_epoch,
                        layout_basis_revision,
                        text_anim,
                        caret_anim,
                        coordinated_anim,
                    );
                if anim_key.is_none() {
                    // Issue #756 评论 5822051193: coordinated 模式下无法建立 cursor track，
                    // 本轮不做动画，走静态 fallback，不影响 IME 正文/候选框正常显示。
                    self.update_preedit_visual_state();
                }
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

        // Issue #756 评论 5821042551: composition 动画进入条件 = coordinated || typing || smooth
        //（任意一种动画需要这笔 composition 事务时即进入）。
        if (self.current_coordinated_animation_enabled
            || self.current_typing_animation_enabled
            || self.current_smooth_cursor_enabled)
            && !text.is_empty()
        {
            if let Some(data) = self.prepare_composition_update(text, cursor) {
                let width = self.bounding_width();
                // Issue #710 评论 5734282079: old/new preedit range 分属不同坐标系，
                // old_snapshot 用 old range，new_snapshot 用 new range。
                let old_composition_range =
                    Some((data.old_preedit_byte_start, data.old_preedit_byte_end));
                let new_composition_range =
                    Some((data.new_preedit_byte_start, data.new_preedit_byte_end));

                let old_snapshot = if data.generation <= 1 || data.old_preedit.is_empty() {
                    self.pipeline
                        .current_layout_snapshot()
                        .clone()
                        .unwrap_or_else(|| {
                            self.build_editor_layout_snapshot(width, false, old_composition_range)
                        })
                } else {
                    self.pipeline
                        .animation_coordinator()
                        .active_composition_new_snapshot()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.pipeline
                                .current_layout_snapshot()
                                .clone()
                                .unwrap_or_else(|| {
                                    self.build_editor_layout_snapshot(
                                        width,
                                        false,
                                        old_composition_range,
                                    )
                                })
                        })
                };

                let new_snapshot = self.build_virtual_layout_snapshot(
                    &data.virtual_text,
                    width,
                    new_composition_range,
                );

                // Issue #722 评论 5749791161 问题2+3: IME 路径使用文档坐标的 caret_rect_doc，
                // 不再用 viewport 坐标的 caret_rect（避免重复减 scroll_y）。
                // 同时传递真实 visual_line_id，不再传 None。
                // Issue #722 评论 5750218208 问题2: old caret 从 old_snapshot 取，
                // 不再从 current_layout_snapshot 取——old_snapshot 才是 preedit 态
                // 的布局快照，current_layout_snapshot 可能已是新布局。
                let old_cursor_rect = old_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let old_cursor_visual_line_id = old_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);
                let new_cursor_rect = new_snapshot.caret_rect_doc.as_ref().map(|c| CursorRect {
                    x: c.x,
                    top: c.y,
                    bottom: c.y + c.h,
                    baseline_y: c.baseline_y,
                });
                let new_cursor_visual_line_id = new_snapshot
                    .caret_rect_doc
                    .as_ref()
                    .map(|c| c.visual_line_id);

                // Issue #722 评论 5749791161: 从 snapshot 的 line_snapshots 中查找行几何。
                let (old_line_top, old_line_bottom) =
                    find_line_geometry_in_snapshot(&old_snapshot, old_cursor_visual_line_id);
                let (new_line_top, new_line_bottom) =
                    find_line_geometry_in_snapshot(&new_snapshot, new_cursor_visual_line_id);

                // Issue #756 评论 5821042551: 算出 text/caret/coordinated 三个开关传入 composition 路径。
                // text = coordinated || typing；caret = coordinated || smooth。
                let coordinated_anim = self.current_coordinated_animation_enabled;
                let text_anim = coordinated_anim || self.current_typing_animation_enabled;
                let caret_anim = coordinated_anim || self.current_smooth_cursor_enabled;
                let layout_basis_revision = self.pipeline.layout_revision();
                let anim_key = self.pipeline
                    .animation_coordinator_mut()
                    .handle_composition_update(
                        &old_snapshot,
                        &new_snapshot,
                        data.old_preedit_byte_start,
                        data.old_preedit_byte_end,
                        data.new_preedit_byte_start,
                        data.new_preedit_byte_end,
                        old_cursor_rect,
                        new_cursor_rect,
                        old_cursor_visual_line_id,
                        new_cursor_visual_line_id,
                        old_line_top,
                        old_line_bottom,
                        new_line_top,
                        new_line_bottom,
                        self.cursor_ctrl.cursor_owner_epoch,
                        layout_basis_revision,
                        text_anim,
                        caret_anim,
                        coordinated_anim,
                    );
                if anim_key.is_none() {
                    // Issue #756 评论 5822051193: coordinated 模式下无法建立 cursor track，
                    // 本轮不做动画，走静态 fallback，不影响 IME 正文/候选框正常显示。
                    self.update_preedit_visual_state();
                }
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
