use super::layout_revision::LayoutRevision;
use super::layout_snapshot::EditorLayoutSnapshot;
use super::*;
// Issue #901 第九轮阻塞5: Arc 共享视觉行数据。
use std::sync::Arc;

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
        // Issue #901 第十五轮问题5b: 用 resolve_line 只物化最后一行，
        // 不物化整章。内容高度 = 最后一行底 y + padding。
        let params = self.layout_params(width);
        let snapshot = self.editor_layout.snapshot(
            self.pipeline.committed_text(),
            params,
            self.pipeline.text_revision(),
        );
        let total_lines = snapshot.lines.len();
        let height = if total_lines > 0 {
            let last_line = snapshot.resolve_line(total_lines - 1);
            last_line.y + last_line.height + f64::from(padding)
        } else {
            f64::from(font_size * line_spacing + padding * 2.0)
        };
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
            .snapshot(
                self.pipeline.committed_text(),
                params,
                self.pipeline.text_revision(),
            )
            .clone()
    }

    /// Issue #901 问题4: 增量排版入口 — 普通编辑走此路径，不全章重排。
    ///
    /// 与 [`build_editor_layout_snapshot`] 的区别：
    /// - 不调用 `prepare_document_visual_snapshot_scoped`（全篇排版），
    ///   而是走 `EditorLayout::snapshot()` 增量路径，只重排受影响段落。
    /// - 从 `EditorLayout::last_canonical_snapshot()` 获取增量排版结果，
    ///   构建 `EditorLayoutSnapshot`。
    /// - 为受影响行提取 QImage 纹理并填充到 `PreparedLineSnapshot.image`，
    ///   使 `prepare_visual_edit_textures` 能注入 texture_cache，
    ///   闭环"普通输入也产生纹理"。
    /// - 不分配独立 animation generation，复用 `EditorLayout` 的 current generation，
    ///   避免为准备动画 target 再全章排版一次。
    ///
    /// `affected_range` 是 Core `display_patches` 提供的受影响字节范围。
    /// `None` 表示无明确范围（fallback 到全篇重排）。
    ///
    /// Issue #904 评论 5: 返回值额外携带本版布局的 `LocalLayoutSource`
    ///（generation + 物化视觉行），供局部动画只在**同一版**布局上补图，
    /// 绝不用当前 live cache 的另一版正文冒充旧快照行。
    pub(crate) fn build_incremental_layout_snapshot(
        &mut self,
        width: f64,
        affected_range: Option<(usize, usize)>,
    ) -> Result<
        (
            EditorLayoutSnapshot,
            Option<super::pipeline::LocalLayoutSource>,
        ),
        String,
    > {
        // 1. 设置受影响范围，让 EditorLayout::snapshot() 增量路径能定位受影响段落。
        if let Some((start, end)) = affected_range {
            self.editor_layout.set_last_edit_affected_range(start, end);
        }

        // 2. 走增量排版。EditorLayout::snapshot() 内部判断：
        //    - 排版参数变化 → 全局失效 + 全篇重排
        //    - 仅 text_revision 变化 → 增量排版（只重排受影响段落）
        //    - 都没变 → cache hit，直接返回
        // 用 block 限定 &mut borrow，释放后才能调 last_canonical_snapshot()。
        let generation = {
            let params = self.layout_params(width);
            let layout_snap = self.editor_layout.snapshot(
                self.pipeline.committed_text(),
                params,
                self.pipeline.text_revision(),
            );
            layout_snap.layout_generation
        };

        // 3. 从 last_snapshot 获取增量排版产生的 CanonicalDocumentVisualSnapshot。
        let doc_snapshot = match self.editor_layout.last_canonical_snapshot() {
            Some(s) => s,
            None => {
                return Err(
                    "build_incremental_layout_snapshot: no canonical snapshot after layout"
                        .to_string(),
                );
            }
        };

        let scroll_y = f64::from(self.current_scroll_y);
        let viewport_h = f64::from(self.current_viewport_height.max(1.0));
        let revision = LayoutRevision::next();

        // 4. 构建 EditorLayoutSnapshot（此时 image 可能为 None，下面填充）。
        // Issue #901 第六轮阻塞5: 改用 build_from_canonical_document_with_height_index，
        // 通过 ParagraphHeightIndex O(log n) 定位视口内段落范围，只遍历视口内的行
        // 构建 cluster，不扫描全章 visual_lines。height_index 与 doc_snapshot 都是不可变
        // 借用 self.editor_layout，可安全共存。
        let height_index = self.editor_layout.height_index();
        let mut snapshot = match super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document_with_height_index(
            revision,
            doc_snapshot,
            scroll_y,
            viewport_h,
            height_index,
        ) {
            Ok(s) => s,
            Err(err) => return Err(err),
        };

        // 5. 为受影响行 + 视口内所有可见行提取 QImage 纹理，填充到
        // PreparedLineSnapshot.image。
        //
        // Issue #901 第八轮问题2修复：旧实现仅为 affected byte range 的行提取 QImage，
        // 导致不在 affected range 的视口内行只有 image=None。当静态 cluster 被选作
        // 动画来源时，缺纹理会被跳过（qquickitem_impl.rs 静态贡献生成时验证纹理存在）。
        // 现扩展到视口内所有可见行，确保静态 cluster 有纹理路径。
        let dpr = {
            let item_ptr = self.get_cpp_object();
            if !item_ptr.is_null() {
                crate::editor::renderer::sujian_item_dpr(item_ptr)
            } else {
                1.0
            }
        };
        let text_color = self.current_text_color.to_string();

        // Issue #901 第十八轮: 将全章物化改为只物化受影响段落 + 视口段落的行。
        //
        // 旧实现（第十四轮问题1）调用 resolved_visual_lines() 物化整章所有视觉行，
        // 然后在物化后的行上用 partition_point 定位 affected range 和视口行范围。
        // 对于大文档（数千行），每帧 O(整章) 物化是性能瓶颈。
        //
        // 新实现：
        // 1. 用 paragraph_offset_index.byte_to_paragraph() 定位 affected range 的段落范围
        // 2. 用 height_index.paragraphs_in_y_range() 定位视口段落范围
        // 3. 合并段落范围，用 cache_slot 在未物化的 visual_lines 上二分查找行索引范围
        // 4. 只物化这些行（resolve_visual_line_range）
        // 5. 在物化后的子集上做 partition_point 定位 affected range 行和视口行
        // 当 paragraph_offset_index 或 height_index 为空时，退化到全章物化逻辑。
        let (affected_start, affected_end) = affected_range.unwrap_or((0, 0));

        let max_line_h = doc_snapshot.font_size * doc_snapshot.line_spacing * 2.0 + 8.0;
        let vp_top = scroll_y - max_line_h;
        let vp_bottom = scroll_y + viewport_h + max_line_h;

        // 计算需要物化的段落范围 [para_start, para_end)。
        // 合并 affected range 段落范围和视口段落范围的并集。
        let has_indices = !doc_snapshot.paragraph_offset_index.is_empty()
            && !doc_snapshot.height_index.is_empty();

        let resolved: std::borrow::Cow<'_, [VisualLine]> = if has_indices {
            // 定位 affected range 的段落范围。
            let (aff_para_start, aff_para_end) = if affected_start < affected_end {
                let p_start = doc_snapshot
                    .paragraph_offset_index
                    .byte_to_paragraph(affected_start);
                let p_end = doc_snapshot
                    .paragraph_offset_index
                    .byte_to_paragraph(affected_end.saturating_sub(1));
                (p_start, p_end + 1)
            } else {
                (0, 0)
            };

            // 定位视口段落范围。
            let (vp_para_start, vp_para_end) = doc_snapshot
                .height_index
                .paragraphs_in_y_range(vp_top, vp_bottom);

            // 合并段落范围并集。
            let para_start = if aff_para_start < aff_para_end {
                aff_para_start.min(vp_para_start)
            } else {
                vp_para_start
            };
            let para_end = if aff_para_start < aff_para_end {
                aff_para_end.max(vp_para_end)
            } else {
                vp_para_end
            };

            if para_start < para_end {
                // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 索引查行范围，
                // 替换 raw partition_point(logical_slot)。
                // 插段后复用后缀行的 raw logical_slot 没有更新，二分查找找错；
                // paragraph_id_to_line_range 按 visual_lines 实际行顺序构建，
                // 索引为空时退化为 partition_point(logical_slot)。
                let (line_start_idx, line_end_idx) =
                    doc_snapshot.paragraph_visual_line_range(para_start, para_end);

                if line_start_idx < line_end_idx {
                    // 只物化需要的行，不物化整章。
                    let materialized =
                        doc_snapshot.resolve_visual_line_range(line_start_idx, line_end_idx);
                    std::borrow::Cow::Owned(materialized)
                } else {
                    doc_snapshot.resolved_visual_lines()
                }
            } else {
                doc_snapshot.resolved_visual_lines()
            }
        } else {
            // paragraph_offset_index 或 height_index 为空，退化到全章物化。
            doc_snapshot.resolved_visual_lines()
        };
        let resolved_lines = resolved.as_ref();

        // 合并 affected range 行索引和视口内可见行索引。
        use std::collections::HashSet;
        let mut line_indices_set: HashSet<usize> = HashSet::new();

        if affected_start < affected_end {
            // Issue #901 问题5缺陷B修复: 不遍历全章 visual_lines 找受影响行。
            // visual_lines 按文档顺序排列，byte_start 和 byte_end 均非递减
            // （相邻行 byte_start 不递减，行不重叠且按序）。用 partition_point
            // 二分定位受影响行范围 [start, end)，从 O(全章行数) 降为
            // O(log n + 受影响行数)。
            //
            // 原线性条件: vl.byte_start < affected_end && vl.byte_end > affected_start
            // 等价于: vl.byte_end > affected_start && vl.byte_start < affected_end
            // - start = 第一个 byte_end > affected_start 的索引
            //   （partition_point 返回第一个不满足 byte_end <= affected_start 的位置）
            // - end   = 第一个 byte_start >= affected_end 的索引
            //   （partition_point 返回第一个不满足 byte_start < affected_end 的位置）
            // [start, end) 即所有满足原条件的行，与线性扫描结果完全一致。
            let start = resolved_lines.partition_point(|vl| vl.byte_end <= affected_start);
            let end = resolved_lines.partition_point(|vl| vl.byte_start < affected_end);
            line_indices_set.extend(start..end);
        }

        // Issue #901 第八轮问题2：视口内所有可见行也提取 QImage。
        // visual_lines 按文档 y 坐标递增排列，用二分查找定位视口内行范围。
        // 视口在文档坐标系中的 y 范围：[scroll_y, scroll_y + viewport_h]。
        // 加边距确保边缘行不被遗漏（与 line_snapshot_builder.rs 一致）。
        let viewport_start = resolved_lines.partition_point(|vl| vl.y + vl.height <= vp_top);
        let viewport_end = resolved_lines.partition_point(|vl| vl.y < vp_bottom);
        line_indices_set.extend(viewport_start..viewport_end);

        let mut affected_line_indices: Vec<usize> = line_indices_set.into_iter().collect();
        affected_line_indices.sort_unstable();

        // Issue #904 评论 5: 本版布局事实源 —— generation + 物化视觉行。
        //
        // 冻结缺图行与后续纹理补图都只允许用这一份。**不再**让后续准备阶段
        // 读当前 live `editor_layout.cache()`：编辑后它已经是另一版正文，
        // 用 byte range 相等会把新字冒充成旧字（本评论修复的根因）。
        let source = super::pipeline::LocalLayoutSource {
            generation,
            // Issue #901 第三十六轮问题3: lines 用 PersistentChunkedVec。
            // resolved_lines 后续仍需使用，此处 to_vec() clone 一次后 from_vec move，
            // 由本版快照的 LocalLayoutSource 持有。
            lines: Arc::new(
                crate::editor::layout::chunked_vec::PersistentChunkedVec::from_vec(
                    resolved_lines.to_vec(),
                ),
            ),
        };
        // 同帧临时 handle：generation 是当前 layout_generation，layout 存活，
        // 本帧内不会被 release_retired_layouts 释放，不需要持有 lease。
        let source_handle = crate::editor::layout::PreparedLayoutHandle {
            generation,
            lines: Arc::clone(&source.lines),
            leases: Vec::new(),
        };

        if !affected_line_indices.is_empty() {
            let anim_visuals = crate::editor::layout::prepare_animation_visuals_from_layout(
                &source_handle,
                &affected_line_indices,
                dpr,
                &text_color,
            );

            // 构建 (paragraph_slot, qtextline_idx) → byte_start 映射，用于匹配 PreparedLineSnapshot。
            // Issue #901 第十七轮问题3: 用 cache_slot（段落编号）作 key，
            // 与 AnimationRasterVisual.paragraph_slot 同语义。
            use std::collections::HashMap;
            let mut para_qtextline_to_byte_start: HashMap<(usize, i32), usize> =
                HashMap::with_capacity(affected_line_indices.len());
            for &idx in &affected_line_indices {
                let vl = &resolved_lines[idx];
                para_qtextline_to_byte_start
                    .insert((vl.cache_slot as usize, vl.qtextline_idx), vl.byte_start);
            }

            // 把 QImage 填充到对应的 PreparedLineSnapshot。
            for anim_line in anim_visuals {
                if let Some(image) = anim_line.image {
                    if let Some(&byte_start) = para_qtextline_to_byte_start
                        .get(&(anim_line.paragraph_slot, anim_line.qtextline_idx))
                    {
                        if let Some(prep_line) = snapshot
                            .line_snapshots
                            .iter_mut()
                            .find(|pl| pl.byte_start == byte_start)
                        {
                            prep_line.image = Some(image);
                        }
                    }
                }
            }
        }

        // Issue #904 评论 5: 把本版布局的真实行图冻结进不可变快照。
        // 旧快照在下一代布局产出后无法再从 live cache 复现，因此必须在产出这一版
        // 布局的此刻就补全；补不出保持 image=None，后续显式 missing，不冒充。
        freeze_missing_line_images(&mut snapshot, &source_handle, dpr, &text_color);

        // 6. 计算 caret（视口坐标 + 文档坐标）。
        let cursor_byte = self.pipeline.cursor();
        let caret =
            doc_snapshot.cursor_rect(cursor_byte, self.cursor_ctrl.affinity, scroll_y, viewport_h);
        let caret_doc = doc_snapshot.cursor_rect_doc(cursor_byte, self.cursor_ctrl.affinity);
        snapshot.caret_rect = Some(caret);
        snapshot.caret_rect_doc = Some(caret_doc);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;

        Ok((snapshot, Some(source)))
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
    ///
    /// Issue #826: `virtual_text` 快照路径（`build_virtual_layout_snapshot`）已删除。
    /// IME preedit 现在是独立临时显示层，不再需要一套 virtualText 排版快照，
    /// 也不再为它单独排版。
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
    ///
    /// Issue #810 评论 5933167246 问题3: 改返回 `Result<EditorLayoutSnapshot, String>`，
    /// `build_from_canonical_document` 返回 Err 时向上传播，不再 fallback 到空 snapshot。
    /// 调用方必须显式处理 Err（invariant failure 时结束本次视觉事务构造，不伪装成功）。
    pub(crate) fn build_editor_layout_snapshot(
        &mut self,
        width: f64,
        promote: bool,
        composition_range: Option<(usize, usize)>,
    ) -> Result<
        (
            EditorLayoutSnapshot,
            Option<super::pipeline::LocalLayoutSource>,
        ),
        String,
    > {
        // Issue #810 评论 5934658350: 显式传入 canonical text revision，不再让 helper
        // 自己猜 pipeline.text_revision()。普通"当前正文快照"路径传当前 revision。
        let canonical_text_revision = self.pipeline.text_revision();
        self.build_editor_layout_snapshot_with_canonical(
            width,
            promote,
            composition_range,
            canonical_text_revision,
        )
    }

    /// Issue #738 评论 5797637204: 共用 helper，返回
    /// `(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot)`。
    /// `EditorLayoutSnapshot` 供动画/纹理使用，`CanonicalDocumentVisualSnapshot` 供
    /// composition commit 路径提交到 `Pipeline.current_canonical_snapshot` 并作为
    /// `reconcile_active_transactions_with_canonical` 的新 canonical 几何。
    /// 一次排版同时产出两份视图，避免 composition commit 再单独排一次 canonical。
    ///
    /// Issue #810 评论 5933167246 问题3: 改返回
    /// `Result<(EditorLayoutSnapshot, CanonicalDocumentVisualSnapshot), String>`，
    /// `build_from_canonical_document` 返回 Err 时向上传播，不再 fallback 到空 snapshot。
    pub(crate) fn build_editor_layout_snapshot_with_canonical(
        &mut self,
        width: f64,
        promote: bool,
        composition_range: Option<(usize, usize)>,
        canonical_text_revision: u64,
    ) -> Result<
        (
            EditorLayoutSnapshot,
            Option<super::pipeline::LocalLayoutSource>,
        ),
        String,
    > {
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
        let committed_text = self.pipeline.committed_text();
        // Issue #901 第二十轮阻塞2: 先确保 paragraph_store 与 committed_text 同步，
        // 再获取 ParagraphId 序列。rebuild_from_text 保持未变化段落的 ID
        //（前缀+后缀内容匹配），对已同步 store 是幂等的；对未同步 store
        //（IME commit 改变了段落数）分配正确 ID，使 canonical_snapshot 中的
        // paragraph_id 与后续 promote_prepared_layout 重建的 store ID 一致
        //（两次 rebuild 对相同 text 产生相同 ID）。
        // Issue #901 第三十八轮缺口3: 不再无条件 rebuild_from_text。
        // 检查 store 段落数是否与 committed_text 段落数匹配。匹配则跳过 rebuild
        //（store 已通过增量排版同步，ID 序列有效），不匹配才 rebuild。
        let committed_para_count = committed_text.split('\n').count();
        let store_para_count = self.editor_layout.paragraph_store().paragraph_count();
        if store_para_count != committed_para_count {
            self.editor_layout.paragraph_store_mut().rebuild_from_text(
                committed_text,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            );
        }
        let paragraph_ids: Vec<crate::editor::layout::ParagraphId> =
            self.editor_layout.paragraph_store().paragraph_ids();
        let mut doc_snapshot = crate::editor::layout::prepare_document_visual_snapshot_scoped(
            committed_text,
            canonical_text_revision,
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
            &paragraph_ids,
        );

        // Issue #901 第十四轮问题1: 用 resolved_visual_lines 物化绝对坐标。
        // TODO(issue-901): 后续优化为按需查询 — 此处需遍历物化行做 diff 比较
        // 和 composition range 覆盖检查，难以简单改为按需物化。
        //
        // Issue #901 第二十一轮问题5: 本轮复核确认保留。此处 resolved_visual_lines
        // 用于 IME composition 的 old/new visual line diff 比较和 animation visual 提取，
        // 需要完整的物化行列表做逐行比较。改为按需物化需要重构 diff 算法，
        // 正确性风险高，保留标注。此路径仅在 IME composition 时触发，非每笔普通输入热路径。
        let resolved = doc_snapshot.resolved_visual_lines();
        let resolved_lines = resolved.as_ref();

        // Issue #904 评论 5: 本版布局事实源（generation + 物化视觉行）。
        //
        // 冻结缺图行与后续补图都只允许用这一份。**不**再让纹理准备阶段读当前
        // live `editor_layout.cache()`：IME commit 时缓存还是编辑前的一版正文，
        // 用 byte range 相等会把另一版字形冒充成本快照行（本评论修复的根因）。
        //
        // promote=true 时 generation 在本函数返回后继续存活（由 pending promoted
        // layout 持有），可作为新一侧的补图源；promote=false 时 generation 在函数
        // 末尾被 retire，返回 None，只能用冻结 image。
        let source_lines = Arc::new(
            crate::editor::layout::chunked_vec::PersistentChunkedVec::from_vec(
                resolved_lines.to_vec(),
            ),
        );
        let source_handle = crate::editor::layout::PreparedLayoutHandle {
            generation,
            lines: Arc::clone(&source_lines),
            leases: Vec::new(),
        };

        // Issue #658 评论 5625515748 问题 3: 从已有 prepared layout 提取受影响行的动画视觉
        // （QImage/glyph/cluster）并注入到 doc_snapshot，使 LineSnapshotBuilder 能消费。
        // 只有 composition_range 非空时才提取（None 表示全篇 fallback 语义，scoped 排版
        // 已对全篇 generate_animation_visuals=false，无动画视觉需提取）。
        // Issue #904 清理: 不再按旧 reflow diff 挑行，也不再合并 active rebind ranges。
        // IME commit 的纹理提取只取"包含 Core 受影响字节范围的段落所覆盖的行"，
        // 与普通输入的 `build_incremental_layout_snapshot` 同一范围语义；下游纯 y
        // 位移段落由 `TextMotion.paragraph_moves` 处理。
        if affected_start < affected_end {
            // Issue #904 清理: 不再用旧 reflow diff（`compare_old_new_visual_lines` /
            // `reusable_move_pairs`），直接按受影响字节范围过滤 resolved 行。
            let line_ids: Vec<usize> = resolved_lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.byte_start < affected_end && l.byte_end > affected_start)
                .map(|(i, _)| i)
                .collect();
            if !line_ids.is_empty() {
                // Issue #810 评论 5932233052 问题1: cluster 已由基础 canonical 排版直接产出
                //（prepare_document_visual_snapshot_scoped 现在始终产出 cluster）。
                // 此处 prepare_animation_visuals_from_layout + inject 仅用于提取可延迟的
                // QImage/纹理（返回 AnimationRasterVisual，不携带 cluster），
                // 不再为动画单独补 cluster。inject 只注入 image，不覆盖已有 cluster。
                // Issue #904 评论 5: 复用本版布局事实源的 handle，不再另建一份。
                let line_snapshots = crate::editor::layout::prepare_animation_visuals_from_layout(
                    &source_handle,
                    &line_ids,
                    dpr,
                    text_color,
                );
                // Issue #785 评论 5857873894 修改 2b: inject 返回成功注入行数，此处忽略。
                let _ = crate::editor::layout::inject_animation_visuals_into_snapshot(
                    &mut doc_snapshot,
                    line_snapshots,
                );
            }
        }

        let cursor_byte = self.pipeline.cursor();
        let caret =
            doc_snapshot.cursor_rect(cursor_byte, self.cursor_ctrl.affinity, scroll_y, viewport_h);
        // Issue #722 评论 5749791161: 同时生成文档坐标的 caret，供 VisualTransaction / caret track 使用。
        let caret_doc = doc_snapshot.cursor_rect_doc(cursor_byte, self.cursor_ctrl.affinity);

        // Issue #810 评论 5934060933 问题2: 不要在 generation 已分配后直接对
        // build_from_canonical_document 用裸 `?`。Err 时 generation 还没有交给
        // pending promoted layout，必须由当前函数释放，避免泄漏 C++ QTextLayout generation。
        let mut snapshot =
            match super::line_snapshot_builder::LineSnapshotBuilder::build_from_canonical_document(
                revision,
                &doc_snapshot,
                scroll_y,
                viewport_h,
            ) {
                Ok(s) => s,
                Err(err) => {
                    // Issue #901 第三十二轮复核问题1: 临时 generation 释放创建者初始引用，
                    // 避免 QTextLayout 泄漏（retire_temporary_generation 标记 retired
                    // 并减 refcount，归零时 delete layout、erase generation）。
                    crate::editor::layout::retire_temporary_generation(generation);
                    return Err(err);
                }
            };
        snapshot.caret_rect = Some(caret);
        snapshot.caret_rect_doc = Some(caret_doc);
        snapshot.caret_affinity = self.cursor_ctrl.affinity;

        // Issue #904 评论 5: 在 retire generation 之前把本版布局的真实行图冻结进快照。
        // 快照一旦作为旧版本被下一代布局覆盖，就无法再从 Qt 布局复现这些字形。
        freeze_missing_line_images(&mut snapshot, &source_handle, dpr, text_color);

        if promote {
            // Issue #658 评论 5623746506 问题 2b: composition commit 的 new text
            // generation 直接成为 current。构造 PromotedLayout 存入 pending，
            // emit_content_changed 取出提升为 EditorLayout current，后续
            // ensure_layout_cached cache hit 不再重新排版同一已提交正文。
            // Issue #658 评论 5624570557 问题 1: composition commit 路径没有 old prepared layout，
            // old_generation 设为 0。
            // Issue #901 第九轮阻塞5: Arc::clone 共享视觉行数据，不 clone Vec。
            let promoted_visual_lines = Arc::clone(&doc_snapshot.visual_lines);
            // Issue #901 第十八轮问题1+2: 直接转交完整 canonical snapshot，
            // 不再从 visual_lines 重建（重建会丢空段和 clusters/image/cursor_x_map）。
            // doc_snapshot 在函数末尾仍要返回给调用方，此处 clone 一份。
            // CanonicalDocumentVisualSnapshot 的 visual_lines 是 Arc（clone 廉价），
            // paragraphs 是 Vec 需 deep clone，但这是 IME commit 路径（非每笔普通编辑），
            // clone 一次可接受。
            let promoted_canonical = doc_snapshot.clone();
            self.pipeline.set_pending_promoted_layout(Some(
                crate::editor::layout::PromotedLayout {
                    generation,
                    visual_lines: promoted_visual_lines,
                    canonical_snapshot: promoted_canonical,
                    width,
                    font_size: font_size as f32,
                    font_family: font_family.clone(),
                    line_spacing: line_spacing as f32,
                    text_indent: text_indent as f32,
                    padding: padding as f32,
                    old_generation: 0,
                    // Issue #901 第三十七轮问题3: IME commit 是普通文本编辑 promotion，
                    // 走接管版本根路径（O(段落数)），不走全篇 rebuild。
                    full_rebuild: false,
                },
            ));
        } else {
            // Issue #658 评论 5621512329 问题 1: 临时 generation 的 QTextLayout 已在
            // prepare_document_visual_snapshot 内部提取完 canonical line/image/cursor
            // 数据并存入 QImage（独立图像数据）。EditorLayoutSnapshot 不携带
            // layout_generation，不被 rebuild_text_node_from_paragraphs 消费，
            // 因此立即释放临时 generation，避免 layout 泄漏或被固定阈值误删。
            // Issue #901 第三十二轮复核问题1: 用 retire_temporary_generation 释放创建者
            // 初始引用（clear_layout_generation 只标记 retired 不减 refcount，临时
            // generation 不进 Store 所有权登记，无后续 release_layout_ref，会泄漏）。
            crate::editor::layout::retire_temporary_generation(generation);
        }

        // Issue #904 评论 5: 只有 promote=true 时本版 generation 在函数返回后仍存活，
        // 才能作为新一侧的补图源交给调用方；promote=false 时 generation 已 retire，
        // 句柄立即失效，返回 None（该侧只能用快照里已冻结 image）。
        let layout_source = if promote {
            Some(super::pipeline::LocalLayoutSource {
                generation,
                lines: Arc::clone(&source_lines),
            })
        } else {
            None
        };

        // Issue #904 清理: canonical document snapshot 只在本函数内部交给
        // LineSnapshotBuilder，不再作为第二份 canonical 上交 Pipeline。
        Ok((snapshot, layout_source))
    }

    pub(crate) fn ensure_layout_cached(
        &mut self,
        width: f64,
    ) -> Arc<crate::editor::layout::chunked_vec::PersistentChunkedVec<VisualLine>> {
        let params = self.layout_params(width);
        // Issue #901 第三十六轮问题3: Arc::clone 共享 LayoutSnapshot.lines，
        // 不 to_vec() 整章复制 VisualLine（其中有 para_text: String）。
        Arc::clone(
            &self
                .editor_layout
                .snapshot(
                    self.pipeline.committed_text(),
                    params,
                    self.pipeline.text_revision(),
                )
                .lines,
        )
    }

    pub(crate) fn adjust_affinity_at_wrap_boundary(&mut self) {
        let width = self.bounding_width();
        let cursor = self.pipeline.cursor();
        // Issue #901 第十轮问题5: 用 resolved_lines 按需物化后缀段绝对坐标。
        // TODO(issue-901): 后续优化为按需查询 — 遍历找换行边界可先用二分定位再 resolve_line。
        let params = self.layout_params(width);
        let snapshot = self.editor_layout.snapshot(
            self.pipeline.committed_text(),
            params,
            self.pipeline.text_revision(),
        );
        let resolved = snapshot.resolved_lines();
        let lines = resolved.as_ref();

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
            self.pipeline.cursor(),
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
}

/// Issue #904 评论 5：把**这一版** Qt 布局的真实行图冻结进不可变快照。
///
/// 为什么必须在产出快照的此刻冻结：增量排版会在同一 generation 内原地重排、
/// 覆盖受影响段落的 QTextLayout（`install_layout_slot_gen` 复用同一物理槽），
/// 旧的 QTextLine 之后不再可得。若等下一次编辑再去取旧字形，就只能拿到新正文，
/// 用 byte range 相等把新字冒充成旧字 —— 那正是本评论修复的根因。
///
/// 因此这里用与快照同版的 `source`（generation + 物化视觉行）为仍缺图、
/// 但确有可见 cluster 的快照行补出 QImage。定位/校验失败就保持 `image: None`，
/// 由 `prepare_local_text_textures` 显式记入 missing，绝不用另一版正文冒充。
fn freeze_missing_line_images(
    snapshot: &mut EditorLayoutSnapshot,
    source: &crate::editor::layout::PreparedLayoutHandle,
    dpr: f64,
    text_color: &str,
) {
    // 收集需要补图的快照行及其在源视觉行里的索引。
    let mut targets: Vec<(usize, usize)> = Vec::new();
    for (snapshot_index, line) in snapshot.line_snapshots.iter().enumerate() {
        if line.image.is_some() || line.clusters.is_empty() {
            continue;
        }
        if let Some(source_index) =
            source_line_index_for_id(source, line.id, line.byte_start, line.byte_end)
        {
            targets.push((snapshot_index, source_index));
        }
    }
    if targets.is_empty() {
        return;
    }

    let line_indices: Vec<usize> = targets.iter().map(|(_, index)| *index).collect();
    let visuals = crate::editor::layout::prepare_animation_visuals_from_layout(
        source,
        &line_indices,
        dpr,
        text_color,
    );

    for (snapshot_index, source_index) in targets {
        let Some(source_line) = source.lines.get(source_index) else {
            continue;
        };
        // 结果按 (段落 logical slot, 段内 QTextLine 序号) 匹配，不依赖返回顺序
        //（提取失败的行会被上游跳过）。
        let key = (source_line.logical_slot, source_line.qtextline_idx);
        let image = visuals
            .iter()
            .find(|visual| (visual.paragraph_slot as i32, visual.qtextline_idx) == key)
            .and_then(|visual| visual.image.clone());
        if let Some(image) = image {
            snapshot.line_snapshots[snapshot_index].image = Some(image);
        }
    }
}

/// Issue #904 评论 5：按 `LineSnapshotId`（段落 logical slot + 段内视觉行号）在
/// 一版物化视觉行里定位对应的行索引。
///
/// 视觉行按文档次序排列，同段落的行连续。用 `logical_slot` 匹配段落、按出现次序
/// 数到 `visual_line_ordinal`，再用非空正文 + byte range 确认确实是该快照行。
/// 定位不到或 byte range 不一致时返回 `None`（调用方显式记 missing）。
fn source_line_index_for_id(
    source: &crate::editor::layout::PreparedLayoutHandle,
    id: super::snapshot_id::LineSnapshotId,
    byte_start: usize,
    byte_end: usize,
) -> Option<usize> {
    let mut seen_ordinal: u32 = 0;
    for index in 0..source.lines.len() {
        let Some(line) = source.lines.get(index) else {
            continue;
        };
        if line.logical_slot as u64 != id.paragraph_id {
            continue;
        }
        if seen_ordinal == id.visual_line_ordinal {
            if !line.para_text.is_empty()
                && line.byte_start == byte_start
                && line.byte_end == byte_end
            {
                return Some(index);
            }
            return None;
        }
        seen_ordinal += 1;
    }
    None
}
