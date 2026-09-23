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
        // Issue #668 评论 5646458592 问题 1: generation 生命周期对齐。
        // editor_layout.invalidate() 会 clear_layout_generation(current_generation)，
        // 释放该 generation 对应的 QTextLayout。如果 prepared_frame 仍然
        // 引用这个 generation，render thread 下一次 update_paint_node 用失效
        // generation 查找 layout 会全部缺失，rebuild 返回 false（虽然不会显示空
        // 节点，但会浪费一帧）。在 invalidate 时同时清除 prepared_frame，
        // 确保 render thread 不会消费已被释放的 generation。调用方随后会调
        // request_static_repaint() 重新 prepare 新 frame。
        // Issue #677 评论 5653944889: 字段从 cached_static_snapshot 改为 prepared_frame。
        self.prepared_frame = None;
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

    /// Issue #705: 为动画/IME 视觉提取分配独立的布局 generation。
    ///
    /// 动画/IME 路径需要独立 generation 写 QTextLayout cache,与静态正文
    /// render generation 互不干扰。**光标位置计算**(cursorToX/xToCursor/
    /// hit_test)已改为优先复用 `prepared_frame` 的 layout_snapshot(当前
    /// render generation),不再走此方法,避免"为了算光标位置再临时排一遍
    /// 文字"导致下一次事务起点与屏幕这一帧真正画出的光标位置不同。
    fn allocate_animation_layout_generation(&self) -> u64 {
        crate::editor::layout::begin_layout_generation()
    }

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
    ///
    /// Issue #738 评论 5797637204: 原 `build_editor_layout_snapshot` 只返回
    /// `EditorLayoutSnapshot`，内部构造的 `CanonicalDocumentVisualSnapshot` 被消耗，
    /// 调用方（composition commit 路径）拿不到新 canonical，无法走 canonical basis
    /// 闭环。抽共用 helper 同时返回 `(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)`，
    /// 让 composition commit 路径能把新 canonical 提交到 Pipeline.current_canonical_snapshot
    /// 并 reconcile 旧活动事务。原 `build_editor_layout_snapshot` 保留签名，内部调本 helper 取 `.0`。
    pub(crate) fn build_editor_layout_snapshot(
        &mut self,
        width: f64,
        promote: bool,
        composition_range: Option<(usize, usize)>,
    ) -> EditorLayoutSnapshot {
        self.build_editor_layout_snapshot_with_canonical(width, promote, composition_range)
            .0
    }

    /// Issue #738 评论 5797637204: 共用 helper，返回
    /// `(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)`。
    /// `EditorLayoutSnapshot` 供动画/纹理使用，`CanonicalDocumentVisualSnapshot` 供
    /// composition commit 路径提交到 `Pipeline.current_canonical_snapshot` 并作为
    /// `reconcile_active_transactions_with_canonical` 的新 canonical 几何。
    /// 一次排版同时产出两份视图，避免 composition commit 再单独排一次 canonical。
    pub(crate) fn build_editor_layout_snapshot_with_canonical(
        &mut self,
        width: f64,
        promote: bool,
        composition_range: Option<(usize, usize)>,
    ) -> (
        EditorLayoutSnapshot,
        crate::editor::layout::CanonicalDocumentVisualSnapshot,
    ) {
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
        // Issue #705: 光标位置计算已改用 prepared_frame 的 render generation;
        // 此处的独立 generation 仅供动画/IME 视觉提取使用。
        let generation = self.allocate_animation_layout_generation();

        // Issue #658 评论 5624570557 问题 3: 基础排版不生成全文动画 QImage，
        // 改为按 composition_range 提取相关行的动画视觉。
        let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));
        // Issue #688: 动画路径需要 text_color 用于 QImage 绘制
        let mut doc_snapshot = crate::editor::layout::prepare_document_visual_snapshot_scoped(
            &self.buffer.text,
            self.pipeline.text_revision(),
            font_size,
            font_family,
            line_spacing,
            padding,
            text_indent,
            width,
            dpr,
            Some(text_color),
            generation,
            affected_start,
            affected_end,
        );

        // Issue #658 评论 5625515748 问题 3: 从已有 prepared layout 提取受影响行的动画视觉
        // （QImage/glyph/cluster）并注入到 doc_snapshot，使 LineSnapshotBuilder 能消费。
        // 只有 composition_range 非空时才提取（None 表示全篇 fallback 语义，scoped 排版
        // 已对全篇 generate_animation_visuals=false，无动画视觉需提取）。
        // Issue #658 评论 5626002895 问题 2: 不再只按 composition range filter 行，
        // 改用 compare_old_new_visual_lines 比较 old/new lines 得到 affected line ids，
        // 再并入 composition range 直接覆盖的行，确保 IME commit 后 downstream reflow 行
        // 也提取动画视觉（handle_composition_commit_or_cancel 会遍历 candidate_byte_end
        // 之后的行做 reflow，这些行没有动画视觉会导致 source_rect 缺失甚至 texture_failed）。
        if affected_start < affected_end {
            let line_ids: Vec<usize> = {
                let old_lines_opt = self.editor_layout.cache().map(|c| &c.lines);
                let mut ids = if let Some(old_lines) = old_lines_opt {
                    let diff = crate::editor::layout::compare_old_new_visual_lines(
                        old_lines,
                        &doc_snapshot.visual_lines,
                        Some((affected_start, affected_end)),
                        None,
                    );
                    let mut new_ids = diff.new_raster_line_ids;
                    // Issue #658 评论 5626628570: 合并 reusable_move_pairs 的 new 索引，
                    // 这些行也需要动画视觉（image/clusters）以走 reflow_move 协同动画。
                    for &(_, new_idx) in &diff.reusable_move_pairs {
                        if !new_ids.contains(&new_idx) {
                            new_ids.push(new_idx);
                        }
                    }
                    new_ids
                } else {
                    Vec::new()
                };
                // 并入 composition range 直接覆盖的行
                for (i, l) in doc_snapshot.visual_lines.iter().enumerate() {
                    if l.byte_start < affected_end
                        && l.byte_end > affected_start
                        && !ids.contains(&i)
                    {
                        ids.push(i);
                    }
                }
                ids
            };
            if !line_ids.is_empty() {
                let handle = crate::editor::layout::PreparedLayoutHandle {
                    generation,
                    lines: &doc_snapshot.visual_lines,
                };
                let line_snapshots = crate::editor::layout::prepare_animation_visuals_from_layout(
                    &handle, &line_ids, dpr, text_color,
                );
                crate::editor::layout::inject_animation_visuals_into_snapshot(
                    &mut doc_snapshot,
                    line_snapshots,
                );
            }
        }

        let caret = doc_snapshot.cursor_rect(
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
            scroll_y,
            viewport_h,
        );
        // Issue #722 评论 5749791161: 同时生成文档坐标的 caret，供 VisualTransaction / caret track 使用。
        let caret_doc = doc_snapshot.cursor_rect_doc(self.buffer.cursor, self.cursor_ctrl.affinity);

        let mut snapshot =
            super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document(
                revision,
                &doc_snapshot,
                scroll_y,
                viewport_h,
                &self.buffer.text,
            );
        snapshot.caret_rect = Some(caret);
        snapshot.caret_rect_doc = Some(caret_doc);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;

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

        // Issue #738 评论 5797637204: 同时返回 doc_snapshot，供 composition commit 路径
        // 提交到 Pipeline.current_canonical_snapshot 并 reconcile 旧活动事务。
        // build_from_canonical_document 接收 &doc_snapshot（借用），此处 doc_snapshot 仍有效。
        (snapshot, doc_snapshot)
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
        // Issue #705: 光标位置计算已改用 prepared_frame 的 render generation;
        // 此处的独立 generation 仅供动画/IME 视觉提取使用。
        let generation = self.allocate_animation_layout_generation();

        // Issue #658 评论 5624570557 问题 3: 基础排版不生成全文动画 QImage，
        // 改为按 composition_range 提取相关行的动画视觉。
        let (affected_start, affected_end) = composition_range.unwrap_or((0, 0));
        // Issue #688: 动画路径需要 text_color 用于 QImage 绘制
        let mut doc_snapshot = crate::editor::layout::prepare_document_visual_snapshot_scoped(
            virtual_text,
            self.pipeline.text_revision(),
            font_size,
            font_family,
            line_spacing,
            padding,
            text_indent,
            width,
            dpr,
            Some(text_color),
            generation,
            affected_start,
            affected_end,
        );

        // Issue #658 评论 5625515748 问题 3: 从已有 prepared layout 提取受影响行的动画视觉
        // （QImage/glyph/cluster）并注入到 doc_snapshot，使 LineSnapshotBuilder 能消费。
        // 只有 composition_range 非空时才提取（None 表示全篇 fallback 语义）。
        // Issue #658 评论 5626002895 问题 2: 不再只按 composition range filter 行，
        // 改用 compare_old_new_visual_lines 比较 old/new lines 得到 affected line ids，
        // 再并入 composition range 直接覆盖的行，确保 IME preedit 后 downstream reflow 行
        // 也提取动画视觉（handle_composition_update 会遍历 composition_byte_end 之后的行
        // 做 reflow，这些行没有动画视觉会导致 source_rect 缺失甚至 texture_failed）。
        if affected_start < affected_end {
            let line_ids: Vec<usize> = {
                let old_lines_opt = self.editor_layout.cache().map(|c| &c.lines);
                let mut ids = if let Some(old_lines) = old_lines_opt {
                    let diff = crate::editor::layout::compare_old_new_visual_lines(
                        old_lines,
                        &doc_snapshot.visual_lines,
                        Some((affected_start, affected_end)),
                        None,
                    );
                    let mut new_ids = diff.new_raster_line_ids;
                    // Issue #658 评论 5626628570: 合并 reusable_move_pairs 的 new 索引，
                    // 这些行也需要动画视觉（image/clusters）以走 reflow_move 协同动画。
                    for &(_, new_idx) in &diff.reusable_move_pairs {
                        if !new_ids.contains(&new_idx) {
                            new_ids.push(new_idx);
                        }
                    }
                    new_ids
                } else {
                    Vec::new()
                };
                // 并入 composition range 直接覆盖的行
                for (i, l) in doc_snapshot.visual_lines.iter().enumerate() {
                    if l.byte_start < affected_end
                        && l.byte_end > affected_start
                        && !ids.contains(&i)
                    {
                        ids.push(i);
                    }
                }
                ids
            };
            if !line_ids.is_empty() {
                let handle = crate::editor::layout::PreparedLayoutHandle {
                    generation,
                    lines: &doc_snapshot.visual_lines,
                };
                let line_snapshots = crate::editor::layout::prepare_animation_visuals_from_layout(
                    &handle, &line_ids, dpr, text_color,
                );
                crate::editor::layout::inject_animation_visuals_into_snapshot(
                    &mut doc_snapshot,
                    line_snapshots,
                );
            }
        }

        let cursor_byte = if let Some(ref session) = self.pipeline.composition().composition_session
        {
            session.replace_start + session.preedit_cursor
        } else {
            self.buffer.cursor + virtual_text.len().saturating_sub(self.buffer.text.len())
        };
        let caret = doc_snapshot.cursor_rect(
            cursor_byte.min(virtual_text.len()),
            self.cursor_ctrl.affinity,
            scroll_y,
            viewport_h,
        );
        // Issue #722 评论 5749791161: 同时生成文档坐标的 caret，供 VisualTransaction / caret track 使用。
        let caret_doc = doc_snapshot.cursor_rect_doc(
            cursor_byte.min(virtual_text.len()),
            self.cursor_ctrl.affinity,
        );

        let mut snapshot =
            super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document(
                revision,
                &doc_snapshot,
                scroll_y,
                viewport_h,
                virtual_text,
            );
        snapshot.caret_rect = Some(caret);
        snapshot.caret_rect_doc = Some(caret_doc);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;

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

    /// Issue #727 评论 5757225958 问题1: canonical caret 文档坐标入口。
    ///
    /// 与原 `editor_layout_cursor_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标
    ///（不减 scroll_y），`visible` 始终为 true（文档坐标版本不关心视口可见性）。
    /// 供 `update_cursor_visual_position` 算 cursor_ctrl.target_y/visual_y 使用，
    /// 使 cursor_ctrl 内部状态统一保存文档坐标，和 scene graph cursor layer
    ///（QSGTransformNode 做 translate(0, -scroll_y)）的假设一致。
    /// QML/IME 边界方法（`cursor_rect_y` / `visual_cursor_rect_y` / `anchor_rect_y`）
    /// 在返回前减 `current_scroll_y` 转成视口坐标。
    pub(crate) fn editor_layout_cursor_rect_doc(
        &mut self,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> CursorLayoutRect {
        let snapshot = self.current_render_layout_snapshot();
        self.editor_layout
            .caret_rect_doc(&snapshot, cursor_byte, affinity)
    }

    pub(crate) fn hit_test(&mut self, x: f64, y: f64) -> (usize, CaretAffinity) {
        let scroll_y = f64::from(self.current_scroll_y);
        // Issue #705 评论 5716410988: 走统一入口,与 editor_layout_cursor_rect /
        // index_at_line_x / cursor_line_and_x 同一代 QTextLayout。
        let snapshot = self.current_render_layout_snapshot();
        let (index, affinity) = self.editor_layout.hit_test(&snapshot, x, y, scroll_y);
        editor_debug_log(&format!(
            "hit_test: mouse_x={:.1}, mouse_y={:.1}, current_scroll_y={:.1}, clamped_index={}, affinity={:?}",
            x, y, self.current_scroll_y, index, affinity
        ));
        (index, affinity)
    }

    pub(crate) fn index_at_line_x(&mut self, line: &VisualLine, x: f64) -> usize {
        // Issue #705 评论 5716410988: 走统一入口,与 editor_layout_cursor_rect /
        // hit_test / cursor_line_and_x 同一代 QTextLayout。
        let snapshot = self.current_render_layout_snapshot();
        self.editor_layout.index_at_line_x(&snapshot, line, x)
    }

    pub(crate) fn cursor_line_and_x(&mut self) -> Option<(usize, f64)> {
        // Issue #705 评论 5716410988: 走统一入口,与 editor_layout_cursor_rect /
        // hit_test / index_at_line_x 同一代 QTextLayout。
        // 去掉 lines 参数:snapshot 和 lines 同源,不需要外部传入做 debug_assert。
        let snapshot = self.current_render_layout_snapshot();
        self.editor_layout.cursor_line_and_x(
            &snapshot,
            self.buffer.cursor,
            self.cursor_ctrl.affinity,
        )
    }

    /// Issue #705 评论 5716410988: 光标正向/反向几何的唯一入口。
    ///
    /// 正常路径只返回 `prepared_frame.layout_snapshot`(当前 render generation),
    /// 保证 `editor_layout_cursor_rect` / `hit_test` / `index_at_line_x` /
    /// `cursor_line_and_x` 都基于同一代 QTextLayout。`prepared_frame` 不存在时
    /// (首帧、invalidate 后尚未 prepare) fallback 到 `layout_snapshot(width)`,
    /// 确保有可用几何,不会让光标位置计算走另一代 EditorLayout。
    ///
    /// 这样 `cursorToX`、`xToCursor`、正文静态绘制、上下方向键/跨行移动
    /// 都真的是同一代 QTextLayout。
    pub(crate) fn current_render_layout_snapshot(&mut self) -> LayoutSnapshot {
        let width = self.bounding_width();
        self.prepared_frame
            .as_ref()
            .map(|pf| pf.layout_snapshot.clone())
            .unwrap_or_else(|| self.layout_snapshot(width))
    }

    /// Issue #738 评论 5789470425 问题1: 构造当前排版参数的 VisualTransactionContext。
    /// 供 `reconcile_after_layout_change` 构造新 canonical snapshot 使用。
    fn build_visual_transaction_context(&self) -> super::pipeline::VisualTransactionContext {
        super::pipeline::VisualTransactionContext {
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
        }
    }

    /// Issue #738 评论 5789470425 问题1: 纯布局变化（resize/字号/字体/行距）后，
    /// 在**新排版已经按新 width/font/line_spacing/padding 算完之后**调此方法。
    /// 构造新 canonical snapshot，再通过 Pipeline 入口
    /// `reconcile_active_transactions_with_new_canonical` 把旧活动事务重绑到这份新 canonical，
    /// 并把它保存为当前 canonical。
    ///
    /// 调用方必须先 `invalidate_layout_cache` + `recalculate_content_height_and_emit`
    ///（确保新排版完成），再调此方法。
    pub(crate) fn reconcile_after_layout_change(&mut self) {
        let ctx = self.build_visual_transaction_context();
        // Issue #738 评论 5792244119 问题 1: 传入 &self.editor_layout，
        // build_canonical_snapshot_for_current_layout 复用当前 EditorLayout generation
        // 提取 active anchor cluster，不再分配临时 generation 泄漏。
        let new_snapshot = self
            .pipeline
            .build_canonical_snapshot_for_current_layout(&ctx, &self.editor_layout);
        self.pipeline
            .reconcile_active_transactions_with_new_canonical(new_snapshot);
    }
}
