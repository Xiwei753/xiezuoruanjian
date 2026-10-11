//! 段落布局存储 — 稳定段落身份与布局缓存。
//!
//! Issue #901: 增量排版子系统。
//!
//! # 核心设计
//!
//! - [`ParagraphId`] 是稳定段落身份，不随 byte offset 变化。
//! - [`ParagraphLayoutCacheKey`] 由文本内容 + 格式 + 字体 + 宽度的哈希构成，
//!   用于判断段落是否需要重新走 QTextLayout。
//! - 普通中间编辑只变更所在段及实际连带范围的 cache_key。
//! - 换行拆段：原段保留 Id，新段分配新 Id。
//! - 删回车合段：被合并段的 Id 失效，保留段保持原 Id。
//! - 不能以随编辑移动的 byte offset 当稳定对象 identity。
//!
//! # 线程安全
//!
//! 与 `EditorLayout` 相同，仅在 GUI 线程中使用，不跨线程共享。

use std::hash::{Hash, Hasher};

use super::types::VisualLine;

// ── 稳定段落身份 ──

/// 稳定段落身份 — 不随 byte offset 变化的段落唯一标识。
///
/// 每次创建新段落（换行拆段、粘贴新段、加载文档）时分配一个新的 `ParagraphId`。
/// 段落内容编辑不改变 `ParagraphId`。
/// 删回车合段时，被合并的段落的 `ParagraphId` 失效，保留的段落保持原 Id。
///
/// **不能**以随编辑移动的 byte offset 当稳定对象 identity ——
/// 在段前插入文本会让所有后续段落的 byte offset 移动，但它们的 `ParagraphId` 不变。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParagraphId(u64);

impl ParagraphId {
    /// 占位 Id，表示无效/未分配。
    pub const INVALID: ParagraphId = ParagraphId(0);

    /// 返回内部数值，仅供测试/调试使用。
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

// ── 布局缓存 key ──

/// 布局缓存 key — 文本内容 + 格式 + 字体 + 宽度的哈希。
///
/// 用于判断段落是否需要重新走 QTextLayout。
/// - key 相同 → shaping/行数据/纹理可复用。
/// - key 不同 → 需要重新排版。
///
/// 字段全部是整数/哈希值，可直接用 `==` 比较，无需浮点容差。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParagraphLayoutCacheKey {
    text_hash: u64,
    font_size_bits: u64,
    font_family_hash: u64,
    line_spacing_bits: u64,
    text_indent_bits: u64,
    padding_bits: u64,
    width_bits: u64,
}

impl ParagraphLayoutCacheKey {
    /// 从段落文本和排版参数构建缓存 key。
    ///
    /// `paragraph_text` 不含尾部 `'\n'`。
    /// 浮点参数用 `f64::to_bits()` 转为可 `==` 比较的整数，
    /// 避免浮点容差问题 —— 排版参数变化时 key 必然不同。
    pub fn build(
        paragraph_text: &str,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
    ) -> Self {
        let text_hash = hash_str(paragraph_text);
        let font_family_hash = hash_str(font_family);

        Self {
            text_hash,
            font_size_bits: font_size.to_bits(),
            font_family_hash,
            line_spacing_bits: line_spacing.to_bits(),
            text_indent_bits: text_indent.to_bits(),
            padding_bits: padding.to_bits(),
            width_bits: width.to_bits(),
        }
    }

    /// 快速判断两个 key 是否匹配（内容 + 格式 + 宽度全相同）。
    pub fn matches(&self, other: &ParagraphLayoutCacheKey) -> bool {
        self == other
    }

    /// Issue #901: 返回文本内容的哈希值，用于快速判断段落文本是否变化。
    pub fn text_hash(&self) -> u64 {
        self.text_hash
    }
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ── 段落布局缓存条目 ──

/// 单个段落的布局缓存条目。
///
/// 存储段落的稳定身份、布局缓存 key、QTextLayout generation 引用
/// 和视觉行缓存引用。
#[derive(Clone, Debug)]
pub struct ParagraphLayoutEntry {
    /// 段落稳定身份（不随 byte offset 变化）
    pub id: ParagraphId,
    /// 布局缓存 key（文本内容 + 格式 + 字体 + 宽度的哈希）
    pub cache_key: ParagraphLayoutCacheKey,
    /// QTextLayout generation 引用 — 对应 `g_layout_generations` 中的代。
    /// 0 表示尚未排版。
    pub layout_generation: u64,
    /// 该段落在 generation cache 中的 slot 索引。
    /// -1 表示尚未分配 slot。
    pub cache_slot: i32,
    /// 段落文档 byte 起始偏移（随编辑移动，**不作为 identity**）。
    pub document_byte_start: usize,
    /// 段落文本长度（UTF-8 byte，不含尾部 `'\n'`）。
    pub paragraph_text_len: usize,
    /// 段落总高度（所有视觉行高度之和）。
    pub height: f64,
    /// 段落内视觉行数。
    pub visual_line_count: usize,
    /// 该段落的视觉行引用（在 `LayoutSnapshot.lines` 中的索引范围 `[line_start, line_start + visual_line_count)`）。
    pub line_start: usize,
}

impl ParagraphLayoutEntry {
    /// 该段落是否已排版（有有效的 generation 和 slot）。
    pub fn is_laid_out(&self) -> bool {
        self.layout_generation != 0 && self.cache_slot >= 0
    }
}

// ── 受影响段落范围 ──

/// 增量排版需要重排的段落范围。
///
/// 由 [`ParagraphLayoutStore::compute_affected_paragraphs`] 计算。
/// 只有这些段落的 cache_key 变化或身份变化，需要重新走 QTextLayout。
/// 其他段落的 shaping/行数据/纹理可复用。
#[derive(Clone, Debug)]
pub struct AffectedParagraphRanges {
    /// 需要重新排版的段落索引范围 `[start, end)`（文档顺序，0-indexed）。
    pub relayout_start: usize,
    pub relayout_end: usize,
    /// 是否需要全局重排（宽度/字体/字号/行距/全局格式变化时为 true）。
    pub full_relayout: bool,
}

/// Issue #901 第五轮阻塞2: 旧段 splice 范围与新段建立范围。
///
/// Enter 拆段（旧 1 段→新 2 段）和 Backspace 合段（旧 2 段→新 1 段）时，
/// **旧删段数不等于新添段数**，不能用同一 `affected_start..affected_end`
/// 同时 splice 旧段数组和新段数组。
///
/// 此结构分别保存：
/// - `old_splice_start..old_splice_end`：在旧段数组中要删除的范围。
/// - `new_layout_start..new_layout_end`：在新段数组中要建立（重排版）的范围。
///
/// # 不变量
///
/// - `old_splice_start == new_layout_start`：编辑点之前的段落数不变。
/// - `old_count - old_splice_end == new_count - new_layout_end`：编辑点之后的段落数不变。
/// - 前 `old_splice_start` 段和后 `old_count - old_splice_end` 段内容完全相同，
///   可直接复用 prev snapshot 的排版结果。
#[derive(Clone, Debug)]
pub struct ParagraphSpliceRanges {
    /// 旧段数组中要删除的范围 `[start, end)`。
    pub old_splice_start: usize,
    pub old_splice_end: usize,
    /// 新段数组中要建立（重排版）的范围 `[start, end)`。
    pub new_layout_start: usize,
    pub new_layout_end: usize,
}

// ── 段落布局存储 ──

/// 段落布局存储 — 管理所有段落的稳定身份和布局缓存。
///
/// # 核心不变量
///
/// 1. `ParagraphId` 在段落生命周期内不变，不随 byte offset 移动。
/// 2. 普通中间编辑只变更所在段及实际连带范围的 `cache_key`。
/// 3. 换行拆段：原段保留 Id，新段分配新 Id。
/// 4. 删回车合段：被合并段的 Id 失效，保留段保持原 Id。
/// 5. `entries` 按文档顺序排列，索引即段落在文档中的顺序。
///
/// # 增量更新策略
///
/// - **普通编辑**（不涉及 `'\n'`）：只更新所在段落的 `cache_key`，
///   其他段落不变。
/// - **换行拆段**（插入 `'\n'`）：原段保留 Id，在原段后插入新段（分配新 Id）。
/// - **删回车合段**（删除 `'\n'`）：移除后一段，将其内容并入前一段，
///   保留前一段 Id，丢弃后一段 Id。
/// - **宽度/字体/字号/行距变化**：全局失效，所有段落的 `cache_key` 失效。
#[derive(Clone, Debug, Default)]
pub struct ParagraphLayoutStore {
    /// 按文档顺序排列的段落缓存条目。
    entries: Vec<ParagraphLayoutEntry>,
    /// 下一个可分配的 `ParagraphId` 内部数值（从 1 开始）。
    next_id: u64,
}

impl ParagraphLayoutStore {
    /// 创建空的段落布局存储。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// 分配一个新的 `ParagraphId`。
    fn allocate_id(&mut self) -> ParagraphId {
        let id = ParagraphId(self.next_id);
        self.next_id += 1;
        id
    }

    /// 段落数量。
    pub fn paragraph_count(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空所有缓存（全局失效）。
    pub fn clear(&mut self) {
        self.entries.clear();
        // next_id 不重置，避免新分配的 Id 与已释放的 Id 冲突。
    }

    /// 获取指定索引处段落的只读引用。
    pub fn entry(&self, index: usize) -> Option<&ParagraphLayoutEntry> {
        self.entries.get(index)
    }

    /// 获取指定索引处段落的可变引用。
    pub fn entry_mut(&mut self, index: usize) -> Option<&mut ParagraphLayoutEntry> {
        self.entries.get_mut(index)
    }

    /// 按 `ParagraphId` 查找段落索引。
    pub fn paragraph_index_by_id(&self, id: ParagraphId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    /// 获取指定索引处段落的 `ParagraphId`。
    pub fn paragraph_id_at(&self, index: usize) -> Option<ParagraphId> {
        self.entries.get(index).map(|e| e.id)
    }

    // ── 全文重建 ──

    /// 从全文文本重建段落身份和缓存 key。
    ///
    /// 用于初始化、文档加载或全局参数变化（宽度/字体/字号/行距）。
    /// 已有段落的 `ParagraphId` 尽量保留（按文档顺序对齐），
    /// 新段落分配新 Id，多余段落的 Id 丢弃。
    ///
    /// Issue #901 第二轮复核: 保留未变化段的排版结果
    /// （layout_generation/cache_slot/height/visual_line_count/line_start）。
    /// 只有 cache_key 变化的段落（内容或参数变了）才重置排版结果，
    /// 需要重新走 QTextLayout。内容相同的段落保留旧 shaping/行数据/纹理，
    /// 不重建全部 QTextLayout/静态 QSGTextNode。
    pub fn rebuild_from_text(
        &mut self,
        text: &str,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
    ) {
        let paragraphs: Vec<&str> = text.split_inclusive('\n').collect();
        let mut new_entries: Vec<ParagraphLayoutEntry> = Vec::with_capacity(paragraphs.len());
        let mut byte_offset = 0usize;

        for (i, paragraph) in paragraphs.iter().enumerate() {
            let paragraph_text = paragraph.trim_end_matches('\n');
            let cache_key = ParagraphLayoutCacheKey::build(
                paragraph_text,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            );

            // Issue #901 第二轮复核: 尽量保留已有 Id 和排版结果。
            // 如果旧 entry 的 cache_key 与新 cache_key 相同（内容+参数没变），
            // 保留 layout_generation/cache_slot/height/visual_line_count/line_start，
            // 该段落不需要重新走 QTextLayout。
            // 只有内容变化的段落才重置排版结果。
            let (
                id,
                layout_generation,
                cache_slot,
                height,
                visual_line_count,
                line_start,
            ) = if i < self.entries.len() {
                let old = &self.entries[i];
                if old.cache_key == cache_key {
                    // 内容相同，保留 Id 和排版结果
                    (
                        old.id,
                        old.layout_generation,
                        old.cache_slot,
                        old.height,
                        old.visual_line_count,
                        old.line_start,
                    )
                } else {
                    // 内容不同，保留 Id 但重置排版结果
                    (old.id, 0u64, -1, 0.0f64, 0usize, 0usize)
                }
            } else {
                // 新段落，分配新 Id
                (self.allocate_id(), 0u64, -1, 0.0f64, 0usize, 0usize)
            };

            new_entries.push(ParagraphLayoutEntry {
                id,
                cache_key,
                layout_generation,
                cache_slot,
                document_byte_start: byte_offset,
                paragraph_text_len: paragraph_text.len(),
                height,
                visual_line_count,
                line_start,
            });

            byte_offset += paragraph.len();
        }

        self.entries = new_entries;
    }

    // ── 增量更新：换行拆段 ──

    /// 换行拆段 — 在段落 `index` 的 `split_byte_offset` 处插入 `'\n'`。
    ///
    /// 原段保留 `ParagraphId`，在 `index + 1` 处插入新段（分配新 Id）。
    /// 两段的 `cache_key` 都需要重新计算（内容变了）。
    ///
    /// `split_byte_offset` 是相对于段落文本开头的 byte offset（不含 `'\n'`）。
    ///
    /// 返回 `(原段索引, 新段索引)`，失败返回 `None`。
    pub fn split_paragraph(
        &mut self,
        index: usize,
        split_byte_offset: usize,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
        original_text_before_split: &str,
        original_text_after_split: &str,
    ) -> Option<(usize, usize)> {
        if index >= self.entries.len() {
            return None;
        }

        let new_id = self.allocate_id();
        let original_entry = &mut self.entries[index];

        // 更新原段（split 点之前的部分）
        let original_doc_start = original_entry.document_byte_start;
        original_entry.cache_key = ParagraphLayoutCacheKey::build(
            original_text_before_split,
            font_size,
            font_family,
            line_spacing,
            text_indent,
            padding,
            width,
        );
        original_entry.layout_generation = 0;
        original_entry.cache_slot = -1;
        original_entry.paragraph_text_len = original_text_before_split.len();
        original_entry.height = 0.0;
        original_entry.visual_line_count = 0;

        // 新段（split 点之后的部分）
        let new_doc_start = original_doc_start + split_byte_offset + 1; // +1 for '\n'
        let new_entry = ParagraphLayoutEntry {
            id: new_id,
            cache_key: ParagraphLayoutCacheKey::build(
                original_text_after_split,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            ),
            layout_generation: 0,
            cache_slot: -1,
            document_byte_start: new_doc_start,
            paragraph_text_len: original_text_after_split.len(),
            height: 0.0,
            visual_line_count: 0,
            line_start: 0,
        };

        self.entries.insert(index + 1, new_entry);
        Some((index, index + 1))
    }

    // ── 增量更新：删回车合段 ──

    /// 删回车合段 — 删除段落 `index` 和 `index + 1` 之间的 `'\n'`，
    /// 将 `index + 1` 的内容并入 `index`。
    ///
    /// 保留段落 `index` 的 `ParagraphId`，丢弃段落 `index + 1` 的 Id。
    /// 合并后段落的 `cache_key` 需要重新计算。
    ///
    /// 返回合并后的段落索引，失败返回 `None`。
    pub fn merge_paragraphs(
        &mut self,
        index: usize,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
        merged_text: &str,
    ) -> Option<usize> {
        if index + 1 >= self.entries.len() {
            return None;
        }

        let entry = &mut self.entries[index];
        entry.cache_key = ParagraphLayoutCacheKey::build(
            merged_text,
            font_size,
            font_family,
            line_spacing,
            text_indent,
            padding,
            width,
        );
        entry.layout_generation = 0;
        entry.cache_slot = -1;
        entry.paragraph_text_len = merged_text.len();
        entry.height = 0.0;
        entry.visual_line_count = 0;

        // 移除被合并的段落（index + 1）
        self.entries.remove(index + 1);
        Some(index)
    }

    // ── 增量更新：普通编辑 ──

    /// 普通编辑 — 更新段落 `index` 的文本内容（不涉及 `'\n'`）。
    ///
    /// 只变更该段的 `cache_key`，其他段落不变。
    /// 段落 `ParagraphId` 不变。
    pub fn update_paragraph_text(
        &mut self,
        index: usize,
        new_text: &str,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        let new_key = ParagraphLayoutCacheKey::build(
            new_text,
            font_size,
            font_family,
            line_spacing,
            text_indent,
            padding,
            width,
        );
        let changed = entry.cache_key != new_key;
        entry.cache_key = new_key;
        if changed {
            entry.layout_generation = 0;
            entry.cache_slot = -1;
            entry.paragraph_text_len = new_text.len();
            entry.height = 0.0;
            entry.visual_line_count = 0;
        }
        changed
    }

    /// 更新段落 `index` 的文档 byte 起始偏移。
    ///
    /// 段前插入/删除文本时，后续段落的 `document_byte_start` 需要平移。
    /// **不改变** `ParagraphId` 和 `cache_key`。
    pub fn shift_document_byte_start(&mut self, index: usize, new_start: usize) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.document_byte_start = new_start;
        true
    }

    // ── 增量更新：段落插入/删除 ──

    /// 在 `index` 处插入新段落（分配新 Id）。
    pub fn insert_paragraph(
        &mut self,
        index: usize,
        text: &str,
        document_byte_start: usize,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
    ) {
        let id = self.allocate_id();
        let entry = ParagraphLayoutEntry {
            id,
            cache_key: ParagraphLayoutCacheKey::build(
                text,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            ),
            layout_generation: 0,
            cache_slot: -1,
            document_byte_start,
            paragraph_text_len: text.len(),
            height: 0.0,
            visual_line_count: 0,
            line_start: 0,
        };
        self.entries.insert(index.min(self.entries.len()), entry);
    }

    /// 删除段落 `index`（丢弃其 Id）。
    pub fn remove_paragraph(&mut self, index: usize) -> Option<ParagraphLayoutEntry> {
        if index < self.entries.len() {
            Some(self.entries.remove(index))
        } else {
            None
        }
    }

    // ── 排版结果更新 ──

    /// 更新段落 `index` 的排版结果。
    ///
    /// 排版完成后，记录 generation、cache_slot、高度、视觉行数等信息。
    pub fn update_layout_result(
        &mut self,
        index: usize,
        layout_generation: u64,
        cache_slot: i32,
        height: f64,
        visual_line_count: usize,
        line_start: usize,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.layout_generation = layout_generation;
        entry.cache_slot = cache_slot;
        entry.height = height;
        entry.visual_line_count = visual_line_count;
        entry.line_start = line_start;
        true
    }

    // ── 判断是否需要重排 ──

    /// 判断段落 `index` 是否需要重新排版。
    ///
    /// 需要重排的条件：
    /// - 尚未排版（`layout_generation == 0` 或 `cache_slot < 0`）
    /// - 给定的缓存 key 与当前 key 不匹配
    pub fn needs_relayout(&self, index: usize, expected_key: &ParagraphLayoutCacheKey) -> bool {
        match self.entries.get(index) {
            Some(entry) => !entry.is_laid_out() || entry.cache_key != *expected_key,
            None => true,
        }
    }

    /// Issue #901: 根据新旧文本差异确定受影响段落索引列表。
    ///
    /// 比较新旧文本的段落分割（按 `'\n'` 分段），找出内容不同的段落索引。
    /// 用于增量排版：只有这些段落需要重新走 QTextLayout，其他段落可复用。
    ///
    /// # 算法
    ///
    /// 1. 将 `old_text` 和 `new_text` 按 `'\n'` 分段。
    /// 2. 逐段比较文本内容，内容不同的段落标记为受影响。
    /// 3. 段落数量变化（换行拆段/删回车合段）时，从第一个变化点起所有后续段落
    ///    都标记为受影响（因为 byte offset 全部漂移）。
    ///
    /// # 返回
    ///
    /// 受影响段落索引列表（按升序排列）。空列表表示无变化（old_text == new_text）。
    pub fn affected_paragraphs(old_text: &str, new_text: &str) -> Vec<usize> {
        if old_text == new_text {
            return Vec::new();
        }

        let old_paras: Vec<&str> = old_text.split_inclusive('\n').collect();
        let new_paras: Vec<&str> = new_text.split_inclusive('\n').collect();

        let mut affected: Vec<usize> = Vec::new();
        let min_len = old_paras.len().min(new_paras.len());

        // 逐段比较段落文本内容（去掉尾部换行符），找第一个不同的段落
        let mut first_diff: Option<usize> = None;
        for i in 0..min_len {
            // 比较段落文本内容，忽略尾部换行符差异
            //（末尾换行符变化只是因为后面增删了段落，不影响本段 shaping）
            let old_text = old_paras[i].trim_end_matches('\n');
            let new_text = new_paras[i].trim_end_matches('\n');
            if old_text != new_text {
                first_diff = Some(i);
                break;
            }
        }

        match first_diff {
            None => {
                // 前 min_len 段内容全相同，差异只在段落数量（末尾增删段）
                // 新增/删除的段落都需要重排
                if new_paras.len() > old_paras.len() {
                    // 新增了段落
                    for i in old_paras.len()..new_paras.len() {
                        affected.push(i);
                    }
                }
                // 如果段落数量减少，旧多出的段落已不存在，不需要标记
            }
            Some(diff_idx) => {
                // 从第一个差异段起，该段及所有后续段都可能受影响
                //（byte offset 漂移，但段落内容可能没变 —— 仍需重排以更新坐标）
                for i in diff_idx..new_paras.len() {
                    affected.push(i);
                }
            }
        }

        affected
    }

    /// Issue #901 第五轮阻塞2: 通过段落文本比较计算旧段 splice 范围和新段建立范围。
    ///
    /// Enter 拆段（旧 1 段→新 2 段）和 Backspace 合段（旧 2 段→新 1 段）时，
    /// 旧删段数不等于新添段数。此方法通过逐段比较新旧段落文本，分别找出：
    /// - 前缀公共段（内容相同，直接复用 prev 排版）
    /// - 后缀公共段（内容相同，直接复用 prev 排版）
    /// - 中间差异段（旧段要删除，新段要建立）
    ///
    /// # 算法
    ///
    /// 1. 从前扫描找前缀公共段数 `prefix`。
    /// 2. 从后扫描找后缀公共段数 `suffix`（在前缀之后的部分中找）。
    /// 3. `old_splice = [prefix, old_count - suffix)`
    ///    `new_layout = [prefix, new_count - suffix)`
    ///
    /// # 复杂度
    ///
    /// O(n)（n 为段落数），但只在段落数变化或内容变化时调用，
    /// 段落数远小于行数，可接受。
    pub fn compute_old_new_paragraph_ranges(
        old_para_texts: &[&str],
        new_para_texts: &[&str],
    ) -> ParagraphSpliceRanges {
        let old_count = old_para_texts.len();
        let new_count = new_para_texts.len();

        // 找前缀公共段
        let mut prefix_common = 0usize;
        let min_len = old_count.min(new_count);
        for i in 0..min_len {
            if old_para_texts[i] == new_para_texts[i] {
                prefix_common = i + 1;
            } else {
                break;
            }
        }

        // 找后缀公共段（在前缀之后的部分中找）
        let mut suffix_common = 0usize;
        let remaining_old = old_count - prefix_common;
        let remaining_new = new_count - prefix_common;
        let max_suffix = remaining_old.min(remaining_new);
        while suffix_common < max_suffix {
            let old_idx = old_count - 1 - suffix_common;
            let new_idx = new_count - 1 - suffix_common;
            if old_para_texts[old_idx] == new_para_texts[new_idx] {
                suffix_common += 1;
            } else {
                break;
            }
        }

        ParagraphSpliceRanges {
            old_splice_start: prefix_common,
            old_splice_end: old_count - suffix_common,
            new_layout_start: prefix_common,
            new_layout_end: new_count - suffix_common,
        }
    }

    /// Issue #901 第五轮阻塞2: 增量更新段落文本 — 只更新新段范围内段落的 cache_key，
    /// 不用 `rebuild_from_text` 全局重建。保留旧段 Id 和排版结果。
    ///
    /// 用于 `prepare_incremental_visual_snapshot_impl` 中段落数不变但内容变化的场景。
    /// 对 `new_layout_start..new_layout_end` 范围内的段落调用 `update_paragraph_text`，
    /// 其他段落不变。
    pub fn update_affected_paragraph_texts(
        &mut self,
        new_para_texts: &[&str],
        new_layout_start: usize,
        new_layout_end: usize,
        font_size: f64,
        font_family: &str,
        line_spacing: f64,
        text_indent: f64,
        padding: f64,
        width: f64,
    ) {
        for i in new_layout_start..new_layout_end.min(new_para_texts.len()) {
            if i < self.entries.len() {
                self.update_paragraph_text(
                    i,
                    new_para_texts[i],
                    font_size,
                    font_family,
                    line_spacing,
                    text_indent,
                    padding,
                    width,
                );
            }
        }
    }

    /// Issue #901: 增量同步 — 只更新受影响段落的排版结果。
    ///
    /// 与 [`sync_from_visual_lines`] 的区别：只更新 `affected_indices` 中列出的段落，
    /// 不扫描全部视觉行。用于增量排版路径，避免 O(n) 全章扫描。
    ///
    /// Issue #901 第三轮复核: 用 para_start → (line_start, line_end, height) 索引
    /// 一次遍历 lines（O(all_lines)），代替每个受影响段落都扫描全部 lines
    ///（O(affected × all_lines)）。
    ///
    /// # 参数
    ///
    /// - `lines`：本次排版产生的全部视觉行（含复用 + 重排段落的行）。
    /// - `layout_generation`：本次排版的 generation。
    /// - `affected_indices`：受影响段落索引列表（由 [`affected_paragraphs`] 计算）。
    pub fn sync_from_visual_lines_incremental(
        &mut self,
        lines: &[VisualLine],
        layout_generation: u64,
        affected_indices: &[usize],
    ) {
        if affected_indices.is_empty() {
            return;
        }

        // Issue #901 第三轮复核: 一次遍历 lines，构建 para_start → 统计信息索引。
        // visual_lines 按段落顺序排列，para_start 变化时切换段落。
        use std::collections::HashMap;
        let mut para_stats: HashMap<usize, (usize, usize, f64)> = HashMap::new();
        // (first_line_index, line_count, total_height)
        if !lines.is_empty() {
            let mut current_para_start = lines[0].para_start;
            let mut first_idx = 0usize;
            let mut count = 0usize;
            let mut height = 0.0f64;
            for (i, line) in lines.iter().enumerate().skip(1) {
                if line.para_start != current_para_start {
                    para_stats.insert(current_para_start, (first_idx, count, height));
                    current_para_start = line.para_start;
                    first_idx = i;
                    count = 0;
                    height = 0.0;
                }
                count += 1;
                height += line.height;
            }
            para_stats.insert(current_para_start, (first_idx, count, height));
        }

        // 对每个受影响段落，O(1) 查表获取统计信息
        for &idx in affected_indices {
            let Some(entry) = self.entries.get(idx) else {
                continue;
            };
            let para_start = entry.document_byte_start;

            let (line_count, height, line_start) = if let Some(&(fs, cnt, h)) = para_stats.get(&para_start) {
                (cnt, h, fs)
            } else {
                (0usize, 0.0f64, 0usize)
            };

            if let Some(entry) = self.entries.get_mut(idx) {
                entry.layout_generation = layout_generation;
                entry.cache_slot = idx as i32;
                entry.visual_line_count = line_count;
                entry.height = height;
                entry.line_start = line_start;
            }
        }
    }

    /// 计算受影响段落范围。
    ///
    /// 根据编辑的 byte 范围确定哪些段落需要重排。
    /// 返回 [`AffectedParagraphRanges`]，只有这些段落需要重新走 QTextLayout。
    pub fn compute_affected_paragraphs(
        &self,
        affected_byte_start: usize,
        affected_byte_end: usize,
    ) -> AffectedParagraphRanges {
        if self.entries.is_empty() {
            return AffectedParagraphRanges {
                relayout_start: 0,
                relayout_end: 0,
                full_relayout: true,
            };
        }

        // 找到第一个与受影响范围相交的段落
        let start = self
            .entries
            .iter()
            .position(|e| {
                let seg_end = e.document_byte_start + e.paragraph_text_len;
                seg_end >= affected_byte_start && e.document_byte_start < affected_byte_end
            })
            .unwrap_or(0);

        // 找到最后一个与受影响范围相交的段落
        let end = self
            .entries
            .iter()
            .rposition(|e| {
                let seg_end = e.document_byte_start + e.paragraph_text_len;
                seg_end >= affected_byte_start && e.document_byte_start < affected_byte_end
            })
            .map(|i| i + 1)
            .unwrap_or(self.entries.len());

        AffectedParagraphRanges {
            relayout_start: start,
            relayout_end: end,
            full_relayout: false,
        }
    }

    /// 全局失效 — 标记所有段落需要重排。
    pub fn invalidate_all(&mut self) {
        for entry in &mut self.entries {
            entry.layout_generation = 0;
            entry.cache_slot = -1;
            entry.height = 0.0;
            entry.visual_line_count = 0;
        }
    }

    /// 使段落 `index` 的布局缓存失效。
    pub fn invalidate_paragraph(&mut self, index: usize) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.layout_generation = 0;
        entry.cache_slot = -1;
        entry.height = 0.0;
        entry.visual_line_count = 0;
        true
    }

    /// 使范围 `[start, end)` 内的段落缓存失效。
    pub fn invalidate_range(&mut self, start: usize, end: usize) {
        for i in start..end {
            self.invalidate_paragraph(i);
        }
    }

    // ── 从 VisualLine 数组同步 ──

    /// 从已有的 `VisualLine` 数组同步段落布局结果。
    ///
    /// 按 `cache_slot` 分组，将每个段落的视觉行数、高度、line_start 等信息
    /// 更新到对应的 `ParagraphLayoutEntry`。
    pub fn sync_from_visual_lines(&mut self, lines: &[VisualLine], layout_generation: u64) {
        if lines.is_empty() {
            for entry in &mut self.entries {
                entry.layout_generation = layout_generation;
                entry.visual_line_count = 0;
                entry.height = 0.0;
            }
            return;
        }

        // 按 cache_slot 分组统计
        let mut slot_stats: Vec<(i32, usize, f64, usize)> = Vec::new();
        // (cache_slot, line_count, total_height, first_line_index)

        let mut current_slot: i32 = lines[0].cache_slot;
        let mut count = 0usize;
        let mut height = 0.0f64;
        let mut first_idx = 0usize;

        for (i, line) in lines.iter().enumerate() {
            if line.cache_slot != current_slot {
                slot_stats.push((current_slot, count, height, first_idx));
                current_slot = line.cache_slot;
                count = 0;
                height = 0.0;
                first_idx = i;
            }
            count += 1;
            height += line.height;
        }
        slot_stats.push((current_slot, count, height, first_idx));

        // 更新对应段落
        for (slot, line_count, total_height, line_start) in slot_stats {
            if slot >= 0 && (slot as usize) < self.entries.len() {
                let entry = &mut self.entries[slot as usize];
                entry.layout_generation = layout_generation;
                entry.cache_slot = slot;
                entry.visual_line_count = line_count;
                entry.height = total_height;
                entry.line_start = line_start;
            }
        }
    }

    /// 获取所有段落的 `ParagraphId` 列表（按文档顺序）。
    pub fn paragraph_ids(&self) -> Vec<ParagraphId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// 获取所有段落的高度列表（按文档顺序）。
    pub fn paragraph_heights(&self) -> Vec<f64> {
        self.entries.iter().map(|e| e.height).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(text: &str) -> ParagraphLayoutCacheKey {
        ParagraphLayoutCacheKey::build(text, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0)
    }

    #[test]
    fn paragraph_id_is_stable() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_count(), 2);

        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();
        assert_ne!(id0, ParagraphId::INVALID);
        assert_ne!(id1, ParagraphId::INVALID);
        assert_ne!(id0, id1);

        // 普通编辑不改 Id
        store.update_paragraph_text(0, "hi", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_id_at(0), Some(id0));
        assert_eq!(store.paragraph_id_at(1), Some(id1));
    }

    #[test]
    fn split_preserves_original_id() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("helloworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let original_id = store.paragraph_id_at(0).unwrap();

        let (orig_idx, new_idx) = store
            .split_paragraph(
                0,
                5,
                16.0,
                "Sans",
                1.2,
                0.0,
                0.0,
                600.0,
                "hello",
                "world",
            )
            .unwrap();

        assert_eq!(orig_idx, 0);
        assert_eq!(new_idx, 1);
        assert_eq!(store.paragraph_count(), 2);
        // 原段保留 Id
        assert_eq!(store.paragraph_id_at(0), Some(original_id));
        // 新段分配新 Id
        let new_id = store.paragraph_id_at(1).unwrap();
        assert_ne!(new_id, original_id);
    }

    #[test]
    fn merge_preserves_first_id() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();

        let merged_idx = store
            .merge_paragraphs(0, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0, "helloworld")
            .unwrap();

        assert_eq!(merged_idx, 0);
        assert_eq!(store.paragraph_count(), 1);
        // 保留第一段 Id
        assert_eq!(store.paragraph_id_at(0), Some(id0));
        // 第二段 Id 被丢弃
        assert_ne!(store.paragraph_id_at(0), Some(id1));
    }

    #[test]
    fn rebuild_from_text_preserves_unchanged_layout_result() {
        // Issue #901 第二轮复核: rebuild_from_text 应保留未变化段的排版结果，
        // 只有内容变化的段落才重置 layout_generation/cache_slot/height。
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

        // 模拟排版完成：设置 layout_generation 和 height
        store.update_layout_result(0, 42, 0, 20.0, 1, 0);
        store.update_layout_result(1, 42, 1, 25.0, 1, 1);

        // rebuild_from_text，段落 0 内容不变，段落 1 内容变化
        store.rebuild_from_text("hello\nWORLD", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

        // 段落 0 内容不变，保留排版结果
        let entry0 = store.entry(0).unwrap();
        assert_eq!(entry0.layout_generation, 42);
        assert_eq!(entry0.cache_slot, 0);
        assert_eq!(entry0.height, 20.0);
        assert_eq!(entry0.visual_line_count, 1);

        // 段落 1 内容变化，重置排版结果
        let entry1 = store.entry(1).unwrap();
        assert_eq!(entry1.layout_generation, 0);
        assert_eq!(entry1.cache_slot, -1);
        assert_eq!(entry1.height, 0.0);
    }

    #[test]
    fn rebuild_from_text_preserves_id_on_unchanged() {
        // Issue #901 第二轮复核: rebuild_from_text 保留未变化段的 ParagraphId。
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();

        // rebuild，内容不变
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_id_at(0), Some(id0));
        assert_eq!(store.paragraph_id_at(1), Some(id1));
    }

    #[test]
    fn cache_key_changes_on_text_edit() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

        let original_key = store.entry(0).unwrap().cache_key.clone();
        let changed = store.update_paragraph_text(0, "hi", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert!(changed);

        let new_key = store.entry(0).unwrap().cache_key.clone();
        assert_ne!(original_key, new_key);
    }

    #[test]
    fn cache_key_same_on_no_change() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

        let changed = store.update_paragraph_text(0, "hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert!(!changed);
    }

    #[test]
    fn cache_key_changes_on_width_change() {
        let key1 = make_key("hello");
        let key2 = ParagraphLayoutCacheKey::build("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 800.0);
        assert_ne!(key1, key2);
    }

    #[test]
    fn cache_key_changes_on_font_change() {
        let key1 = make_key("hello");
        let key2 = ParagraphLayoutCacheKey::build("hello", 18.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_ne!(key1, key2);
    }

    #[test]
    fn needs_relayout_for_unlaid_out() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let key = store.entry(0).unwrap().cache_key.clone();
        assert!(store.needs_relayout(0, &key));
    }

    #[test]
    fn needs_relayout_for_changed_key() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        store.update_layout_result(0, 1, 0, 20.0, 1, 0);
        let different_key = make_key("world");
        assert!(store.needs_relayout(0, &different_key));
    }

    #[test]
    fn no_relayout_after_layout_result() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let key = store.entry(0).unwrap().cache_key.clone();
        store.update_layout_result(0, 1, 0, 20.0, 1, 0);
        assert!(!store.needs_relayout(0, &key));
    }

    #[test]
    fn compute_affected_paragraphs_basic() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nbbb\nccc", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        // 段落 0: byte [0, 3), 段落 1: byte [4, 7), 段落 2: byte [8, 11)
        let ranges = store.compute_affected_paragraphs(4, 7);
        assert_eq!(ranges.relayout_start, 1);
        assert_eq!(ranges.relayout_end, 2);
        assert!(!ranges.full_relayout);
    }

    #[test]
    fn compute_affected_paragraphs_empty() {
        let store = ParagraphLayoutStore::new();
        let ranges = store.compute_affected_paragraphs(0, 10);
        assert!(ranges.full_relayout);
    }

    #[test]
    fn invalidate_all_clears_layout() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello\nworld", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        store.update_layout_result(0, 1, 0, 20.0, 1, 0);
        store.update_layout_result(1, 1, 1, 20.0, 1, 1);
        assert!(!store.entry(0).unwrap().needs_relayout_check());
        store.invalidate_all();
        assert!(store.entry(0).unwrap().needs_relayout_check());
    }

    #[test]
    fn rebuild_preserves_ids_for_aligned_paragraphs() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nbbb", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();

        // 重建（全局参数变化），同位置段落保留 Id
        store.rebuild_from_text("aaa\nbbb", 16.0, "Sans", 1.2, 0.0, 0.0, 800.0);
        assert_eq!(store.paragraph_id_at(0), Some(id0));
        assert_eq!(store.paragraph_id_at(1), Some(id1));
    }

    #[test]
    fn rebuild_allocates_new_ids_for_new_paragraphs() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id0 = store.paragraph_id_at(0).unwrap();

        store.rebuild_from_text("aaa\nbbb", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        // 段落 0 保留 Id
        assert_eq!(store.paragraph_id_at(0), Some(id0));
        // 段落 1 是新段落，分配新 Id
        let id1 = store.paragraph_id_at(1).unwrap();
        assert_ne!(id1, id0);
    }

    #[test]
    fn insert_and_remove_paragraph() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nccc", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_count(), 2);

        store.insert_paragraph(1, "bbb", 4, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_count(), 3);

        let removed = store.remove_paragraph(1);
        assert!(removed.is_some());
        assert_eq!(store.paragraph_count(), 2);
    }

    #[test]
    fn shift_document_byte_start_preserves_id_and_key() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nbbb", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id1 = store.paragraph_id_at(1).unwrap();
        let key1 = store.entry(1).unwrap().cache_key.clone();

        store.shift_document_byte_start(1, 100);
        assert_eq!(store.paragraph_id_at(1), Some(id1));
        assert_eq!(store.entry(1).unwrap().cache_key, key1);
        assert_eq!(store.entry(1).unwrap().document_byte_start, 100);
    }

    // ── Issue #901: affected_paragraphs 测试 ──

    #[test]
    fn affected_paragraphs_no_change() {
        let affected = ParagraphLayoutStore::affected_paragraphs("hello\nworld", "hello\nworld");
        assert!(affected.is_empty());
    }

    #[test]
    fn affected_paragraphs_single_edit() {
        // 只编辑第一段内容
        let affected = ParagraphLayoutStore::affected_paragraphs("hello\nworld", "hi\nworld");
        // 从第一个差异段起所有后续段都标记为受影响
        assert_eq!(affected, vec![0, 1]);
    }

    #[test]
    fn affected_paragraphs_edit_second_para() {
        // 编辑第二段内容，第一段不变
        let affected =
            ParagraphLayoutStore::affected_paragraphs("aaa\nbbb\nccc", "aaa\nBBB\nccc");
        // 从差异段（index 1）起所有后续段
        assert_eq!(affected, vec![1, 2]);
    }

    #[test]
    fn affected_paragraphs_add_paragraph() {
        // 末尾新增段落
        let affected = ParagraphLayoutStore::affected_paragraphs("aaa\nbbb", "aaa\nbbb\nccc");
        // 前两段相同，新增第三段
        assert_eq!(affected, vec![2]);
    }

    #[test]
    fn affected_paragraphs_remove_paragraph() {
        // 末尾删除段落
        let affected = ParagraphLayoutStore::affected_paragraphs("aaa\nbbb\nccc", "aaa\nbbb");
        // 段落数量减少，旧多出的段落已不存在
        assert!(affected.is_empty());
    }

    #[test]
    fn affected_paragraphs_insert_newline() {
        // 在第一段中间插入换行（拆段）
        let affected = ParagraphLayoutStore::affected_paragraphs("helloworld", "hello\nworld");
        // 从第一个差异段起所有后续段
        assert_eq!(affected, vec![0, 1]);
    }

    #[test]
    fn affected_paragraphs_empty_texts() {
        let affected = ParagraphLayoutStore::affected_paragraphs("", "");
        assert!(affected.is_empty());
    }

    #[test]
    fn sync_from_visual_lines_incremental_basic() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nbbb", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

        // 构造视觉行：段落 0 有 1 行高 20.0，段落 1 有 1 行高 20.0
        let lines = vec![
            VisualLine {
                id: 0,
                byte_start: 0,
                byte_end: 3,
                qchar_start: 0,
                qchar_end: 3,
                hard_break: true,
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 20.0,
                para_text: "aaa".to_string(),
                para_start: 0,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 3,
                line_wrap_width: 600.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 0.0,
                qt_ascent: 16.0,
                qt_descent: 4.0,
                cache_slot: 0,
            },
            VisualLine {
                id: 1,
                byte_start: 4,
                byte_end: 7,
                qchar_start: 4,
                qchar_end: 7,
                hard_break: false,
                x: 0.0,
                y: 20.0,
                width: 30.0,
                height: 20.0,
                para_text: "bbb".to_string(),
                para_start: 4,
                qtextline_idx: 0,
                para_qchar_start: 0,
                para_qchar_end: 3,
                line_wrap_width: 600.0,
                line_indent_x: 0.0,
                para_indent: 0.0,
                x_end_trailing: 0.0,
                qt_ascent: 16.0,
                qt_descent: 4.0,
                cache_slot: 1,
            },
        ];

        // 只增量更新段落 1
        store.sync_from_visual_lines_incremental(&lines, 42, &[1]);
        let entry1 = store.entry(1).unwrap();
        assert_eq!(entry1.layout_generation, 42);
        assert_eq!(entry1.height, 20.0);
        assert_eq!(entry1.visual_line_count, 1);
        // 段落 0 不受影响
        let entry0 = store.entry(0).unwrap();
        assert_eq!(entry0.layout_generation, 0);
    }

    // ── Issue #901 第五轮阻塞2: compute_old_new_paragraph_ranges 测试 ──

    #[test]
    fn compute_old_new_ranges_enter_split() {
        // Enter 拆段：旧 1 段 "helloworld" → 新 2 段 "hello" + "world"
        let old: Vec<&str> = vec!["helloworld"];
        let new: Vec<&str> = vec!["hello", "world"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 旧删 1 段 [0,1)，新建 2 段 [0,2)
        assert_eq!(ranges.old_splice_start, 0);
        assert_eq!(ranges.old_splice_end, 1);
        assert_eq!(ranges.new_layout_start, 0);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_backspace_merge() {
        // Backspace 合段：旧 2 段 "hello" + "world" → 新 1 段 "helloworld"
        let old: Vec<&str> = vec!["hello", "world"];
        let new: Vec<&str> = vec!["helloworld"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 旧删 2 段 [0,2)，新建 1 段 [0,1)
        assert_eq!(ranges.old_splice_start, 0);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 0);
        assert_eq!(ranges.new_layout_end, 1);
    }

    #[test]
    fn compute_old_new_ranges_edit_middle() {
        // 普通编辑中间段：段落数不变
        let old: Vec<&str> = vec!["aaa", "bbb", "ccc"];
        let new: Vec<&str> = vec!["aaa", "BBB", "ccc"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 1 段，后缀公共 1 段
        // 旧删 [1,2)，新建 [1,2)
        assert_eq!(ranges.old_splice_start, 1);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 1);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_add_trailing() {
        // 末尾新增段落
        let old: Vec<&str> = vec!["aaa", "bbb"];
        let new: Vec<&str> = vec!["aaa", "bbb", "ccc"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 2 段，后缀公共 0 段
        // 旧删 [2,2)（空），新建 [2,3)
        assert_eq!(ranges.old_splice_start, 2);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 2);
        assert_eq!(ranges.new_layout_end, 3);
    }

    #[test]
    fn compute_old_new_ranges_remove_trailing() {
        // 末尾删除段落
        let old: Vec<&str> = vec!["aaa", "bbb", "ccc"];
        let new: Vec<&str> = vec!["aaa", "bbb"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 2 段，后缀公共 0 段
        // 旧删 [2,3)，新建 [2,2)（空）
        assert_eq!(ranges.old_splice_start, 2);
        assert_eq!(ranges.old_splice_end, 3);
        assert_eq!(ranges.new_layout_start, 2);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_no_change() {
        // 无变化
        let old: Vec<&str> = vec!["aaa", "bbb"];
        let new: Vec<&str> = vec!["aaa", "bbb"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 全部是前缀公共，旧删和新建都是空范围
        assert_eq!(ranges.old_splice_start, 2);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 2);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_insert_newline_between() {
        // 在第一、二段之间新插空行
        let old: Vec<&str> = vec!["aaa", "bbb", "ccc"];
        let new: Vec<&str> = vec!["aaa", "", "bbb", "ccc"];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 1 段（"aaa"），后缀公共 2 段（"bbb","ccc"）
        // 旧删 [1,1)（空），新建 [1,2)（空行）
        assert_eq!(ranges.old_splice_start, 1);
        assert_eq!(ranges.old_splice_end, 1);
        assert_eq!(ranges.new_layout_start, 1);
        assert_eq!(ranges.new_layout_end, 2);
    }
}

/// 辅助 trait：检查段落是否需要重排（仅供测试）。
#[cfg(test)]
impl ParagraphLayoutEntry {
    fn needs_relayout_check(&self) -> bool {
        !self.is_laid_out()
    }
}
