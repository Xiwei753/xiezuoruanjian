use super::layout_revision::LayoutRevision;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::*;

impl SujianEditorItem {
    pub(crate) fn recalculate_content_height_and_emit(&mut self) {
        let next = self.compute_content_height();
        if (self.current_content_height - next).abs() > 0.5 {
            self.current_content_height = next;
            self.content_height_dirty.set(false);
            self.content_height_changed();
        }
    }

    pub(crate) fn compute_content_height(&mut self) -> f32 {
        let width = self.bounding_width();
        let padding = self.current_padding;
        let font_size = self.current_font_pixel_size;
        let line_spacing = self.current_line_spacing;
        let lines = self.ensure_layout_cached(width);
        let height = lines
            .last()
            .map(|line| line.y + line.height + f64::from(padding))
            .unwrap_or(f64::from(font_size * line_spacing + padding * 2.0));
        height.max(1.0) as f32
    }

    pub(crate) fn invalidate_layout_cache(&mut self) {
        self.editor_layout.invalidate();
    }

    pub(crate) fn layout_params(&self, width: f64) -> LayoutParams {
        LayoutParams {
            width,
            font_size: self.current_font_pixel_size,
            font_family: self.current_font_family.to_string(),
            line_spacing: self.current_line_spacing,
            text_indent: self.current_text_indent,
            padding: self.current_padding,
        }
    }

    pub(crate) fn layout_snapshot(&mut self, width: f64) -> LayoutSnapshot {
        let params = self.layout_params(width);
        self.editor_layout
            .snapshot(&self.buffer.text, params, self.pipeline.text_revision())
            .clone()
    }

    // Issue #658 评论 5622188166 问题 1: 删除 layout_snapshot_for_text。
    // 该函数为 fill_visual_transaction_coords_legacy 专用，用同一 EditorLayout
    // 对任意 text 做 snapshot，连续调用会互相清 generation，导致光标 x 塌缩到行首。
    // legacy 路径已删除，正文/动画/IME 各自走独立 generation 的 canonical 排版入口。

    /// Issue #658 评论 5623746506 问题 2b: `promote=true` 时不再 clear 临时
    /// generation，而是构造 `PromotedLayout` 存入 `pipeline.pending_promoted_layout`，
    /// 由 `emit_content_changed` 提升为 `EditorLayout` current，避免
    /// `emit_content_changed -> ensure_layout_cached` 对同一已提交正文再次排版。
    /// `promote=false` 时保持原逻辑（临时 generation 用完即 clear），供 preedit
    /// virtual_text / old_snapshot fallback 等非 committed current 路径使用。
    ///
    /// Issue #658 评论 5624570557 问题 3: 增加 `composition_range` 参数，只对受影响范围
    /// 提取动画视觉。`None` 表示全篇（fallback 语义），`Some((start, end))` 表示只提取
    /// 与该 byte range 相交的行。
    pub(crate) fn build_editor_layout_snapshot(
        &mut self,
        width: f64,
        promote: bool,
        composition_range: Option<(usize, usize)>,
    ) -> EditorLayoutSnapshot {
        let scroll_y = f64::from(self.current_scroll_y);
        let viewport_h = f64::from(self.current_viewport_height.max(1.0));
        let font_size = f64::from(self.current_font_pixel_size);
        let font_family = &self.current_font_family.to_string();
        let text_indent = f64::from(self.current_text_indent);
        let line_spacing = f64::from(self.current_line_spacing);
        let padding = f64::from(self.current_padding);
        let dpr = {
            let item_ptr = self.get_cpp_object();
            if !item_ptr.is_null() {
                crate::editor::renderer::sujian_item_dpr(item_ptr)
            } else {
                1.0
            }
        };
        let text_color = &self.current_text_color.to_string();
        let revision = LayoutRevision::next();

        // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
        // 而是分配独立 generation，与静态正文路径互不干扰。
        let generation = crate::editor::layout::begin_layout_generation();

        // Issue #658 评论 5624570557 问题 3: 基础排版不生成全文动画 QImage，
        // 改为按 composition_range 提取相关行的动画视觉。
        let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));
        let doc_snapshot = crate::editor::layout::prepare_document_visual_snapshot_scoped(
            &self.buffer.text,
            self.pipeline.text_revision(),
            font_size,
            font_family,
            line_spacing,
            padding,
            text_indent,
            width,
            dpr,
            text_color,
            generation,
            affected_start,
            affected_end,
        );

        let caret = doc_snapshot.cursor_rect(
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
            scroll_y,
            viewport_h,
        );

        let mut snapshot =
            super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document(
                revision,
                &doc_snapshot,
                scroll_y,
                viewport_h,
            );
        snapshot.caret_rect = Some(caret);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;
        snapshot.virtual_text = self.buffer.text.clone();

        if promote {
            // Issue #658 评论 5623746506 问题 2b: composition commit 的 new text
            // generation 直接成为 current。构造 PromotedLayout 存入 pending，
            // emit_content_changed 取出提升为 EditorLayout current，后续
            // ensure_layout_cached cache hit 不再重新排版同一已提交正文。
            // Issue #658 评论 5624570557 问题 1: composition commit 路径没有 old prepared layout，
            // old_generation 设为 0。
            let promoted_visual_lines = doc_snapshot.visual_lines.clone();
            self.pipeline.set_pending_promoted_layout(Some(
                crate::editor::layout::PromotedLayout {
                    generation,
                    visual_lines: promoted_visual_lines,
                    width,
                    font_size: font_size as f32,
                    font_family: font_family.clone(),
                    line_spacing: line_spacing as f32,
                    text_indent: text_indent as f32,
                    padding: padding as f32,
                    old_generation: 0,
                },
            ));
        } else {
            // Issue #658 评论 5621512329 问题 1: 临时 generation 的 QTextLayout 已在
            // prepare_document_visual_snapshot 内部提取完 canonical line/image/cursor
            // 数据并存入 QImage（独立图像数据）。EditorLayoutSnapshot 不携带
            // layout_generation，不被 rebuild_text_node_from_paragraphs 消费，
            // 因此立即释放临时 generation，避免 layout 泄漏或被固定阈值误删。
            crate::editor::layout::clear_layout_generation(generation);
        }

        snapshot
    }

    /// Issue #658 评论 5624570557 问题 3: 增加 `composition_range` 参数，只对受影响范围
    /// 提取动画视觉。`None` 表示全篇（fallback 语义），`Some((start, end))` 表示只提取
    /// 与该 byte range 相交的行。virtual text 路径用临时 generation，用完即 clear。
    pub(crate) fn build_virtual_layout_snapshot(
        &mut self,
        virtual_text: &str,
        width: f64,
        composition_range: Option<(usize, usize)>,
    ) -> EditorLayoutSnapshot {
        let scroll_y = f64::from(self.current_scroll_y);
        let viewport_h = f64::from(self.current_viewport_height.max(1.0));
        let font_size = f64::from(self.current_font_pixel_size);
        let font_family = &self.current_font_family.to_string();
        let text_indent = f64::from(self.current_text_indent);
        let line_spacing = f64::from(self.current_line_spacing);
        let padding = f64::from(self.current_padding);
        let dpr = {
            let item_ptr = self.get_cpp_object();
            if !item_ptr.is_null() {
                crate::editor::renderer::sujian_item_dpr(item_ptr)
            } else {
                1.0
            }
        };
        let text_color = &self.current_text_color.to_string();
        let revision = LayoutRevision::next();

        // Issue #658 评论 5620035970 问题 2: 不再 clear_paragraph_layout_cache()，
        // 而是分配独立 generation，与静态正文路径互不干扰。
        let generation = crate::editor::layout::begin_layout_generation();

        // Issue #658 评论 5624570557 问题 3: 基础排版不生成全文动画 QImage，
        // 改为按 composition_range 提取相关行的动画视觉。
        let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));
        let doc_snapshot = crate::editor::layout::prepare_document_visual_snapshot_scoped(
            virtual_text,
            self.pipeline.text_revision(),
            font_size,
            font_family,
            line_spacing,
            padding,
            text_indent,
            width,
            dpr,
            text_color,
            generation,
            affected_start,
            affected_end,
        );

        let cursor_byte = if let Some(ref session) = self.pipeline.composition().composition_session
        {
            session.replace_start.value() + session.preedit_cursor_offset.value()
        } else {
            self.buffer.cursor + virtual_text.len().saturating_sub(self.buffer.text.len())
        };
        let caret = doc_snapshot.cursor_rect(
            cursor_byte.min(virtual_text.len()),
            self.cursor_ctrl.affinity,
            scroll_y,
            viewport_h,
        );

        let mut snapshot =
            super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document(
                revision,
                &doc_snapshot,
                scroll_y,
                viewport_h,
            );
        snapshot.caret_rect = Some(caret);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;
        snapshot.virtual_text = virtual_text.to_string();

        // Issue #658 评论 5621512329 问题 1: 临时 generation 的 QTextLayout 已在
        // prepare_document_visual_snapshot 内部提取完 canonical line/image/cursor
        // 数据并存入 QImage（独立图像数据）。EditorLayoutSnapshot 不携带
        // layout_generation，不被 rebuild_text_node_from_paragraphs 消费，
        // 因此立即释放临时 generation，避免 layout 泄漏或被固定阈值误删。
        crate::editor::layout::clear_layout_generation(generation);

        snapshot
    }

    pub(crate) fn ensure_layout_cached(&mut self, width: f64) -> &Vec<VisualLine> {
        let params = self.layout_params(width);
        &self
            .editor_layout
            .snapshot(&self.buffer.text, params, self.pipeline.text_revision())
            .lines
    }

    pub(crate) fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let lines = self.ensure_layout_cached(width).clone();
        let cursor = self.buffer.cursor;

        let is_wrap_boundary = lines.iter().enumerate().any(|(idx, line)| {
            idx + 1 < lines.len() && line.byte_end == cursor && lines[idx + 1].byte_start == cursor
        });

        if is_wrap_boundary {
            self.cursor_ctrl.affinity = CaretAffinity::Upstream;
        } else {
            self.cursor_ctrl.affinity = CaretAffinity::Downstream;
        }
    }

    pub(crate) fn editor_layout_cursor_rect(
        &mut self,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
    ) -> CursorLayoutRect {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        self.editor_layout.caret_rect(
            &snapshot,
            cursor_byte,
            affinity,
            scroll_y,
            f64::from(self.current_viewport_height.max(1.0)),
        )
    }

    pub(crate) fn hit_test(&mut self, x: f64, y: f64) -> (usize, CaretAffinity) {
        let width = self.bounding_width();
        let snapshot = self.layout_snapshot(width);
        let scroll_y = f64::from(self.current_scroll_y);
        let (index, affinity) = self.editor_layout.hit_test(&snapshot, x, y, scroll_y);
        editor_debug_log(&format!(
            "hit_test: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, clamped_index={}, affinity={:?}",
            x, y, self.current_scroll_y, index, affinity
        ));
        (index, affinity)
    }

    pub(crate) fn index_at_line_x(&self, line: &VisualLine, x: f64) -> usize {
        let Some(snapshot) = self.editor_layout.cache() else {
            return line.byte_start;
        };
        self.editor_layout.index_at_line_x(snapshot, line, x)
    }

    pub(crate) fn cursor_line_and_x(&self, lines: &[VisualLine]) -> Option<(usize, f64)> {
        let Some(snapshot) = self.editor_layout.cache() else {
            return None;
        };
        debug_assert_eq!(lines.len(), snapshot.lines.len());
        self.editor_layout.cursor_line_and_x(
            snapshot,
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
        )
    }
}
