// ── Qt 文本布局模块 ──
//
// 坐标空间约定：
// - Qt 层（本模块）：QChar index（UTF-16 code unit），与 QTextLayout/QTextLine API 一致
// - Core 层：UTF-8 byte offset
// - 转换入口：`sujian_editor_item` 中的 `utf8_to_utf16` / `utf16_to_utf8`
//   在调用本模块函数前完成坐标转换
//
// 线程安全：`g_editor_layout_buf` 为 thread_local，
// 仅在 GUI 线程中使用，不跨线程共享。
//
// 模块拆分（Issue #748 评论 5805325152 / 5810761209）：
// - types: 纯数据类型
// - qt_cache: QTextLayout/QTextLine generation cache
// - engine: 段落排版核心、几何 helper、坐标转换
// - hit_test: hit test / caret / cursor 定位
// - canonical_snapshot: Canonical document visual snapshot
// - diff: VisualLine diff / affected paragraph ranges
// - test_support: 测试线程 helper (run_on_qt_thread)，仅 test/test-helpers feature 下编译
// - paragraph_layout_store: Issue #901 稳定段落身份 + 布局缓存
// - paragraph_height_index: Issue #901 Fenwick 树段落前缀高度索引
// - chunked_vec: Issue #901 第三十三轮问题3 分块前缀和 + 分块 slot 映射

mod canonical_snapshot;
// Issue #901 第三十四轮缺口2: chunked_vec 对 crate 可见，供 pipeline/tests.rs 等构造 ChunkedSlotMap。
pub(crate) mod chunked_vec;
mod diff;
mod engine;
mod hit_test;
mod paragraph_height_index;
mod paragraph_layout_store;
mod qt_cache;
mod test_support;
mod types;

// ── re-export：外部公开类型 ──
pub use canonical_snapshot::{
    inject_animation_visuals_into_snapshot,
    prepare_affected_paragraphs_visual_snapshot, prepare_animation_visuals_from_layout,
    prepare_document_visual_snapshot, prepare_document_visual_snapshot_scoped,
    prepare_incremental_visual_snapshot,
    AnimationRasterVisual, CanonicalClusterSnapshot,
    CanonicalDocumentVisualSnapshot, CanonicalLineSnapshot, CanonicalParagraphSnapshot,
    CursorXMapEntry,
};
pub use diff::compute_affected_paragraph_ranges;
// Issue #901: 增量排版子系统 — 稳定段落身份 + 布局缓存 + Fenwick 树高度索引
pub use paragraph_height_index::{ParagraphHeightIndex, ParagraphLineCountIndex, ParagraphOffsetIndex};
pub use paragraph_layout_store::{
    AffectedParagraphRanges, ParagraphLayoutCacheKey, ParagraphLayoutEntry,
    ParagraphLayoutStore,
};
pub use engine::{
    byte_offset_to_qchar_offset, cursor_rect_for_line, get_font_ascent, get_font_descent,
    qchar_offset_to_byte_offset, text_baseline_y,
};
pub use hit_test::{
    affinity_for_index_on_line, calculate_cursor_x_for_line, caret_rect, caret_rect_doc,
    cursor_line_and_x, hit_test, index_at_line_x, line_contains_cursor_with_affinity,
};
pub use qt_cache::{
    begin_layout_generation, clear_layout_generation, retire_temporary_generation,
    get_paragraph_layout_cursor_to_x_on_line, get_paragraph_layout_cursor_to_x_on_line_checked,
    get_paragraph_layout_x_to_cursor_on_line, get_paragraph_layout_x_to_cursor_on_line_checked,
    retain_layout_ref,
};
#[cfg(any(test, feature = "test-helpers"))]
pub use test_support::run_on_qt_thread;
pub use types::{
    CaretAffinity, CaretRect, CursorLayoutRect, LayoutParams, LayoutSnapshot, ParagraphId,
    PreparedLayoutHandle, PromotedLayout, QtLayoutId, QtLayoutLease, VisualLine,
};
use std::sync::Arc;
use chunked_vec::PersistentChunkedVec;

/// 编辑器布局引擎 — 管理 QTextLayout 排版缓存。
///
/// 线程约束：QTextLayout 只能在 GUI 线程使用，EditorLayout 不可跨线程。
/// 缓存策略：snapshot() 在参数/revision 不变时复用缓存，避免重复排版。
/// Issue #658 评论 5620035970 问题 2: 持有 current_generation，
/// invalidate 时 clear 旧 generation，重新排版时分配新 generation。
///
/// Issue #901: 集成增量排版子系统：
/// - `paragraph_store`: 稳定段落身份 + 布局缓存，判断哪些段落需要重排。
/// - `height_index`: Fenwick 树段落前缀高度索引，O(log n) 查 y→段落 / 段顶 / 改段高。
#[derive(Default)]
pub struct EditorLayout {
    cache: Option<LayoutSnapshot>,
    current_generation: u64,
    /// Issue #901: 段落布局存储 — 稳定段落身份 + 布局缓存。
    paragraph_store: ParagraphLayoutStore,
    /// Issue #901: 段落高度索引 — Fenwick 树前缀高度。
    /// Issue #901 第二十九轮复核阻塞4: 用 Arc 共享，避免每笔 snapshot() clone O(段落数)。
    /// EditorLayout 是可变权威源，snapshot 通过 Arc::clone O(1) 共享。
    /// 修改时用 Arc::make_mut（refcount==1 原地改，refcount>1 COW）。
    height_index: Arc<ParagraphHeightIndex>,
    /// Issue #901 第二轮复核: 上一版 immutable canonical snapshot，
    /// 传给 `prepare_incremental_visual_snapshot` 的 `previous_snapshot` 参数，
    /// 使增量排版路径能复用不受影响段落的 VisualLine/shaping/纹理。
    /// 参数变化（invalidate_params）时清空，普通编辑保留。
    last_snapshot: Option<CanonicalDocumentVisualSnapshot>,
    /// Issue #901 第三轮复核: Core `EditorEditResult.display_patches` 提供的
    /// 受影响字节范围。由外部调用 `set_last_edit_affected_range` 设置，
    /// `snapshot()` 消费后清空。优先于按段落下标比较 hash 推测编辑范围，
    /// 避免 Enter 拆段/Backspace 合段后段落索引整体偏移导致的误判。
    last_edit_affected_range: Option<(usize, usize)>,
}

impl EditorLayout {
    /// 全局失效 — 清空所有缓存（排版参数变化时调用）。
    ///
    /// Issue #901: 此方法对应**字体/宽度/字号/行距/全局格式变化**的全局失效。
    /// 清空 `paragraph_store` 和 `height_index`，因为这些缓存依赖排版参数，
    /// 参数变化后所有段落的 cache_key 都失效，必须全部重排。
    pub fn invalidate(&mut self) {
        // Issue #658 评论 5620035970 问题 2: 失效时释放旧 generation 的 layout cache。
        if self.current_generation != 0 {
            clear_layout_generation(self.current_generation);
            self.current_generation = 0;
        }
        self.cache = None;
        // Issue #901: 同步清空增量排版子系统（全局失效）。
        self.paragraph_store.clear();
        Arc::make_mut(&mut self.height_index).clear();
        // Issue #901 第二轮复核: 参数变化全局失效，旧 snapshot 不再可复用。
        self.last_snapshot = None;
        // Issue #901 第三轮复核: 清空编辑范围缓存。
        self.last_edit_affected_range = None;
    }

    /// Issue #901: 全局参数失效 — 字体/宽度/字号/行距/全局格式变化时调用。
    ///
    /// 与 [`invalidate`] 相同，清空所有缓存。语义上明确表示这是参数变化导致的
    /// 全局失效，不是普通正文编辑。
    pub fn invalidate_params(&mut self) {
        self.invalidate();
    }

    /// Issue #901: 局部段落失效 — 普通正文编辑时调用。
    ///
    /// 只失效受影响段落，不清空整个缓存。未改段落的 shaping/行数据/纹理保持有效。
    ///
    /// # 参数
    ///
    /// - `affected_byte_start` / `affected_byte_end`：受影响字节范围。
    ///   `affected_byte_start >= affected_byte_end` 表示无明确范围，不做任何操作
    ///   （下一次 `snapshot` 会通过 text_revision 变化检测到需要重排）。
    pub fn invalidate_affected(&mut self, affected_byte_start: usize, affected_byte_end: usize) {
        if affected_byte_start >= affected_byte_end {
            return;
        }
        // 只失效受影响段落的布局缓存，不清空整个 store
        // Issue #901 第三十二轮问题3: 优先用 last_snapshot.paragraph_offset_index
        // 的 O(log n) byte_to_paragraph 二分定位替换全文遍历。invalidate_affected
        // 在编辑前调用，last_snapshot 与 paragraph_store 描述同一编辑前段落结构。
        // 无 last_snapshot 时回退到 compute_affected_paragraphs 全文遍历。
        // 用 block 限制 offset_index 借用作用域，确保后续 invalidate_range 的
        // 可变借用不冲突。
        let affected = {
            let offset_index = self
                .last_snapshot
                .as_ref()
                .map(|s| &*s.paragraph_offset_index);
            match offset_index {
                Some(idx) => self.paragraph_store.compute_affected_paragraphs_via_index(
                    affected_byte_start,
                    affected_byte_end,
                    idx,
                ),
                None => self.paragraph_store.compute_affected_paragraphs(
                    affected_byte_start,
                    affected_byte_end,
                ),
            }
        };
        if !affected.full_relayout {
            self.paragraph_store
                .invalidate_range(affected.relayout_start, affected.relayout_end);
        } else {
            // store 为空或全局失效
            self.paragraph_store.clear();
            Arc::make_mut(&mut self.height_index).clear();
        }
    }

    pub fn cache(&self) -> Option<&LayoutSnapshot> {
        self.cache.as_ref()
    }

    /// Issue #901 第三轮复核: 设置本次编辑的受影响字节范围。
    ///
    /// 由外部从 Core `EditorEditResult.display_patches` 的
    /// `replace_byte_range` / `inserted_byte_range` 提取后调用。
    /// `snapshot()` 会优先消费此范围定位受影响段落，
    /// 不再以整章下标比较 hash 冒充段落身份。
    ///
    /// 调用时机：在 `snapshot()` 之前调用。`snapshot()` 消费后自动清空。
    pub fn set_last_edit_affected_range(&mut self, byte_start: usize, byte_end: usize) {
        self.last_edit_affected_range = Some((byte_start, byte_end));
    }

    /// Issue #901 第三十二轮复核问题2: 查询当前未消费的受影响字节范围（非消费只读）。
    ///
    /// 与 [`set_last_edit_affected_range`] 对应的只读查询。[`snapshot`] 会消费此范围
    ///（`take`），消费后返回 `None`。`build_canonical_snapshot_for_current_layout`
    /// 用此方法判断是否能按需 lease：有范围则只对受影响段获取 lease，
    /// 无则回退到全量 [`current_prepared_layout`]。
    pub fn last_edit_affected_range(&self) -> Option<(usize, usize)> {
        self.last_edit_affected_range
    }

    /// Issue #901: 获取段落布局存储的只读引用。
    pub fn paragraph_store(&self) -> &ParagraphLayoutStore {
        &self.paragraph_store
    }

    /// Issue #901: 获取段落布局存储的可变引用。
    pub fn paragraph_store_mut(&mut self) -> &mut ParagraphLayoutStore {
        &mut self.paragraph_store
    }

    /// Issue #901: 获取段落高度索引的只读引用。
    pub fn height_index(&self) -> &ParagraphHeightIndex {
        &self.height_index
    }

    /// Issue #901: 获取段落高度索引的可变引用。
    /// Issue #901 第二十九轮复核阻塞4: Arc::make_mut 获取可变引用（COW 语义）。
    pub fn height_index_mut(&mut self) -> &mut ParagraphHeightIndex {
        Arc::make_mut(&mut self.height_index)
    }

    /// Issue #658 评论 5624570557 问题 1: 获取当前有效的 prepared layout 句柄（如果存在）。
    ///
    /// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
    /// 返回的句柄包含 generation 和 VisualLine 数组引用，
    /// 调用方可用 `prepare_animation_visuals_from_layout` 按需提取动画资源。
    ///
    /// Issue #901 第三十一轮问题3: 同时为每个有 Qt layout 的行创建 `QtLayoutLease`，
    /// 持有 C++ 引用计数。旧帧持有 lease 防止 Store 在新 snapshot 完成时
    /// `release_retired_layouts` 释放旧 layout，旧帧结束（handle Drop）才退引用。
    pub fn current_prepared_layout(&self) -> Option<PreparedLayoutHandle> {
        self.cache.as_ref().map(|c| {
            let generation = c.layout_generation;
            // Issue #901 第三十一轮问题3: 为每个有 Qt layout 的行创建 lease，
            // 接通所有权链。retain 成功才收集；失败（layout 已释放/epoch 不匹配）
            // 跳过该行 lease，不阻塞 handle 创建。
            //
            // Issue #901 第三十二轮复核问题1: 按 (generation, cache_slot, layout_epoch)
            // 三元组去重。一个长段自动折为多条视觉行，其实只对应同一个
            // (generation, physical_slot, epoch)，现在只 retain 同一 QTextLayout 一次，
            // 不按视觉行重复 retain。lease 的 try_from_id 调用 C++ retain_layout_ref
            // 是有成本的 FFI 调用，去重后从 O(视觉行数) 降到 O(唯一 layout 数)。
            //
            // Issue #901 第三十二轮复核缺口3: 从 paragraph_store 获取有 Qt layout 的段列表，
            // 而非遍历所有视觉行。一段可能有多行，遍历所有行是 O(视觉行数)，
            // 从 paragraph_store 获取段列表是 O(段落数)，且段落数通常远小于视觉行数。
            // paragraph_store 的每个段只有一个 (cache_slot, layout_epoch)，
            // 天然去重，无需 HashSet。只对 layout_generation == generation 且
            // cache_slot >= 0 的段创建 lease，确保与视觉行的 layout 一致。
            // lines: c.lines.as_slice() 保持不变，调用方仍可通过 handle.lines 访问所有视觉行。
            let mut leases = Vec::new();
            let store = &self.paragraph_store;
            for i in 0..store.len() {
                if let Some(entry) = store.entry(i) {
                    if entry.layout_generation == generation && entry.cache_slot >= 0 {
                        let id = QtLayoutId::new(generation, entry.cache_slot, entry.layout_epoch);
                        if let Some(lease) = QtLayoutLease::try_from_id(id) {
                            leases.push(lease);
                        }
                    }
                }
            }
            // Issue #901 第三十六轮问题3: Arc::clone O(1) 共享，不 to_vec() 整章复制。
            PreparedLayoutHandle {
                generation,
                lines: Arc::clone(&c.lines),
                leases,
            }
        })
    }

    /// Issue #901 第三十二轮复核问题2: 按需 lease API — 只对指定段落索引获取 lease。
    ///
    /// 与 [`current_prepared_layout`] 逻辑相同，但只对 `paragraph_indices` 中
    /// 指定的段落创建 `QtLayoutLease`，不遍历所有段。对于几万字、很多段落的章节，
    /// 动画准备不应每笔接触所有 Qt layout；只在受影响段获取 lease 即可。
    ///
    /// `lines` 字段仍保持 `c.lines.as_slice()`（所有视觉行），因为 handle 调用方
    /// 可能需要访问所有行（如 `prepare_animation_visuals_from_layout` 提取所有行
    /// 的 QImage/clusters）。lease 只限制"持有哪些段的 C++ layout 引用"，
    /// 不限制"能读取哪些视觉行"。
    ///
    /// # 参数
    ///
    /// - `paragraph_indices`：需要获取 lease 的段落索引列表。越界索引静默跳过。
    ///
    /// # 返回
    ///
    /// - `Some(handle)`：cache 存在时返回 handle（即使 `paragraph_indices` 为空，
    ///   handle 的 `leases` 为空 Vec，不持有任何 lease）。
    /// - `None`：cache 不存在（首帧/invalidate 后尚未排版）。
    ///
    /// # 复杂度
    ///
    /// O(`paragraph_indices.len()`) 次 `QtLayoutLease::try_from_id`（FFI retain），
    /// 而非 [`current_prepared_layout`] 的 O(段落数)。对于只编辑一段的常见场景，
    /// `paragraph_indices` 只含 1 个段，lease 成本从 O(全章段数) 降到 O(1)。
    pub fn current_prepared_layout_for_paragraphs(
        &self,
        paragraph_indices: &[usize],
    ) -> Option<PreparedLayoutHandle> {
        self.cache.as_ref().map(|c| {
            let generation = c.layout_generation;
            // Issue #901 第三十二轮复核问题2: 只遍历 paragraph_indices，
            // 不遍历 0..store.len()。对每个索引检查越界，越界跳过。
            // lease 创建逻辑与 current_prepared_layout 完全相同：
            // 只对 layout_generation == generation 且 cache_slot >= 0 的段
            // 创建 lease，retain 成功才收集；失败跳过不阻塞 handle 创建。
            let mut leases = Vec::new();
            let store = &self.paragraph_store;
            let store_len = store.len();
            for &i in paragraph_indices {
                if i >= store_len {
                    continue;
                }
                if let Some(entry) = store.entry(i) {
                    if entry.layout_generation == generation && entry.cache_slot >= 0 {
                        let id = QtLayoutId::new(
                            generation,
                            entry.cache_slot,
                            entry.layout_epoch,
                        );
                        if let Some(lease) = QtLayoutLease::try_from_id(id) {
                            leases.push(lease);
                        }
                    }
                }
            }
            // Issue #901 第三十六轮问题3: Arc::clone O(1) 共享，不 to_vec() 整章复制。
            PreparedLayoutHandle {
                generation,
                lines: Arc::clone(&c.lines),
                leases,
            }
        })
    }

    /// Issue #901 第三十二轮复核问题2: 按需 lease — 优先只对受影响段获取 lease。
    ///
    /// 从 `last_edit_affected_range` 获取受影响字节范围，用 `paragraph_store` 计算段落
    /// 索引范围，再调 [`current_prepared_layout_for_paragraphs`] 只对受影响段 retain。
    /// 无法确定受影响段范围（已被 [`snapshot`] 消费 / 首帧 / 全量重建 / full_relayout）
    /// 时回退到 [`current_prepared_layout`]（全量），保持原语义。
    ///
    /// 将此逻辑封装在 `EditorLayout` 方法中（而非 pipeline.rs 内联），使
    /// `build_canonical_snapshot_for_current_layout` 函数体保持简洁，
    /// 不因按需 lease 分支增长而超出静态守卫测试的函数窗口。
    pub fn current_prepared_layout_for_affected(&self) -> Option<PreparedLayoutHandle> {
        if let Some((byte_start, byte_end)) = self.last_edit_affected_range {
            // Issue #901 第三十三轮优化: 优先用 last_snapshot.paragraph_offset_index
            // 的 O(log n) byte_to_paragraph 二分定位，与 invalidate_affected 一致。
            // 修复 compute_affected_paragraphs 的 hard_break 边界误判（仅改变换行符
            // 的编辑可能误判为全章重排）。无 last_snapshot 时回退到全文遍历。
            let ranges = {
                let offset_index = self
                    .last_snapshot
                    .as_ref()
                    .map(|s| &*s.paragraph_offset_index);
                match offset_index {
                    Some(idx) => self.paragraph_store.compute_affected_paragraphs_via_index(
                        byte_start,
                        byte_end,
                        idx,
                    ),
                    None => self
                        .paragraph_store
                        .compute_affected_paragraphs(byte_start, byte_end),
                }
            };
            if !ranges.full_relayout && ranges.relayout_start < ranges.relayout_end {
                let indices: Vec<usize> = (ranges.relayout_start..ranges.relayout_end).collect();
                self.current_prepared_layout_for_paragraphs(&indices)
            } else {
                self.current_prepared_layout()
            }
        } else {
            self.current_prepared_layout()
        }
    }

    /// Issue #901 第三十三轮问题2: 返回本次动画实际需要查询的视觉行索引，
    /// 与 [`current_prepared_layout_for_affected`] 的 lease 范围**严格一致**。
    ///
    /// 从 `last_edit_affected_range` 获取受影响字节范围，用 `paragraph_store` 计算段落
    /// 索引范围，再从 `lines` 中筛选属于受影响段落的行索引（按 `logical_slot` 判断）。
    /// 无法确定受影响范围（首帧 / 全量重建 / `full_relayout`）时回退到全量，
    /// 与 [`current_prepared_layout_for_affected`] 回退到 [`current_prepared_layout`] 对称。
    ///
    /// # 为什么 lease 范围与查询范围必须一致
    ///
    /// `current_prepared_layout_for_affected` 只对受影响段创建 `QtLayoutLease`（retain
    /// C++ 引用）。若 `all_line_ids` 用 `0..lines.len()`（全量），会对未 retain 的段
    /// 调 `extract_animation_raster_from_existing_line`。虽然 `EditorLayout` 自身持有
    /// 这些 layout（generation 活跃），通常不会访问无效，但 lease 范围与查询范围不一致
    /// 是语义缺陷：一旦旧 generation 被 invalidate，未 retain 的段 layout 被释放，
    /// 全量查询会访问无效 `QTextLayout`。本方法保证查询范围 ⊆ lease 范围。
    ///
    /// # 参数
    ///
    /// - `lines`：`PreparedLayoutHandle.lines`（`EditorLayout` 当前已排版的视觉行集合）。
    ///
    /// # 返回
    ///
    /// 需要查询的视觉行索引列表。受影响段为空段时返回空 `Vec`（空段无 `QTextLayout`，
    /// 不需要提取 raster），调用方对此自然跳过，不回退全量。
    pub fn affected_visual_line_indices(
        &self,
        lines: &PersistentChunkedVec<VisualLine>,
    ) -> Vec<usize> {
        if let Some((byte_start, byte_end)) = self.last_edit_affected_range {
            // Issue #901 第三十三轮优化: 与 current_prepared_layout_for_affected 使用
            // 同一 compute_affected_paragraphs_via_index 路径，保证 lease 范围与查询范围
            // 严格一致，同时获得 O(log n) 定位和 hard_break 边界正确性。
            let ranges = {
                let offset_index = self
                    .last_snapshot
                    .as_ref()
                    .map(|s| &*s.paragraph_offset_index);
                match offset_index {
                    Some(idx) => self.paragraph_store.compute_affected_paragraphs_via_index(
                        byte_start,
                        byte_end,
                        idx,
                    ),
                    None => self
                        .paragraph_store
                        .compute_affected_paragraphs(byte_start, byte_end),
                }
            };
            if !ranges.full_relayout && ranges.relayout_start < ranges.relayout_end {
                // 用 ParagraphLineCountIndex 直接计算受影响段的视觉行闭开区间，
                // 不遍历全部视觉行用 logical_slot 过滤。
                let line_count_index = self.paragraph_store.line_count_index();
                let line_start = line_count_index.paragraph_line_start(ranges.relayout_start);
                let line_end = line_count_index.paragraph_line_start(ranges.relayout_end);
                // paragraph_line_start 返回的是前缀和（该段首行的全局视觉行索引）。
                // 受影响段 [relayout_start, relayout_end) 的视觉行范围是 [line_start, line_end)。
                let end = line_end.min(lines.len());
                (line_start..end).collect()
            } else {
                // full_relayout 或无效范围 → 全量，与 lease 全量对称。
                (0..lines.len()).collect()
            }
        } else {
            // 无受影响范围（首帧 / 全量重建）→ 全量，与 lease 全量对称。
            (0..lines.len()).collect()
        }
    }

    /// Issue #658 评论 5622829886 问题 1: 把外部已排好的 prepared layout 提升为 current。
    ///
    /// 由 record_visual_transaction 全篇排版 new text 后调用，
    /// 把 new_generation 和 visual_lines 直接设为 current，
    /// 后续 snapshot() 发现 cache 有效（text_revision/text_ptr/text_len 匹配）直接返回，
    /// 不再重新排版同一 new text。
    /// 旧 current_generation（若存在且不同于 promoted.generation）会被 clear。
    /// text_revision / text_ptr / text_len 从当前 buffer.text 和 pipeline.text_revision()
    /// 获取，确保与 snapshot() 的 cache 有效性检查一致。
    /// Issue #658 评论 5624570557 问题 1: 同时释放 old_generation（如果非 0）。
    pub fn promote_prepared_layout(
        &mut self,
        promoted: PromotedLayout,
        text: &str,
        text_revision: u64,
    ) {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        // 释放旧 generation（若存在且不同于新 generation）
        if self.current_generation != 0 && self.current_generation != promoted.generation {
            clear_layout_generation(self.current_generation);
        }
        // Issue #658 评论 5624570557 问题 1: 释放 old_generation（old 动画使用的 generation）
        if promoted.old_generation != 0 && promoted.old_generation != promoted.generation {
            clear_layout_generation(promoted.old_generation);
        }
        self.current_generation = promoted.generation;

        // Issue #901: 同步增量排版子系统。
        // 从 promoted visual_lines 提取排版参数，重建 paragraph_store 和 height_index。
        // promoted layout 是全篇排版结果，需要同步到 store 和 height_index。
        let font_size = promoted.font_size as f64;
        let font_family = &promoted.font_family;
        let line_spacing = promoted.line_spacing as f64;
        let text_indent = promoted.text_indent as f64;
        let padding = promoted.padding as f64;
        let width = promoted.width;

        // Issue #901 第三十七轮问题3: promotion 不再无条件全篇重建 Store。
        // - full_rebuild=true（字体/宽度全局变化、初始化）→ 走全篇 rebuild。
        // - full_rebuild=false（普通文本编辑 promotion）→ 接管版本根，
        //   直接从 canonical snapshot 接管段落布局数据和索引（O(段落数)），
        //   不从 text 重建 Store，不复制全部 VisualLine，不扫描全部视觉行。
        if promoted.full_rebuild {
            self.paragraph_store.rebuild_from_text(
                text,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            );
            let promoted_vls = promoted.visual_lines.to_vec();
            self.paragraph_store
                .sync_from_visual_lines(&promoted_vls, promoted.generation);

            // Issue #901 第三十二轮复核问题2: clear_layout_generation 改为 retired 标记语义后，
            // 旧 generation 的 Store 初始引用（refcount=1）不再被 clear 暴力 delete 释放，
            // 需要由 release_retired_layouts 显式退引用。rebuild_from_text 和 sync_from_visual_lines
            // 把未被继承的旧 handle 推入 retired_slots，此处统一释放。
            self.paragraph_store.release_retired_layouts();

            let heights = self.paragraph_store.paragraph_heights();
            Arc::make_mut(&mut self.height_index).rebuild(heights);
        } else {
            // Issue #901 第三十七轮问题3: 接管版本根路径。
            // adopt_from_canonical_snapshot 从 canonical snapshot 接管段落布局数据：
            // - entries 从 snapshot.paragraphs 构造（O(段落数)），每段 O(1) 从索引
            //   查询 visual_line_count/height/cache_slot/layout_epoch/paragraph_id。
            // - id_to_slot_cache/line_count_index/byte_offset_index 直接 Arc::clone 接管。
            // 不调用 rebuild_from_text（省去 O(文本长度) 分段），
            // 不调用 visual_lines.to_vec()（省去 O(视觉行数) 深拷贝），
            // 不调用 sync_from_visual_lines（省去 O(视觉行数) 分组统计）。
            self.paragraph_store.adopt_from_canonical_snapshot(
                &promoted.canonical_snapshot,
                promoted.generation,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            );
            // 旧 handle 推入 retired_slots 后统一释放（与全篇 rebuild 路径对称）。
            self.paragraph_store.release_retired_layouts();
            // height_index 直接 Arc::clone 接管 canonical snapshot 的 height_index，
            // 不调用 paragraph_heights() + rebuild(heights)（省去 O(段落数) 重建）。
            self.height_index = Arc::clone(&promoted.canonical_snapshot.height_index);
        }

        // Issue #901 第三轮复核问题 5: 不再把 last_snapshot 设为 None。
        // Issue #901 第十八轮问题1+2: 直接使用 promoted 携带的完整 canonical snapshot，
        // 不再从 visual_lines 重建（重建会丢空段和 clusters/image/cursor_x_map）。
        // 使下次普通编辑（仅 text_revision 变化）能走增量排版路径，
        // 不会因为 last_snapshot=None 而退化为全篇重排。
        // promote_prepared_layout 与增量缓存共用同一身份和生命周期。
        self.last_snapshot = Some(promoted.canonical_snapshot);
        // Issue #901 第九轮阻塞5: Arc::clone 共享 visual_lines，不 clone Vec<VisualLine>。
        // cache 和 last_snapshot 通过 Arc 共享同一份视觉行数据。
        let cache_lines = Arc::clone(
            &self
                .last_snapshot
                .as_ref()
                .map(|s| &s.visual_lines)
                .unwrap_or(&Arc::new(PersistentChunkedVec::new())),
        );

        self.cache = Some(LayoutSnapshot {
            text_revision,
            text_ptr,
            text_len,
            width: promoted.width,
            font_size: promoted.font_size,
            font_family: promoted.font_family,
            line_spacing: promoted.line_spacing,
            text_indent: promoted.text_indent,
            padding: promoted.padding,
            lines: cache_lines,
            paragraph_offset_index: self.last_snapshot.as_ref()
                .map(|s| s.paragraph_offset_index.clone())
                .unwrap_or_default(),
            height_index: Arc::clone(&self.height_index),
            layout_generation: promoted.generation,
            // Issue #901 第十九轮问题1: 从 canonical snapshot 继承 ParagraphId → slot 映射。
            // Issue #901 第十九轮问题5: Arc 共享，O(1) clone 而非 O(段落数)。
            paragraph_id_to_slot: self
                .last_snapshot
                .as_ref()
                .map(|s| std::sync::Arc::clone(&s.paragraph_id_to_slot))
                .unwrap_or_default(),
            // Issue #901 第二十二轮问题4: Arc::clone 共享，O(1)。
            paragraph_line_count_index: self
                .last_snapshot
                .as_ref()
                .map(|s| std::sync::Arc::clone(&s.paragraph_line_count_index))
                .unwrap_or_default(),
        });
    }

    pub fn snapshot(
        &mut self,
        text: &str,
        params: LayoutParams,
        text_revision: u64,
    ) -> &LayoutSnapshot {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();

        // Issue #901: 区分"排版参数变化"和"仅 text_revision 变化"。
        // - 排版参数变化（宽度/字体/字号/行距/缩进/内边距）→ 全局失效，全篇重排。
        // - 仅 text_revision 变化（普通正文编辑）→ 增量排版，只重排受影响段落。
        let cache = &self.cache;
        let params_changed = match cache {
            Some(c) => {
                (c.width - params.width).abs() > 0.1
                    || (c.font_size - params.font_size).abs() > 0.1
                    || c.font_family != params.font_family
                    || (c.line_spacing - params.line_spacing).abs() > 0.01
                    || (c.text_indent - params.text_indent).abs() > 0.1
                    || (c.padding - params.padding).abs() > 0.1
            }
            None => true,
        };

        let text_changed = match cache {
            Some(c) => {
                c.text_revision != text_revision
                    || c.text_ptr != text_ptr
                    || c.text_len != text_len
                    // Issue #901 第五轮阻塞4：如果有未消费的 last_edit_affected_range，
                    // 即使 text_revision + text_ptr + text_len 都相同也必须重排。
                    // 禁止靠指针和长度判断是否需要重排——等字节长度替换（如 IME
                    // 替换中文字符、同宽 ASCII 替换）text_ptr 和 text_len 可能不变，
                    // 但内容已变。last_edit_affected_range 由外部从 Core
                    // display_patches 提取后通过 set_last_edit_affected_range 设置，
                    // 在增量排版路径中被 take() 消费。
                    || self.last_edit_affected_range.is_some()
            }
            None => true,
        };

        if params_changed {
            // 排版参数变化 → 全局失效
            self.invalidate_params();
        }

        if params_changed {
            // Issue #901 第二轮复核问题 2: 排版参数变化时才清旧 generation 并分配新 generation。
            // 普通编辑（仅 text_changed）不清旧 generation —— 旧 QTextLayout/纹理要保留
            // 供不受影响段落复用。受影响段落用同一 generation 重排（覆盖旧 cache slot）。
            if self.current_generation != 0 {
                clear_layout_generation(self.current_generation);
            }
            self.current_generation = begin_layout_generation();
            self.cache = None;
        } else if text_changed {
            // 仅文本变化 → 增量排版。
            // 不清旧 generation：保留旧 QTextLayout 供不受影响段落复用。
            // 不分配新 generation：受影响段落用当前 generation 重排（覆盖旧 cache slot）。
            // 旧 generation 在下次参数变化时由 invalidate_params 清理。
            self.cache = None;
        }

        // Issue #901: 不用 get_or_insert_with（闭包不能同时借用 self 的多个字段），
        // 改为先检查 cache 是否存在，不存在时在闭包外计算。
        if self.cache.is_some() {
            // cache 已存在，直接返回
            return self.cache.as_ref().unwrap();
        }

        // Issue #901: 增量排版路径。
        // 当 paragraph_store 非空（有缓存的排版结果）且 text_revision 变化时，
        // 走增量排版：只重排受影响段落，其他段落复用已有 shaping/行数据/纹理。
        //
        // 当 paragraph_store 为空（初始化/参数变化后 invalidate）时，
        // 走全篇重排（prepare_document_visual_snapshot）。
        let store_has_cache = !self.paragraph_store.is_empty();
        let current_gen = self.current_generation;

        let new_snapshot = if store_has_cache {
            // Issue #901: 增量排版路径
            let old_para_count = self.paragraph_store.paragraph_count();

            // Issue #901 第三轮复核: 优先消费 Core display_patches 提供的受影响范围。
            // 消费后清空，避免下次无编辑信息时误用旧范围。
            // Issue #901 第二十五轮复核问题3: 当 last_edit_affected_range 存在时，
            // 不需要 text.split_inclusive('\n').collect() 全文切分来计算 affected range，
            // 直接用 Core 提供的范围，省去 O(全文) 切分。只有无 display_patches 时
            // 才全文切分并按段落 hash 比较。
            let (affected_start, affected_end, affected_text_range) =
                if let Some(range) = self.last_edit_affected_range.take() {
                    // Issue #901 第二十六轮复核缺口4: Core 提供的受影响范围同时
                    // 传给 prepare_incremental_visual_snapshot 用于增量拼接 new_paras。
                    let (s, e) = range;
                    (s, e, Some((s, e)))
                } else {
                    // 无 display_patches 时回退到按段落 hash 比较，需要全文切分
                    let new_paras: Vec<&str> = text.split_inclusive('\n').collect();
                    let (s, e) = self.compute_affected_byte_range(text, &new_paras, old_para_count);
                    (s, e, None)
                };

            // Issue #901 第三轮复核问题 1: 用 mem::take 取走 last_snapshot 的所有权，
            // 避免 clone 整篇 CanonicalDocumentVisualSnapshot（含 paragraphs/images）。
            // 函数只需 &previous_snapshot 引用，take 后在函数返回前 prev_snapshot 持有所有权。
            let prev_snapshot = std::mem::take(&mut self.last_snapshot);

            let doc_snapshot = prepare_incremental_visual_snapshot(
                text,
                text_revision,
                f64::from(params.font_size),
                &params.font_family,
                f64::from(params.line_spacing),
                f64::from(params.padding),
                f64::from(params.text_indent),
                params.width,
                1.0,
                None,
                current_gen,
                affected_start,
                affected_end,
                affected_text_range,
                prev_snapshot,
                &mut self.paragraph_store,
                &mut self.height_index,
            );

            // Issue #901 第三轮复核问题 1: 只 clone visual_lines 给 LayoutSnapshot，
            // 把整个 doc_snapshot move 到 last_snapshot（避免 clone paragraphs/images）。
            // 此前是 clone 整个 doc_snapshot 再 move visual_lines，多 clone 了 paragraphs。
            //
            // Issue #901 第九轮阻塞5: Arc::clone 共享 visual_lines，不 clone Vec<VisualLine>。
            // LayoutSnapshot.lines 和 last_snapshot.visual_lines 通过 Arc 共享同一份数据。
            // 下次 snapshot() 时 self.cache=None 释放 Arc 引用，mem::take 取走 last_snapshot
            // 的 Arc（refcount==1），prepare_incremental_visual_snapshot 内部 Arc::make_mut
            // 原地修改，不产生 Vec clone。
            let lines = Arc::clone(&doc_snapshot.visual_lines);
            let paragraph_offset_index = doc_snapshot.paragraph_offset_index.clone();
            // Issue #901 第十九轮问题1: Arc 共享映射供 LayoutSnapshot 使用。
            // Issue #901 第十九轮问题5: Arc 共享，O(1) clone 而非 O(段落数)。
            let paragraph_id_to_slot = std::sync::Arc::clone(&doc_snapshot.paragraph_id_to_slot);
            // Issue #901 第二十二轮问题4: Arc::clone 共享，O(1)。
            let paragraph_line_count_index = std::sync::Arc::clone(&doc_snapshot.paragraph_line_count_index);
            self.last_snapshot = Some(doc_snapshot);

            LayoutSnapshot {
                text_revision,
                text_ptr,
                text_len,
                width: params.width,
                font_size: params.font_size,
                font_family: params.font_family,
                line_spacing: params.line_spacing,
                text_indent: params.text_indent,
                padding: params.padding,
                lines,
                paragraph_offset_index,
                // Issue #901 第二十九轮复核阻塞4: Arc::clone 共享，O(1) 而非 O(段落数)。
                height_index: Arc::clone(&self.height_index),
                layout_generation: current_gen,
                paragraph_id_to_slot,
                paragraph_line_count_index,
            }
        } else {
            // 全篇重排路径（初始化/参数变化后 invalidate 清空了 store）
            // Issue #658 评论 5622829886 问题 2: 静态正文只做基础排版。
            // Issue #901 第二十轮阻塞2: 先重建 paragraph_store 获取 ParagraphId 序列，
            // 再传入 prepare_document_visual_snapshot，不再用 slot+1 伪造 Id。
            self.paragraph_store.rebuild_from_text(
                text,
                f64::from(params.font_size),
                &params.font_family,
                f64::from(params.line_spacing),
                f64::from(params.text_indent),
                f64::from(params.padding),
                params.width,
            );
            let paragraph_ids: Vec<ParagraphId> = self.paragraph_store.paragraph_ids();
            let doc_snapshot = prepare_document_visual_snapshot(
                text,
                text_revision,
                f64::from(params.font_size),
                &params.font_family,
                f64::from(params.line_spacing),
                f64::from(params.padding),
                f64::from(params.text_indent),
                params.width,
                1.0,
                current_gen,
                false,
                &paragraph_ids,
            );

            // Issue #901: 全篇重排后同步 paragraph_store 和 height_index
            let doc_vls = doc_snapshot.visual_lines.to_vec();
            self.paragraph_store
                .sync_from_visual_lines(&doc_vls, current_gen);
            let heights = self.paragraph_store.paragraph_heights();
            Arc::make_mut(&mut self.height_index).rebuild(heights);

            // Issue #901 第二轮复核: 保存全篇重排结果供下次增量排版复用。
            // Issue #901 第三轮复核: 只 clone visual_lines，move 整个 doc_snapshot 到 last_snapshot。
            // Issue #901 第九轮阻塞5: Arc::clone 共享 visual_lines，不 clone Vec<VisualLine>。
            let lines = Arc::clone(&doc_snapshot.visual_lines);
            let paragraph_offset_index = doc_snapshot.paragraph_offset_index.clone();
            // Issue #901 第十九轮问题1: Arc 共享映射供 LayoutSnapshot 使用。
            // Issue #901 第十九轮问题5: Arc 共享，O(1) clone 而非 O(段落数)。
            let paragraph_id_to_slot = std::sync::Arc::clone(&doc_snapshot.paragraph_id_to_slot);
            // Issue #901 第二十二轮问题4: Arc::clone 共享，O(1)。
            let paragraph_line_count_index = std::sync::Arc::clone(&doc_snapshot.paragraph_line_count_index);
            self.last_snapshot = Some(doc_snapshot);

            LayoutSnapshot {
                text_revision,
                text_ptr,
                text_len,
                width: params.width,
                font_size: params.font_size,
                font_family: params.font_family,
                line_spacing: params.line_spacing,
                text_indent: params.text_indent,
                padding: params.padding,
                lines,
                paragraph_offset_index,
                // Issue #901 第二十九轮复核阻塞4: Arc::clone 共享，O(1) 而非 O(段落数)。
                height_index: Arc::clone(&self.height_index),
                layout_generation: current_gen,
                paragraph_id_to_slot,
                paragraph_line_count_index,
            }
        };

        self.cache = Some(new_snapshot);
        self.cache.as_ref().unwrap()
    }

    /// Issue #901: 计算受影响字节范围。
    ///
    /// 比较 `paragraph_store` 中已缓存的段落信息和 new text 的段落，
    /// 确定哪些段落的文本内容发生了变化，返回受影响的字节范围。
    ///
    /// Issue #901 第二轮复核问题 4: 段落数量变化（换行拆段/删回车合段）时，
    /// 只返回拆/并段及受影响范围，不把 affected 设成整个 text 范围。
    /// 后续段只调整 y 坐标（由 height_index 前缀和计算），不需要重排。
    fn compute_affected_byte_range(
        &self,
        _text: &str,
        new_paras: &[&str],
        _old_para_count: usize,
    ) -> (usize, usize) {
        // 逐段比较，找内容不同的段落范围。
        // 段落数变化时也只返回差异范围，不返回整个文本。
        let mut byte_start = 0usize;
        let mut affected_start: Option<usize> = None;
        let mut affected_end: Option<usize> = None;

        for (i, para) in new_paras.iter().enumerate() {
            let para_text = para.trim_end_matches('\n');
            let para_end = byte_start + para.len();

            // 从 store 获取旧段落的 cache_key，与新段落的 cache_key 比较
            if let Some(entry) = self.paragraph_store.entry(i) {
                let new_key = ParagraphLayoutCacheKey::build(
                    para_text,
                    // 使用 entry 中已有的排版参数构建 key 进行比较
                    // cache_key 中包含了排版参数，如果参数变了 key 自然不同
                    // 但参数变化已经由 invalidate_params 处理，此处参数一定没变
                    // 用 entry.cache_key 中的参数（通过重建 key 比较 text_hash）
                    // 简化：直接比较 paragraph_text_len 和 cache_key
                    0.0, // placeholder，实际用 text_hash 比较
                    "",
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                );
                // 比较文本长度和内容 hash
                let text_changed = entry.paragraph_text_len != para_text.len()
                    || entry.cache_key.text_hash() != new_key.text_hash();

                if text_changed {
                    if affected_start.is_none() {
                        affected_start = Some(byte_start);
                    }
                    affected_end = Some(para_end);
                }
            } else {
                // store 中没有该段落（新增段），需要重排
                if affected_start.is_none() {
                    affected_start = Some(byte_start);
                }
                affected_end = Some(para_end);
            }

            byte_start = para_end;
        }

        match (affected_start, affected_end) {
            (Some(s), Some(e)) => (s, e),
            _ => (0, 0), // 无变化
        }
    }

    pub fn hit_test(
        &self,
        snapshot: &LayoutSnapshot,
        x: f64,
        y: f64,
        scroll_y: f64,
    ) -> (usize, CaretAffinity) {
        hit_test(snapshot, x, y, scroll_y)
    }

    pub fn caret_rect(
        &self,
        snapshot: &LayoutSnapshot,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        caret_rect(snapshot, cursor_byte, affinity, scroll_y, viewport_h)
    }

    /// Issue #722 评论 5748596920 问题1: canonical caret 文档坐标入口。
    ///
    /// 与 `caret_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标（不减 scroll_y），
    /// `visible` 始终为 true（文档坐标版本不关心视口可见性）。
    /// 供正文事务 `record_visual_transaction` 构造 old/new caret track 使用，
    /// 使 caret track 与 AnimatedSlice/StaticPatch 文档坐标系一致；
    /// scene graph 在渲染时按当前 scroll_y 做 viewport transform。
    pub fn caret_rect_doc(
        &self,
        snapshot: &LayoutSnapshot,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> CaretRect {
        caret_rect_doc(snapshot, cursor_byte, affinity)
    }

    pub fn cursor_line_and_x(
        &self,
        snapshot: &LayoutSnapshot,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> Option<(usize, f64)> {
        cursor_line_and_x(snapshot, cursor, affinity)
    }

    pub fn index_at_line_x(&self, snapshot: &LayoutSnapshot, line: &VisualLine, x: f64) -> usize {
        index_at_line_x(snapshot, line, x)
    }

    pub fn cursor_x_for_line(
        &self,
        snapshot: &LayoutSnapshot,
        line: &VisualLine,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        calculate_cursor_x_for_line(line, cursor, affinity, snapshot)
    }

    /// Issue #901 问题4: 获取上一次排版产生的 `CanonicalDocumentVisualSnapshot` 的只读引用。
    ///
    /// `snapshot()` 在增量排版路径中会把结果存入 `last_snapshot`。
    /// 普通编辑路径通过此方法获取 canonical snapshot，用于构建 `EditorLayoutSnapshot`
    ///（含 QImage/cluster），不需要再走 `build_editor_layout_snapshot` 的全篇排版路径。
    ///
    /// 返回 `None` 表示尚未排版（初始化/参数变化后 invalidate）。
    pub fn last_canonical_snapshot(
        &self,
    ) -> Option<&CanonicalDocumentVisualSnapshot> {
        self.last_snapshot.as_ref()
    }

    /// Issue #748: 用 QFontMetricsF::horizontalAdvance 测量文本宽度（纯 QFont 测量），
    /// 不创建 QTextLayout，符合"正式路径只从 PreparedLayoutHandle/QTextLine 走"的要求。
    /// 供 preedit/IME 文本宽度测量，不进入排版生命周期。
    pub fn text_width(&self, text: &str, font_size: f64, font_family: &str) -> f64 {
        engine::text_width(text, font_size, font_family)
    }

    pub fn affinity_for_index_on_line(&self, line: &VisualLine, index: usize) -> CaretAffinity {
        affinity_for_index_on_line(line, index)
    }
}
