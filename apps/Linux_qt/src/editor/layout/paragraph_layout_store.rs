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

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::paragraph_height_index::ParagraphLineCountIndex;
use super::types::{ParagraphId, VisualLine};

// ── 稳定段落身份 ──
//
// Issue #901 第十八轮问题4: `ParagraphId` 定义已移到 `super::types`，
// 使 `VisualLine` 可直接持有 `ParagraphId`。此处通过 `use` 导入保持本模块
// 内 `ParagraphId` 引用不变（`ParagraphLayoutEntry.id` 等仍用此类型）。
// 原 doc 注释保留为模块级文档，描述稳定身份语义。

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
    /// Issue #901 第二十八轮复核问题3: 该段落 layout 的 epoch。
    ///
    /// 分配物理槽（`acquire_physical_slot_for_generation`）时由 C++ 设置，
    /// Store 通过 `add_layout_ref` 获取并记录于此。retire 时连同 generation/slot
    /// 一起传给 `release_layout_ref(gen, slot, epoch)`，epoch 不匹配则忽略释放，
    /// 防止旧帧误释新 layout。
    pub layout_epoch: u64,
    /// 段落文档 byte 起始偏移（随编辑移动，**不作为 identity**）。
    pub document_byte_start: usize,
    /// 段落文本长度（UTF-8 byte，不含尾部 `'\n'`）。
    pub paragraph_text_len: usize,
    /// 段落总高度（所有视觉行高度之和）。
    pub height: f64,
    /// 段落内视觉行数。
    pub visual_line_count: usize,
    /// Issue #901 第二十三轮问题1: 该段落是否以硬换行（`'\n'`）结尾。
    ///
    /// `甲\n` 切分为 `[("甲", true), ("", false)]`，第一段 `hard_break=true`，
    /// 尾空段 `hard_break=false`。`甲`（无换行）只有一段 `hard_break=false`。
    /// 用于段落身份比较（问题2）和尾空段结构对齐（问题1）。
    pub hard_break: bool,
    /// Issue #901 第十轮问题5: 段内相对视觉行 — 按 ParagraphId 持久分块储存。
    ///
    /// 每行的坐标是**段内相对值**：
    /// - `y` 从段顶 0 起累加行高。
    /// - `byte_start` / `byte_end` 相对段落文本开头（0-based）。
    /// - `qchar_start` / `qchar_end` 相对段落文本开头（0-based）。
    /// - `para_start` = 0。
    /// - `id` = 段内行索引（0-based）。
    ///
    /// 段落前缀偏移（段顶 y、段首 byte/qchar）由 [`ParagraphHeightIndex`] 的
    /// 偏移索引按需解析，不随段前编辑逐行重写。
    ///
    /// 增量编辑时只 splice 受影响段的 `relative_lines`，未改后缀段的
    /// `relative_lines` 不变，消除 O(后缀行数) 热路径。
    pub relative_lines: Vec<VisualLine>,
}

impl ParagraphLayoutEntry {
    /// 该段落是否已排版（有有效的 generation 和 slot）。
    pub fn is_laid_out(&self) -> bool {
        self.layout_generation != 0 && self.cache_slot >= 0
    }
}

// ── Issue #901 第二十三轮问题1: 统一段落切分 ──

/// 统一段落切分：生成 `(content, hard_break)` 序列，包括末尾换行产生的尾空段。
///
/// 与 `str::split_inclusive('\n')` 的区别：`split_inclusive` 对 `甲\n` 只产生
/// `["甲\n"]`（一个条目），不产生尾空段；本函数产生尾空段，使 store entries、
/// canonical paragraphs、VisualLine、ParagraphId 逐段一一对应。
///
/// # 规则
///
/// - `甲\n` → `[("甲", true), ("", false)]`
/// - `甲` → `[("甲", false)]`
/// - `甲\n乙` → `[("甲", true), ("乙", false)]`
/// - `甲\n乙\n` → `[("甲", true), ("乙", true), ("", false)]`
/// - `` → `[("", false)]`
///
/// `content` 是去掉 `'\n'` 的段落正文；`hard_break` 表示该段后是否有 `'\n'`。
/// 尾空段（末尾 `'\n'` 后的空段）`hard_break=false`，因为没有后续 `'\n'`。
pub fn split_paragraphs_with_hard_break(text: &str) -> Vec<(&str, bool)> {
    if text.is_empty() {
        return vec![("", false)];
    }
    let mut result = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            // SAFETY: i 是 '\n' 字节位置，start..i 是有效的 UTF-8 边界
            //（'\n' 是单字节 ASCII，不会切割多字节字符）。
            result.push((&text[start..i], true));
            start = i + 1;
        }
    }
    // 处理最后一段（不以 \n 结尾的剩余部分，或末尾 \n 后的空段）
    result.push((&text[start..], false));
    result
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// 2. 普通编辑只变更所在段及实际连带范围的 `cache_key`。
/// 3. 换行拆段：原段保留 Id，新段分配新 Id。
/// 4. 删回车合段：被合并段的 Id 失效，保留段保持原 Id。
/// 5. `entries` 按文档顺序排列，索引即段落在文档中的顺序。
/// 6. `id_to_slot_cache` 始终与 `entries` 同步：`id_to_slot_cache[entries[i].id] == i`。
/// 7. `line_count_index` 在 `sync_from_visual_lines` / `sync_from_visual_lines_incremental`
///    后与 `entries` 的 `visual_line_count` 同步。段落行范围由前缀和动态计算，
///    不持有以全局 visual_lines 下标为绝对值的 HashMap。
///
/// # 增量更新策略
///
/// - **普通编辑**（不涉及 `'\n'`）：只更新所在段落的 `cache_key`，
///   其他段落不变。`id_to_slot_cache` 不变（slot 不变）。
/// - **换行拆段**（插入 `'\n'`）：原段保留 Id，在原段后插入新段（分配新 Id）。
///   `id_to_slot_cache` 中新段加入，后续段 slot +1。
/// - **删回车合段**（删除 `'\n'`）：移除后一段，将其内容并入前一段，
///   保留前一段 Id，丢弃后一段 Id。
///   `id_to_slot_cache` 中被删段移除，后续段 slot -1。
/// - **宽度/字体/字号/行距变化**：全局失效，所有段落的 `cache_key` 失效。
#[derive(Clone, Debug, Default)]
pub struct ParagraphLayoutStore {
    /// 按文档顺序排列的段落缓存条目。
    entries: Vec<ParagraphLayoutEntry>,
    /// 下一个可分配的 `ParagraphId` 内部数值（从 1 开始）。
    next_id: u64,
    /// Issue #901 第二十一轮阻塞5+问题4: ParagraphId → slot 持久映射缓存。
    ///
    /// 与 `entries` 同步维护：`id_to_slot_cache[entries[i].id] == i`。
    /// 普通编辑（不涉及换行）时 slot 不变，映射不变，O(1) 获取。
    /// 拆段/合段时增量更新（新段加入/被删段移除 + 后续段 slot 平移），
    /// 不每笔遍历整章重建。
    ///
    /// Issue #901 第二十一轮问题4: 用 `Arc<HashMap>` 持久维护，
    /// `paragraph_id_to_slot_arc` 返回 `Arc::clone`（O(1)）而非
    /// `Arc::new(map.clone())`（O(段落数) 深复制）。更新时用 `Arc::make_mut`
    ///（refcount==1 原地改，refcount>1 COW）。
    id_to_slot_cache: Arc<HashMap<ParagraphId, usize>>,
    /// Issue #901 第二十一轮问题2: 段落行数前缀索引（Fenwick 树）。
    ///
    /// 替代旧的 `id_to_line_range: HashMap<ParagraphId, (usize, usize)>`。
    /// 旧 HashMap 以全局 visual_lines 数组下标为绝对值，却只对改动段更新，
    /// 换行造成行数变化时全部后缀段范围仍指向旧下标。
    ///
    /// 改用 Fenwick 树维护每段 `visual_line_count` 的前缀和：
    /// - 段落 `i` 的行范围 `[line_start, line_start + visual_line_count)`。
    /// - `line_start = line_count_index.paragraph_line_start(i)`（前 i 段行数和）。
    /// - 前面段行数变化时，后缀段的 `line_start` 由前缀和自动平移，
    ///   不需要逐段更新绝对行下标。
    /// Issue #901 第二十二轮问题4: Arc 包装实现版本化共享。
    /// snapshot 通过 `line_count_index_arc()` 获取 `Arc::clone`（O(1)），
    /// store 更新时用 `Arc::make_mut`（COW：refcount==1 原地改，refcount>1 clone 新副本）。
    line_count_index: Arc<ParagraphLineCountIndex>,
    /// Issue #901 第二十五轮复核问题1: 已无持有者可回收的 (generation, physical_slot) 对。
    ///
    /// 增量编辑（普通改段/拆段/合段）使旧 `cache_slot` 被置 -1 后，对应的
    /// QTextLayout 不再被任何 snapshot 引用。将旧 `(layout_generation, cache_slot)`
    /// 收集到此列表，由 `release_retired_layouts` 在增量排版完成后统一释放，
    /// 避免 QTextLayout 内存随编辑次数持续增长。
    ///
    /// Issue #901 第二十七轮复核问题2: 槽号通过按 generation 隔离的 freelist 复用
    ///（见 `qt_cache::acquire_physical_slot_for_generation`），释放后回收到同一
    /// generation 的 free_slots，跨 generation 槽号互不影响。
    ///
    /// Issue #901 第二十八轮复核问题2: 完全移除 Rust 端的 `next_physical_slot` 高水位
    /// 门槛。同一 generation 物理 slot 的占用/回收/分配全部由 C++ `LayoutGeneration`
    /// 的 freelist 一个 owner 管理，旧槽可被真正复用，不再随编辑次数单调增长。
    ///
    /// Issue #901 第二十七轮复核问题3: 不再延迟一轮释放。retired slots 在
    /// `release_retired_layouts` 中立即释放。这比延迟一轮更安全，因为 retired_slots
    /// 只在确认旧 layout 不再被任何 snapshot 引用时才被收集。完整的基于引用计数的
    /// 资源所有权（QtLayoutHandle with epoch/version）留作后续优化。
    ///
    /// Issue #901 第二十八轮复核问题3: retired_slots 改为 `(generation, slot, epoch)` 3-tuple，
    /// 释放时带 epoch 校验。epoch 不匹配（slot 已被复用）则忽略释放，不误减新 layout 的 refcount。
    retired_slots: Vec<(u64, i32, u64)>,
}

impl ParagraphLayoutStore {
    /// 创建空的段落布局存储。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            id_to_slot_cache: Arc::new(HashMap::new()),
            line_count_index: Arc::new(ParagraphLineCountIndex::new()),
            retired_slots: Vec::new(),
        }
    }

    /// 分配一个新的 `ParagraphId`。
    fn allocate_id(&mut self) -> ParagraphId {
        let id = ParagraphId::from_u64(self.next_id);
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
        // Issue #901 第二十七轮复核问题3: 不再延迟一轮释放。clear 是全局失效，
        // 立即释放当前 retired_slots 和 entries 中所有有效 handle。
        // Issue #901 第二十八轮复核问题3: clear 是全局失效（字体变化等），
        // 所有旧引用都无效，直接用 release_paragraph_layout_slot_gen 立即释放
        //（C++ 端用当前 epoch 调 release_layout_ref，全局失效语义下安全）。
        for (gen, slot, _epoch) in std::mem::take(&mut self.retired_slots) {
            super::qt_cache::release_paragraph_layout_slot_gen(gen, slot);
        }
        // 释放当前 entries 的有效 handle。
        for entry in &self.entries {
            if entry.layout_generation != 0 && entry.cache_slot >= 0 {
                super::qt_cache::release_paragraph_layout_slot_gen(
                    entry.layout_generation,
                    entry.cache_slot,
                );
            }
        }
        self.entries.clear();
        // next_id 不重置，避免新分配的 Id 与已释放的 Id 冲突。
        Arc::make_mut(&mut self.id_to_slot_cache).clear();
        Arc::make_mut(&mut self.line_count_index).clear();
        // Issue #901 第二十八轮复核问题2: next_physical_slot 已移除，槽号复用完全由
        // C++ LayoutGeneration 的 freelist 按 generation 隔离管理，clear 后旧 generation
        // 的槽号自然失效，无需 Rust 端维护高水位。
    }

    /// Issue #901 第二十五轮复核问题1: 取出并清空 retired slots 列表。
    ///
    /// 返回所有已无持有者可回收的 `(generation, physical_slot, epoch)` 三元组。
    /// 调用方应对每个 `(gen, slot, epoch)` 调用 `qt_cache::release_layout_ref`
    /// 释放对应的 QTextLayout。
    pub fn drain_retired_slots(&mut self) -> Vec<(u64, i32, u64)> {
        std::mem::take(&mut self.retired_slots)
    }

    /// Issue #901 第二十五轮复核问题1 + 第二十七轮复核问题3 + 第二十八轮复核问题3:
    /// 释放 retired slots 对应的 QTextLayout。
    ///
    /// 立即释放机制：retired slots 在此直接释放，不再延迟一轮。
    /// retired_slots 只在确认旧 layout 不再被任何 snapshot 引用时才被收集
    ///（旧 cache_slot 被置 -1 后收集），因此立即释放是安全的。
    ///
    /// Issue #901 第二十八轮复核问题3: 基于 epoch 校验的引用计数释放。
    /// 每个 retired slot 携带收集时的 epoch，调用 `release_layout_ref(gen, slot, epoch)`。
    /// epoch 不匹配（slot 已被复用）则忽略，不误减新 layout 的 refcount。
    /// epoch 匹配则减 refcount，归零时才 delete layout 并回收 slot。
    ///
    /// 在增量排版完成后（snapshot 构建后）调用。
    pub fn release_retired_layouts(&mut self) {
        for (gen, slot, epoch) in std::mem::take(&mut self.retired_slots) {
            super::qt_cache::release_layout_ref(gen, slot, epoch);
        }
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
    /// 已有段落的 `ParagraphId` 尽量保留（按内容前缀+后缀匹配对齐），
    /// 新段落分配新 Id，多余段落的 Id 丢弃。
    ///
    /// Issue #901 第二轮复核: 保留未变化段的排版结果
    /// （layout_generation/cache_slot/height/visual_line_count/line_start）。
    /// 只有 cache_key 变化的段落（内容或参数变了）才重置排版结果，
    /// 需要重新走 QTextLayout。内容相同的段落保留旧 shaping/行数据/纹理，
    /// 不重建全部 QTextLayout/静态 QSGTextNode。
    ///
    /// Issue #901 第十六轮问题1: 以 ParagraphId 作为稳定身份，按旧/新段落
    /// 内容 diff 做 Id 的 splice（前缀匹配 + 后缀匹配），而非按下标对齐。
    /// 中间插段时，后缀相同内容段继承**对应旧 entry** 的 ParagraphId，
    /// 不再把后缀段的身份给新插入的段。
    /// 反例：正文"甲\n乙"在"甲"前插入回车 → "\n甲\n乙"（3 段）。
    /// 旧 entries = [甲, 乙]。按下标对齐会把"甲"的 Id 给新段[0]（空行），
    /// 把"乙"的 Id 给新段[1]（"甲"），新段[2]（"乙"）分配新 Id —— 身份错位。
    /// 按前缀+后缀匹配：前缀 0（空行 ≠ 甲），后缀 2（甲==甲, 乙==乙），
    /// 新段[0] 分配新 Id，新段[1] 继承旧[0] 的 Id，新段[2] 继承旧[1] 的 Id。
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
        // Issue #901 第二十三轮问题1: 用统一切分器生成 (content, hard_break) 序列，
        // 包括末尾换行产生的尾空段。store entries、canonical paragraphs、VisualLine、
        // ParagraphId 逐段一一对应。尾空段是结构事实，不是游离视觉行。
        let paragraphs = split_paragraphs_with_hard_break(text);
        let new_count = paragraphs.len();
        let old_count = self.entries.len();

        // Issue #901 第十六轮问题1: 计算新段 cache_key 和 text_hash，
        // 用 (text_hash, hard_break) 做前缀+后缀内容匹配（text_hash 冲突概率极低，可接受）。
        // Issue #901 第二十三轮问题2: hard_break 是段落身份的一部分，
        // (content, hard_break) 不同则视为不同段，不继承 Id。
        let mut new_keys: Vec<ParagraphLayoutCacheKey> = Vec::with_capacity(new_count);
        let mut new_text_hashes: Vec<u64> = Vec::with_capacity(new_count);
        for (paragraph_text, _hard_break) in &paragraphs {
            let cache_key = ParagraphLayoutCacheKey::build(
                paragraph_text,
                font_size,
                font_family,
                line_spacing,
                text_indent,
                padding,
                width,
            );
            new_text_hashes.push(cache_key.text_hash());
            new_keys.push(cache_key);
        }

        // 旧段的 text_hash（不含 hard_break — rebuild_from_text 的 Id 继承
        // 只看内容是否相同，hard_break 不影响排版结果/cache_key）
        let old_text_hashes: Vec<u64> =
            self.entries.iter().map(|e| e.cache_key.text_hash()).collect();

        // 前缀匹配：从前往后找 text_hash 相同的段
        let mut prefix_common = 0usize;
        let min_len = old_count.min(new_count);
        for i in 0..min_len {
            if old_text_hashes[i] == new_text_hashes[i] {
                prefix_common = i + 1;
            } else {
                break;
            }
        }

        // 后缀匹配：从后往前找 text_hash 相同的段（在前缀之后的部分中找）
        let mut suffix_common = 0usize;
        let remaining_old = old_count - prefix_common;
        let remaining_new = new_count - prefix_common;
        let max_suffix = remaining_old.min(remaining_new);
        while suffix_common < max_suffix {
            let old_idx = old_count - 1 - suffix_common;
            let new_idx = new_count - 1 - suffix_common;
            if old_text_hashes[old_idx] == new_text_hashes[new_idx] {
                suffix_common += 1;
            } else {
                break;
            }
        }

        let mut new_entries: Vec<ParagraphLayoutEntry> = Vec::with_capacity(new_count);
        let mut byte_offset = 0usize;

        // Issue #901 第二十六轮复核缺口1: 记录被新 entries 继承的旧
        // (layout_generation, cache_slot) handle。前缀/后缀段在 cache_key
        // 完全相同时继承旧 entry 的排版结果（含 Qt 槽），这些 handle 仍存活
        // 不能回收。只有未被继承的旧 handle（被丢弃的旧 entry、cache_key 变化
        // 导致重置的段、中间段对应的旧 entry）才推入 retired_slots。
        let mut inherited_handles: std::collections::HashSet<(u64, i32)> =
            std::collections::HashSet::new();

        for (i, (paragraph_text, hard_break)) in paragraphs.iter().enumerate() {
            let cache_key = new_keys[i].clone();

            // Issue #901 第十六轮问题1: 按前缀/后缀/中间段分别处理 Id 继承。
            // - 前缀段（i < prefix_common）：继承旧 entry[i] 的 Id。
            // - 后缀段（i >= new_count - suffix_common）：继承对应旧 entry
            //   （old_count - (new_count - i)）的 Id。
            // - 中间段：分配新 Id。
            //
            // 排版结果保留逻辑（Issue #901 第二轮复核）：
            // - cache_key 完全相同（内容+参数没变）→ 保留 layout_generation/
            //   cache_slot/height/visual_line_count/relative_lines。
            // - cache_key 不同（内容或参数变了）→ 重置排版结果，但保留 Id。
            //   （参数变化时 Id 保留但需重排，内容变化时 Id 保留但需重排。）
            let (
                id,
                layout_generation,
                cache_slot,
                layout_epoch,
                height,
                visual_line_count,
                relative_lines,
            ) = if i < prefix_common {
                // 前缀段：继承旧 entry[i]
                let old = &self.entries[i];
                if old.cache_key == cache_key {
                    // Issue #901 第二十六轮复核缺口1: cache_key 完全相同，
                    // 新 entry 继承了旧 entry 的 (layout_generation, cache_slot)，
                    // 该 Qt 槽仍存活，记录到 inherited_handles 避免被回收。
                    if old.layout_generation != 0 && old.cache_slot >= 0 {
                        inherited_handles.insert((old.layout_generation, old.cache_slot));
                    }
                    (
                        old.id,
                        old.layout_generation,
                        old.cache_slot,
                        old.layout_epoch,
                        old.height,
                        old.visual_line_count,
                        old.relative_lines.clone(),
                    )
                } else {
                    (old.id, 0u64, -1, 0u64, 0.0f64, 0usize, Vec::new())
                }
            } else if suffix_common > 0 && i >= new_count - suffix_common {
                // 后缀段：继承对应旧 entry（从后往前对齐）
                let old_idx = old_count - (new_count - i);
                let old = &self.entries[old_idx];
                if old.cache_key == cache_key {
                    // Issue #901 第二十六轮复核缺口1: cache_key 完全相同，
                    // 新 entry 继承了旧 entry 的 (layout_generation, cache_slot)，
                    // 该 Qt 槽仍存活，记录到 inherited_handles 避免被回收。
                    if old.layout_generation != 0 && old.cache_slot >= 0 {
                        inherited_handles.insert((old.layout_generation, old.cache_slot));
                    }
                    (
                        old.id,
                        old.layout_generation,
                        old.cache_slot,
                        old.layout_epoch,
                        old.height,
                        old.visual_line_count,
                        old.relative_lines.clone(),
                    )
                } else {
                    (old.id, 0u64, -1, 0u64, 0.0f64, 0usize, Vec::new())
                }
            } else {
                // 中间段：分配新 Id
                (self.allocate_id(), 0u64, -1, 0u64, 0.0f64, 0usize, Vec::new())
            };

            new_entries.push(ParagraphLayoutEntry {
                id,
                cache_key,
                layout_generation,
                cache_slot,
                layout_epoch,
                document_byte_start: byte_offset,
                paragraph_text_len: paragraph_text.len(),
                height,
                visual_line_count,
                hard_break: *hard_break,
                relative_lines,
            });

            // byte_offset 推进：content 长度 + '\n' 长度（如果 hard_break）
            byte_offset += paragraph_text.len() + if *hard_break { 1 } else { 0 };
        }

        // Issue #901 第二十六轮复核缺口1: 回收未被新 entries 继承的旧 Qt 槽。
        //
        // rebuild_from_text 在 self.entries = new_entries 之前没有遍历被删除或被替换的
        // 旧 entry，连续 Enter、删回车合段会积累不可回收的 QTextLayout 对象。此处遍历
        // 所有旧 entries，把没有被 inherited_handles 包含且有效（layout_generation != 0
        // && cache_slot >= 0）的 (layout_generation, cache_slot) 推入 retired_slots，
        // 由 release_retired_layouts 在增量排版完成后统一释放 C++ QTextLayout。
        //
        // 前缀/后缀段 cache_key 相同时继承了旧 handle（已记入 inherited_handles），
        // 不回收。cache_key 不同（内容或参数变了）的段新 entry 的
        // layout_generation=0/cache_slot=-1，旧 handle 未被继承，需回收。
        // 中间段全部分配新 Id，对应旧 entry 的 handle 未被继承，需回收。
        //
        // 先收集到局部 Vec 再 extend，避免同时借用 self.entries（不可变）和
        // self.retired_slots（可变）的借用冲突。
        // Issue #901 第二十八轮复核问题3: retired_slots 改为 3-tuple，记录旧 entry 的 epoch。
        let mut newly_retired: Vec<(u64, i32, u64)> = Vec::new();
        for old in &self.entries {
            if old.layout_generation != 0
                && old.cache_slot >= 0
                && !inherited_handles.contains(&(old.layout_generation, old.cache_slot))
            {
                newly_retired.push((old.layout_generation, old.cache_slot, old.layout_epoch));
            }
        }
        self.retired_slots.extend(newly_retired);

        self.entries = new_entries;
        // Issue #901 第二十一轮阻塞5: 同步重建 id_to_slot_cache。
        self.rebuild_id_to_slot_cache();
        // Issue #901 第二十二轮问题1: 从新 entries 已继承的 visual_line_count
        // 初始化行数索引，而不是全部置零。rebuild_from_text 的前缀/后缀匹配已
        // 把复用段的 visual_line_count 正确继承到 new_entries（见上方 435-471 行），
        // 行数索引必须与 entries 保持一致。若全部置零，后续 sync_from_visual_lines_incremental
        // 只更新 affected_indices，未改的后缀段行数再也恢复不了，导致
        // resolve_viewport_lines / paragraph_line_range 找错行。
        let counts: Vec<usize> = self.entries.iter().map(|e| e.visual_line_count).collect();
        Arc::make_mut(&mut self.line_count_index).rebuild(counts);
        // Issue #901 第二十八轮复核问题2: next_physical_slot 已移除，槽号复用完全由
        // C++ LayoutGeneration 的 freelist 管理，无需在此同步高水位。
    }

    /// Issue #901 第十六轮问题1: 按 `ParagraphId` 查找当前段落 slot（段落索引）。
    ///
    /// 建立 ParagraphId → 当前 paragraph slot 映射，用于消费方查询。
    /// 与 [`paragraph_index_by_id`] 的区别：语义更明确——返回的是当前
    /// 段落在文档中的 slot 索引（即 `entries` 中的下标）。
    ///
    /// Issue #901 第二十一轮阻塞5: 优先从 `id_to_slot_cache` 查询（O(1)），
    /// 缓存未命中时回退到 O(n) 线性查找（仅在缓存失效的异常情况下触发）。
    pub fn paragraph_id_to_slot(&self, id: ParagraphId) -> Option<usize> {
        if let Some(&slot) = self.id_to_slot_cache.get(&id) {
            // 验证缓存一致性（debug 模式下断言）
            debug_assert!(
                self.entries.get(slot).map_or(true, |e| e.id == id),
                "id_to_slot_cache 不一致: id={:?} cache_slot={} entry_id={:?}",
                id,
                slot,
                self.entries.get(slot).map(|e| e.id)
            );
            return Some(slot);
        }
        self.entries.iter().position(|e| e.id == id)
    }

    /// Issue #901 第二十一轮问题4: 获取 `ParagraphId → slot` 映射的 Arc 共享引用。
    ///
    /// 返回内部持久维护的 `id_to_slot_cache` 的 Arc 克隆（O(1)），
    /// 供消费方共享，不每笔遍历整章重建 HashMap。
    /// 映射在 `rebuild_from_text` / splice 操作后已同步更新，
    /// 调用方直接 `Arc::clone` 即可，不再深复制全章 HashMap。
    pub fn paragraph_id_to_slot_arc(&self) -> Arc<HashMap<ParagraphId, usize>> {
        Arc::clone(&self.id_to_slot_cache)
    }

    /// Issue #901 第二十一轮问题2: 获取段落行数前缀索引的只读引用。
    ///
    /// 消费方通过 `line_count_index.paragraph_line_range(slot)` 或
    /// `line_count_index.paragraph_line_start(slot)` 动态计算行范围，
    /// 不再持有以全局 visual_lines 下标为绝对值的 HashMap。
    pub fn line_count_index(&self) -> &ParagraphLineCountIndex {
        &self.line_count_index
    }

    /// Issue #901 第二十二轮问题4: 获取段落行数前缀索引的 Arc 共享引用。
    ///
    /// 用于构造 snapshot 时传递一份行数索引。`Arc::clone` 是 O(1)，
    /// 不再每笔 clone 整个 Fenwick 树（O(段落数)）。
    /// store 后续更新时通过 `Arc::make_mut` COW，snapshot 持有的旧 Arc
    /// 仍指向不可变旧版本。
    pub fn line_count_index_arc(&self) -> Arc<ParagraphLineCountIndex> {
        Arc::clone(&self.line_count_index)
    }

    /// Issue #901 第二十一轮问题2: 按段落索引（slot）查询视觉行范围。
    ///
    /// 通过行数前缀索引动态计算：`line_start = prefix_sum(slot)`，
    /// `line_end = line_start + entries[slot].visual_line_count`。
    /// 前面段行数变化时，后缀段范围由前缀和自动平移，不需要逐段更新。
    ///
    /// 越界返回 `(total, total)`。
    pub fn paragraph_line_range_by_slot(&self, slot: usize) -> (usize, usize) {
        self.line_count_index.paragraph_line_range(slot)
    }

    /// Issue #901 第二十一轮阻塞5+问题4: 内部方法 — 从 `entries` 全量重建 `id_to_slot_cache`。
    ///
    /// 在 `rebuild_from_text` 等全量替换 entries 的操作后调用。
    /// 用 `Arc::make_mut` 原地改（refcount==1）或 COW（refcount>1）。
    fn rebuild_id_to_slot_cache(&mut self) {
        let cache = Arc::make_mut(&mut self.id_to_slot_cache);
        cache.clear();
        cache.reserve(self.entries.len());
        for (i, entry) in self.entries.iter().enumerate() {
            cache.insert(entry.id, i);
        }
    }

    /// Issue #901 第二十一轮阻塞5+问题4: 内部方法 — 在 `index` 处插入新段后
    /// 增量更新 `id_to_slot_cache`。
    ///
    /// `new_id` 是新段的 ParagraphId，`index` 是新段的 slot。
    /// 原 slot >= index 的段全部 +1。
    /// 用 `Arc::make_mut` 原地改或 COW，保证已发出的 Arc 引用不受影响。
    fn id_to_slot_cache_insert(&mut self, new_id: ParagraphId, index: usize) {
        let cache = Arc::make_mut(&mut self.id_to_slot_cache);
        // 后续段 slot +1
        for slot in cache.values_mut() {
            if *slot >= index {
                *slot += 1;
            }
        }
        cache.insert(new_id, index);
    }

    /// Issue #901 第二十一轮阻塞5+问题4: 内部方法 — 删除 `index` 处的段后
    /// 增量更新 `id_to_slot_cache`。
    ///
    /// `removed_id` 是被删段的 ParagraphId，`index` 是被删段的 slot。
    /// 原 slot > index 的段全部 -1。
    /// 用 `Arc::make_mut` 原地改或 COW，保证已发出的 Arc 引用不受影响。
    fn id_to_slot_cache_remove(&mut self, removed_id: ParagraphId, index: usize) {
        let cache = Arc::make_mut(&mut self.id_to_slot_cache);
        cache.remove(&removed_id);
        // 后续段 slot -1
        for slot in cache.values_mut() {
            if *slot > index {
                *slot -= 1;
            }
        }
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
        // Issue #901 第二十五轮复核问题1: 收集原段旧 (generation, slot) 供后续释放。
        let old_gen = original_entry.layout_generation;
        let old_slot = original_entry.cache_slot;
        // Issue #901 第二十八轮复核问题3: 记录旧 epoch 供 release_layout_ref 校验。
        let old_epoch = original_entry.layout_epoch;
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
        original_entry.layout_epoch = 0;
        original_entry.paragraph_text_len = original_text_before_split.len();
        original_entry.height = 0.0;
        original_entry.visual_line_count = 0;

        // 新段（split 点之后的部分）
        // Issue #901 第二十一轮问题1: cache_slot = -1 表示未分配 physical_slot，
        // 后续 update_layout_result / sync_from_visual_lines_incremental 会分配新槽。
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
            layout_epoch: 0,
            document_byte_start: new_doc_start,
            paragraph_text_len: original_text_after_split.len(),
            height: 0.0,
            visual_line_count: 0,
            // Issue #901 第二十三轮问题1: split 产生的新段后是否有 '\n' 取决于
            // 原段 split 点之后的内容。调用方通过 rebuild_from_text 统一切分时
            // 会正确设置；此处 split_paragraph 增量路径中新段 hard_break 未知，
            // 默认 false，由后续 sync 同步。
            hard_break: false,
            relative_lines: Vec::new(),
        };

        self.entries.insert(index + 1, new_entry);
        // Issue #901 第二十五轮复核问题1: 原段旧 QTextLayout 进入 retired 集合，
        // 后续 release_retired_layouts 释放，避免内存随拆段次数持续增长。
        // Issue #901 第二十八轮复核问题3: 携带 epoch 供 release_layout_ref 校验。
        if old_slot >= 0 && old_gen != 0 {
            self.retired_slots.push((old_gen, old_slot, old_epoch));
        }
        // Issue #901 第二十一轮阻塞5: 增量更新 id_to_slot_cache。
        // 新段插入在 index+1，原 slot >= index+1 的段全部 +1。
        self.id_to_slot_cache_insert(new_id, index + 1);
        // Issue #901 第二十一轮问题2: 同步插入行数索引槽位（初始行数 0）。
        Arc::make_mut(&mut self.line_count_index).insert(index + 1, 0);
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

        // Issue #901 第二十五轮复核问题1: 先在 entry 借用块内读出旧 layout 信息，
        // 块结束后再访问 self.entries.remove 和 self.retired_slots。
        let (old_gen, old_slot, old_epoch) = {
            let entry = &mut self.entries[index];
            let old_gen = entry.layout_generation;
            let old_slot = entry.cache_slot;
            // Issue #901 第二十八轮复核问题3: 记录旧 epoch 供 release_layout_ref 校验。
            let old_epoch = entry.layout_epoch;
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
            entry.layout_epoch = 0;
            entry.paragraph_text_len = merged_text.len();
            entry.height = 0.0;
            entry.visual_line_count = 0;
            (old_gen, old_slot, old_epoch)
        };

        // 移除被合并的段落（index + 1）
        // Issue #901 第二十八轮复核问题2: 被合并段的 physical_slot 通过 retired_slots
        // 回收到 C++ LayoutGeneration 的 freelist 供后续复用，不再单调增长。
        let removed_entry = self.entries.remove(index + 1);
        // Issue #901 第二十五轮复核问题1: 原段和被合并段的旧 QTextLayout 进入 retired 集合，
        // 后续 release_retired_layouts 释放，避免内存随合段次数持续增长。
        // Issue #901 第二十八轮复核问题3: 携带 epoch 供 release_layout_ref 校验。
        if old_slot >= 0 && old_gen != 0 {
            self.retired_slots.push((old_gen, old_slot, old_epoch));
        }
        if removed_entry.cache_slot >= 0 && removed_entry.layout_generation != 0 {
            self.retired_slots
                .push((removed_entry.layout_generation, removed_entry.cache_slot, removed_entry.layout_epoch));
        }
        // Issue #901 第二十一轮阻塞5: 增量更新 id_to_slot_cache。
        // 被删段在 index+1，原 slot > index+1 的段全部 -1。
        self.id_to_slot_cache_remove(removed_entry.id, index + 1);
        // Issue #901 第二十一轮问题2: 同步删除行数索引槽位。
        Arc::make_mut(&mut self.line_count_index).remove(index + 1);
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
        let new_key = ParagraphLayoutCacheKey::build(
            new_text,
            font_size,
            font_family,
            line_spacing,
            text_indent,
            padding,
            width,
        );
        // Issue #901 第二十五轮复核问题1: 先在 entry 借用块内读出旧 layout 信息，
        // 块结束后再 push 到 retired_slots，避免同时 &mut self.entries 和
        // &mut self.retired_slots 的借用冲突。
        let (changed, old_gen, old_slot, old_epoch) = {
            let Some(entry) = self.entries.get_mut(index) else {
                return false;
            };
            let changed = entry.cache_key != new_key;
            entry.cache_key = new_key;
            if changed {
                let old_gen = entry.layout_generation;
                let old_slot = entry.cache_slot;
                // Issue #901 第二十八轮复核问题3: 记录旧 epoch 供 release_layout_ref 校验。
                let old_epoch = entry.layout_epoch;
                entry.layout_generation = 0;
                entry.cache_slot = -1;
                entry.layout_epoch = 0;
                entry.paragraph_text_len = new_text.len();
                entry.height = 0.0;
                entry.visual_line_count = 0;
                (true, old_gen, old_slot, old_epoch)
            } else {
                (false, 0u64, -1i32, 0u64)
            }
        };
        // 旧 QTextLayout 进入 retired 集合，后续 release_retired_layouts 释放，
        // 避免内存随编辑次数持续增长。
        // Issue #901 第二十八轮复核问题3: 携带 epoch 供 release_layout_ref 校验。
        if changed && old_slot >= 0 && old_gen != 0 {
            self.retired_slots.push((old_gen, old_slot, old_epoch));
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
            layout_epoch: 0,
            document_byte_start,
            paragraph_text_len: text.len(),
            height: 0.0,
            visual_line_count: 0,
            // Issue #901 第二十三轮问题1: insert_paragraph 增量路径中新段
            // hard_break 未知，默认 false，由后续 sync 同步。
            hard_break: false,
            relative_lines: Vec::new(),
        };
        let insert_idx = index.min(self.entries.len());
        self.entries.insert(insert_idx, entry);
        // Issue #901 第二十一轮阻塞5: 增量更新 id_to_slot_cache。
        self.id_to_slot_cache_insert(id, insert_idx);
        // Issue #901 第二十一轮问题2: 同步插入行数索引槽位（初始行数 0）。
        Arc::make_mut(&mut self.line_count_index).insert(insert_idx, 0);
    }

    /// 删除段落 `index`（丢弃其 Id）。
    pub fn remove_paragraph(&mut self, index: usize) -> Option<ParagraphLayoutEntry> {
        if index < self.entries.len() {
            let removed = self.entries.remove(index);
            // Issue #901 第二十六轮复核缺口2a: 回收被删除段落的旧 handle。
            // removed 是 owned 值，读取其字段不借用 self，无借用冲突。
            // Issue #901 第二十八轮复核问题3: 携带 epoch 供 release_layout_ref 校验。
            if removed.layout_generation != 0 && removed.cache_slot >= 0 {
                self.retired_slots
                    .push((removed.layout_generation, removed.cache_slot, removed.layout_epoch));
            }
            // Issue #901 第二十一轮阻塞5: 增量更新 id_to_slot_cache。
            self.id_to_slot_cache_remove(removed.id, index);
            // Issue #901 第二十一轮问题2: 同步删除行数索引槽位。
            Arc::make_mut(&mut self.line_count_index).remove(index);
            Some(removed)
        } else {
            None
        }
    }

    // ── 排版结果更新 ──

    /// Issue #901 第二十一轮问题1: 获取或分配段落 `index` 的 Qt 物理槽号。
    ///
    /// physical_slot 与文档段序（logical_slot）无关，由 store 内部分配：
    /// - 如果 entry 已有有效 `cache_slot >= 0`，复用已有值（复用段）。
    /// - 否则通过 `acquire_physical_slot_for_generation` 从 C++ 按 generation 隔离的
    ///   freelist 分配新槽（新段）。
    ///
    /// `layout_generation` 指定槽号所属的 generation，跨 generation 的槽号互不影响。
    ///
    /// # 关键不变量
    ///
    /// 同一 generation 下，不同 ParagraphId 的 physical_slot 互不相同。
    /// 新段分配新槽，不会覆盖仍存活段的 QTextLayout。
    ///
    /// 越界返回 -1。
    pub fn paragraph_physical_slot(&mut self, index: usize, layout_generation: u64) -> i32 {
        if index >= self.entries.len() {
            return -1;
        }
        // 已有有效 cache_slot，直接复用。
        if self.entries[index].cache_slot >= 0 {
            return self.entries[index].cache_slot;
        }
        // Issue #901 第二十八轮复核问题2: 从按 generation 隔离的 freelist 分配槽号。
        // 不再传 min_slot 高水位门槛，C++ freelist 取任意空闲槽复用。
        // 同一代内不会有两个段落同时使用同一 slot：复用段已在上方返回，此处仅新段分配。
        let slot = super::qt_cache::acquire_physical_slot_for_generation(layout_generation);
        self.entries[index].cache_slot = slot;
        // Issue #901 第二十九轮复核阻塞2: 不再 add_layout_ref（layout 尚未写入，
        // 不应增加引用）。acquire 时 epoch 已设，用 get_layout_epoch 读取并记录到 entry。
        // layout 写入由 install_layout_slot_gen 设 refcount=1（Store 初始引用）。
        // 全篇和增量路径统一：acquire(分配槽,refcount=0) → install(写入layout,refcount=1) → Store 记录 epoch。
        let epoch = super::qt_cache::get_layout_epoch(layout_generation, slot);
        self.entries[index].layout_epoch = epoch;
        slot
    }

    /// 更新段落 `index` 的排版结果。
    ///
    /// 排版完成后，记录 generation、cache_slot、高度、视觉行数等信息。
    ///
    /// Issue #901 第二十一轮问题1: `cache_slot` 参数被忽略，physical_slot 由 store
    /// 内部分配。旧实现用 logical_slot 当 physical_slot，新段会覆盖存活段的
    /// QTextLayout。现在复用段保持已有 physical_slot，新段（`cache_slot < 0`）
    /// 分配新槽。调用方应先用 [`paragraph_physical_slot`] 获取 physical_slot
    /// 传给 `prepare_paragraph_visual_snapshot` 和设置 `VisualLine.cache_slot`。
    pub fn update_layout_result(
        &mut self,
        index: usize,
        layout_generation: u64,
        _cache_slot: i32,
        height: f64,
        visual_line_count: usize,
    ) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        // Issue #901 第二十一轮问题1: physical_slot 由 store 内部分配，
        // 忽略传入的 _cache_slot。复用段保持已有值，新段分配新槽。
        // Issue #901 第二十八轮复核问题2: 从按 generation 隔离的 freelist 分配槽号，
        // 不再传 min_slot 高水位门槛，C++ freelist 取任意空闲槽复用。
        if self.entries[index].cache_slot < 0 {
            let slot = super::qt_cache::acquire_physical_slot_for_generation(layout_generation);
            self.entries[index].cache_slot = slot;
            // Issue #901 第二十九轮复核阻塞2: 不再 add_layout_ref（layout 尚未写入）。
            // 用 get_layout_epoch 读取 acquire 时设的 epoch。
            let epoch = super::qt_cache::get_layout_epoch(layout_generation, slot);
            self.entries[index].layout_epoch = epoch;
        }
        let entry = &mut self.entries[index];
        entry.layout_generation = layout_generation;
        entry.height = height;
        entry.visual_line_count = visual_line_count;
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
    /// # 与 ParagraphId / Core patch 的关系
    ///
    /// 本方法按"去掉 `'\n'` 后文本相同即视为同一段"判断公共段，这与
    /// [`rebuild_from_text`] 的 ParagraphId 保留策略（内容相同保留原 Id）
    /// 语义一致：内容相同的段复用 prev 排版，仅 hard_break 可能因前后有无
    /// `'\n'` 而变化（由 full.rs 增量路径同步重建）。
    ///
    /// 当前 Core 仅提供 `affected_byte_start/end`（字节级 patch），不提供段落级
    /// 结构化 patch（哪些段被拆/合/改）。在 Core 提供段落级 patch 前，文本比较
    /// 是当前可得信息下的合理近似。未来若 Core 暴露段落级结构变更，可改为直接
    /// 用稳定 ParagraphId 匹配新旧段，省去全段文本扫描并消除"中间段内容恰好
    /// 与前后段相同"时的误判可能。
    ///
    /// # 复杂度
    ///
    /// O(n)（n 为段落数），但只在段落数变化或内容变化时调用，
    /// 段落数远小于行数，可接受。
    /// Issue #901 第二十三轮问题2: 段落身份比较包含 hard_break。
    /// `(content, hard_break)` 完全相同才视为同一段，避免仅插入/删除末尾回车时
    /// 按去掉 `\n` 后的段落正文比较误判"无变化"。
    pub fn compute_old_new_paragraph_ranges(
        old_paras: &[(&str, bool)],
        new_paras: &[(&str, bool)],
    ) -> ParagraphSpliceRanges {
        let old_count = old_paras.len();
        let new_count = new_paras.len();

        // 找前缀公共段：(content, hard_break) 完全相同
        let mut prefix_common = 0usize;
        let min_len = old_count.min(new_count);
        for i in 0..min_len {
            if old_paras[i] == new_paras[i] {
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
            if old_paras[old_idx] == new_paras[new_idx] {
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
    /// Issue #901 第十一轮问题5: 不再全章枚举 `lines` 组装 HashMap。
    /// 用已有稳定 `ParagraphId → document_byte_start` 索引，对每个受影响段
    /// 用二分查找在 `lines` 中定位 `para_start` 对应的行范围，
    /// O(affected × log(all_lines))，不随全章行数线性增长。
    /// 同时修正了旧实现首行累计计数初始为 0、循环从第二条开始导致单行段
    /// 误统计 `visual_line_count=0` 的 bug。
    /// 并将 `relative_lines` 接入主流程：排版后回填段内相对坐标视觉行。
    ///
    /// Issue #901 第二十一轮问题2: 同时增量更新 `line_count_index`（Fenwick 树）。
    /// 只更新受影响段落的行数（O(k log n)），后缀段的行范围由前缀和自动平移，
    /// 不需要逐段更新绝对行下标。替代旧的 `id_to_line_range` HashMap ——
    /// 旧 HashMap 以全局 visual_lines 下标为绝对值却只对改动段更新，
    /// 换行造成行数变化时全部后缀段范围仍指向旧下标。
    ///
    /// # 参数
    ///
    /// - `lines`：**本次新排版产生的视觉行**（仅受影响段落的行，非全量 visual_lines）。
    ///   Issue #901 第二十二轮问题2: 调用方必须只传新排版的行，不能传 splice 后的
    ///   全量 visual_lines——复用后缀段的 raw logical_slot 可能过期/重复，在全量行上
    ///   二分会把新段和复用旧段的行合为一个段。
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

        // Issue #901 第二十一轮问题2: 先确保 line_count_index 长度匹配段落数。
        // 段落数变化（拆段/合段）时 line_count_index 已在 split/merge 中 insert/remove，
        // 此处 ensure_len 仅在异常情况下补齐。
        Arc::make_mut(&mut self.line_count_index)
            .ensure_len(self.entries.len(), 0);

        // Issue #901 第十五轮问题4: 改成以受影响段索引 + 基礎 VisualLine.logical_slot
        // 定位段内行范围。lines 中的行坐标已经是段内相对的（y/byte/qchar 相对于段首），
        // 直接复制为 relative_lines，不再二次归零（删除旧的 saturating_sub 逻辑）。
        // 不再用 entry.document_byte_start 二分查 lines[].para_start（混合坐标）。
        // Issue #901 第二十轮阻塞1+3: 改用 logical_slot（文档次序）而非 cache_slot。
        // 注意：此处 lines 是本次新排版的视觉行，其 logical_slot 是新段索引（最新），
        // 不是 prev 中可能过期的 raw logical_slot，二分查找正确。
        for &idx in affected_indices {
            // 遍历 lines，按 logical_slot == idx 分组，找到该段的所有行
            // lines 按 logical_slot 非降序排列（段落顺序 = 行顺序），同一段落的行连续
            let (line_start, line_end) = match find_line_range_by_logical_slot(lines, idx) {
                Some(range) => range,
                None => continue,
            };

            let line_count = line_end - line_start;
            let height: f64 = lines[line_start..line_end].iter().map(|l| l.height).sum();

            // Issue #901 第十五轮问题4: lines 中的行坐标已经是段内相对坐标
            //（y 从段顶 0 起、byte/qchar 相对段首、para_start = 段落编号、id = 段内行索引）。
            // 直接 clone 为 relative_lines，不再二次归零。
            let relative_lines: Vec<VisualLine> = lines[line_start..line_end]
                .iter()
                .cloned()
                .collect();

            if let Some(entry) = self.entries.get_mut(idx) {
                entry.layout_generation = layout_generation;
                // Issue #901 第二十一轮问题1: physical_slot 从 VisualLine.cache_slot 读取。
                // full.rs 增量路径已改为用独立 physical_slot（由 paragraph_physical_slot
                // 分配），不再用 idx as i32（logical_slot）当 physical_slot。
                // 旧实现用 idx as i32 会导致新段覆盖存活段的 QTextLayout。
                let physical_slot = lines[line_start].cache_slot;
                if physical_slot >= 0 {
                    // Issue #901 第二十九轮复核阻塞2: 不再 add_layout_ref（layout 已由
                    // install_layout_slot_gen 设 refcount=1）。用 get_layout_epoch 读取 epoch。
                    // physical_slot 来自 paragraph_physical_slot 预分配，
                    // entry.layout_epoch 已在该方法中设置，此处刷新以防 slot 变化。
                    entry.cache_slot = physical_slot;
                    let epoch = super::qt_cache::get_layout_epoch(layout_generation, physical_slot);
                    entry.layout_epoch = epoch;
                } else if entry.cache_slot < 0 {
                    // 防御性：lines 中 cache_slot 无效时由 store 分配新槽。
                    // Issue #901 第二十八轮复核问题2: 从按 generation 隔离的 freelist 分配，
                    // 不再传 min_slot 高水位门槛。
                    let slot =
                        super::qt_cache::acquire_physical_slot_for_generation(layout_generation);
                    entry.cache_slot = slot;
                    // Issue #901 第二十九轮复核阻塞2: 不再 add_layout_ref（layout 尚未写入）。
                    // 用 get_layout_epoch 读取 acquire 时设的 epoch。
                    let epoch = super::qt_cache::get_layout_epoch(layout_generation, slot);
                    entry.layout_epoch = epoch;
                }
                entry.visual_line_count = line_count;
                entry.height = height;
                entry.relative_lines = relative_lines;
            }
            // Issue #901 第二十一轮问题2: 更新行数前缀索引。
            // 后缀段的行范围由前缀和自动平移，不需要逐段更新绝对行下标。
            Arc::make_mut(&mut self.line_count_index).update_count(idx, line_count);
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
        // Issue #901 第二十六轮复核缺口2a: 先收集所有有效 handle 到局部 Vec，
        // 再 extend 到 retired_slots，避免同时借用 self.entries 和 self.retired_slots。
        // Issue #901 第二十八轮复核问题3: retired_slots 改为 3-tuple，记录 epoch。
        let newly_retired: Vec<(u64, i32, u64)> = self
            .entries
            .iter()
            .filter(|e| e.layout_generation != 0 && e.cache_slot >= 0)
            .map(|e| (e.layout_generation, e.cache_slot, e.layout_epoch))
            .collect();
        self.retired_slots.extend(newly_retired);
        for entry in &mut self.entries {
            entry.layout_generation = 0;
            entry.cache_slot = -1;
            entry.layout_epoch = 0;
            entry.height = 0.0;
            entry.visual_line_count = 0;
        }
        // Issue #901 第二十一轮问题2: 行数索引失效（全 0）。
        let counts = vec![0usize; self.entries.len()];
        Arc::make_mut(&mut self.line_count_index).rebuild(counts);
    }

    /// 使段落 `index` 的布局缓存失效。
    pub fn invalidate_paragraph(&mut self, index: usize) -> bool {
        // Issue #901 第二十六轮复核缺口2a: 先取出旧 handle 再修改，
        // 避免同时借用 self.entries（get_mut）和 self.retired_slots（push）。
        // Issue #901 第二十八轮复核问题3: retired_slots 改为 3-tuple，记录 epoch。
        let old_handle = {
            let Some(entry) = self.entries.get(index) else {
                return false;
            };
            if entry.layout_generation != 0 && entry.cache_slot >= 0 {
                Some((entry.layout_generation, entry.cache_slot, entry.layout_epoch))
            } else {
                None
            }
        };
        if let Some(h) = old_handle {
            self.retired_slots.push(h);
        }
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.layout_generation = 0;
        entry.cache_slot = -1;
        entry.layout_epoch = 0;
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
    /// 按 `logical_slot` 分组，将每个段落的视觉行数、高度、line_start 等信息
    /// 更新到对应的 `ParagraphLayoutEntry`。
    /// Issue #901 第二十轮阻塞1+3: 改用 logical_slot（文档次序）而非 cache_slot（Qt 物理槽）。
    /// Issue #901 第二十一轮问题2: 同时全量重建 `line_count_index`（Fenwick 树）。
    pub fn sync_from_visual_lines(&mut self, lines: &[VisualLine], layout_generation: u64) {
        if lines.is_empty() {
            for entry in &mut self.entries {
                entry.layout_generation = layout_generation;
                entry.visual_line_count = 0;
                entry.height = 0.0;
            }
            // Issue #901 第二十一轮问题2: 重建行数索引（全 0）。
            let counts = vec![0usize; self.entries.len()];
            Arc::make_mut(&mut self.line_count_index).rebuild(counts);
            return;
        }

        // 按 logical_slot 分组统计
        // Issue #901 第二十九轮复核阻塞1: 同时记录 physical_slot（从 VisualLine.cache_slot 读取），
        // 不再用 logical_slot 当 physical_slot。全篇排版路径已通过 acquire 分配物理槽。
        let mut slot_stats: Vec<(i32, i32, usize, f64, usize)> = Vec::new();
        // (logical_slot, physical_slot, line_count, total_height, first_line_index)

        let mut current_slot: i32 = lines[0].logical_slot;
        let mut current_physical_slot: i32 = lines[0].cache_slot;
        let mut count = 0usize;
        let mut height = 0.0f64;
        let mut first_idx = 0usize;

        for (i, line) in lines.iter().enumerate() {
            if line.logical_slot != current_slot {
                slot_stats.push((current_slot, current_physical_slot, count, height, first_idx));
                current_slot = line.logical_slot;
                current_physical_slot = line.cache_slot;
                count = 0;
                height = 0.0;
                first_idx = i;
            }
            count += 1;
            height += line.height;
        }
        slot_stats.push((current_slot, current_physical_slot, count, height, first_idx));

        // 更新对应段落
        for (slot, physical_slot, line_count, total_height, _line_start) in slot_stats {
            if slot >= 0 && (slot as usize) < self.entries.len() {
                let entry = &mut self.entries[slot as usize];
                entry.layout_generation = layout_generation;
                // Issue #901 第二十九轮复核阻塞1+2: 从 VisualLine.cache_slot 读取物理槽，
                // 不再用 logical_slot 当物理槽。全篇排版路径已通过 acquire 分配物理槽
                // 并传给 VisualLine.cache_slot。
                // Issue #901 第二十九轮复核阻塞2: 不再 add_layout_ref（layout 已由
                // install_layout_slot_gen 设 refcount=1）。用 get_layout_epoch 读取 epoch。
                entry.cache_slot = physical_slot;
                let epoch = super::qt_cache::get_layout_epoch(layout_generation, physical_slot);
                entry.layout_epoch = epoch;
                entry.visual_line_count = line_count;
                entry.height = total_height;
            }
        }

        // Issue #901 第二十一轮问题2: 从 entries 全量重建行数索引。
        // 用 entries 的 visual_line_count 作为每段行数，保证 line_count_index
        // 与 entries 同步。后缀段行范围由前缀和自动平移。
        let counts: Vec<usize> = self
            .entries
            .iter()
            .map(|e| e.visual_line_count)
            .collect();
        Arc::make_mut(&mut self.line_count_index).rebuild(counts);

        // Issue #901 第二十八轮复核问题2: next_physical_slot 已移除，槽号复用完全由
        // C++ LayoutGeneration 的 freelist 管理，无需在此同步高水位。
    }

    /// 获取所有段落的 `ParagraphId` 列表（按文档顺序）。
    pub fn paragraph_ids(&self) -> Vec<ParagraphId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// 获取所有段落的高度列表（按文档顺序）。
    pub fn paragraph_heights(&self) -> Vec<f64> {
        self.entries.iter().map(|e| e.height).collect()
    }

    // ── Issue #901 第十轮问题5: 段内相对视觉行分块储存 ──

    /// 获取段落 `index` 的段内相对视觉行（只读引用）。
    ///
    /// 段内相对坐标：y 从段顶 0 起，byte/qchar 从段首 0 起。
    /// 绝对坐标由消费方通过 [`ParagraphHeightIndex`] 段落前缀偏移按需解析。
    pub fn relative_lines(&self, index: usize) -> Option<&[VisualLine]> {
        self.entries.get(index).map(|e| e.relative_lines.as_slice())
    }

    /// 获取段落 `index` 的段内相对视觉行（可变引用）。
    pub fn relative_lines_mut(&mut self, index: usize) -> Option<&mut Vec<VisualLine>> {
        self.entries.get_mut(index).map(|e| &mut e.relative_lines)
    }

    /// 设置段落 `index` 的段内相对视觉行。
    ///
    /// 排版完成后回填。同时更新 `visual_line_count` 和 `height`。
    pub fn set_relative_lines(
        &mut self,
        index: usize,
        lines: Vec<VisualLine>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        entry.visual_line_count = lines.len();
        entry.height = lines.iter().map(|l| l.height).sum();
        entry.relative_lines = lines;
        true
    }

    /// Issue #901 第二十三轮问题4: 从 `line_count_index` 动态查询行范围，
    /// 不再消费 `entry.line_start`（切片内局部下标会被误当全章行索引）。
    ///
    /// 返回段落 `index` 在扁平 `visual_lines` 数组中的行范围
    /// `[line_start, line_start + visual_line_count)`。
    /// 用于增量排版路径代替 `build_para_start_to_line_range` 的 O(all_lines) 建图。
    ///
    /// 越界或未排版返回 `(0, 0)`。
    pub fn line_range_of_paragraph(&self, index: usize) -> (usize, usize) {
        self.line_count_index.paragraph_line_range(index)
    }

    /// Issue #901 第十轮问题5: 批量更新后续段落的 `document_byte_start`。
    ///
    /// 段前插入/删除导致后续段落 byte 起点平移 `delta`。
    /// 只改 `document_byte_start`（段落级元数据），不逐行改 visual_lines，
    /// 也不改 `cache_key` / `ParagraphId` / `relative_lines`。
    /// 绝对坐标在消费时由偏移索引按需解析。
    pub fn shift_suffix_document_byte_starts(&mut self, start_index: usize, delta: i64) {
        let len = self.entries.len();
        for entry in &mut self.entries[start_index.min(len)..] {
            entry.document_byte_start = (entry.document_byte_start as i64 + delta) as usize;
        }
    }
}

/// Issue #901 第十五轮问题4: 用二分查找在已排序的 visual_lines 中定位
/// `logical_slot` 对应的行范围，O(log n)。
///
/// `lines` 按 `logical_slot` 非降序排列（段落顺序 = 行顺序），
/// 同一段落的所有行 `logical_slot` 相同且连续。
/// 返回 `(line_start, line_end)` 半开区间，未找到返回 `None`。
/// Issue #901 第二十轮阻塞1+3: 改用 logical_slot（文档次序）而非 cache_slot（Qt 物理槽）。
fn find_line_range_by_logical_slot(
    lines: &[VisualLine],
    logical_slot: usize,
) -> Option<(usize, usize)> {
    if lines.is_empty() {
        return None;
    }
    // 二分查找第一个 logical_slot >= target 的行
    let start = lines.partition_point(|vl| (vl.logical_slot as usize) < logical_slot);
    if start >= lines.len() || (lines[start].logical_slot as usize) != logical_slot {
        return None;
    }
    // 二分查找段落后第一个 logical_slot != target 的行
    let end = lines[start..]
        .partition_point(|vl| (vl.logical_slot as usize) == logical_slot)
        + start;
    Some((start, end))
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
        store.update_layout_result(0, 42, 0, 20.0, 1);
        store.update_layout_result(1, 42, 1, 25.0, 1);

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
        store.update_layout_result(0, 1, 0, 20.0, 1);
        let different_key = make_key("world");
        assert!(store.needs_relayout(0, &different_key));
    }

    #[test]
    fn no_relayout_after_layout_result() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("hello", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let key = store.entry(0).unwrap().cache_key.clone();
        store.update_layout_result(0, 1, 0, 20.0, 1);
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
        store.update_layout_result(0, 1, 0, 20.0, 1);
        store.update_layout_result(1, 1, 1, 20.0, 1);
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

    /// Issue #901 第十六轮问题1: 验证中间插段时后缀段正确继承原 ParagraphId。
    ///
    /// 反例：正文"甲\n乙"在"甲"前插入回车 → "\n甲\n乙"（3 段）。
    /// 旧 entries = [甲, 乙]。按下标对齐会把"甲"的 Id 给新段[0]（空行），
    /// 把"乙"的 Id 给新段[1]（"甲"），新段[2]（"乙"）分配新 Id —— 身份错位。
    /// 按前缀+后缀匹配：前缀 0，后缀 2（甲==甲, 乙==乙），
    /// 新段[0] 分配新 Id，新段[1] 继承旧[0] 的 Id，新段[2] 继承旧[1] 的 Id。
    #[test]
    fn rebuild_prefix_suffix_match_preserves_suffix_ids() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("甲\n乙", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id_jia = store.paragraph_id_at(0).unwrap();
        let id_yi = store.paragraph_id_at(1).unwrap();

        // 在"甲"前插入回车 → "\n甲\n乙"（3段）
        store.rebuild_from_text("\n甲\n乙", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.paragraph_count(), 3);

        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();
        let id2 = store.paragraph_id_at(2).unwrap();

        // 空行段分配新 Id，不继承甲或乙的 Id
        assert_ne!(id0, id_jia, "空行段不应继承甲的Id");
        assert_ne!(id0, id_yi, "空行段不应继承乙的Id");
        // 甲段继承旧甲的 Id
        assert_eq!(id1, id_jia, "甲段应继承旧甲的Id");
        // 乙段继承旧乙的 Id
        assert_eq!(id2, id_yi, "乙段应继承旧乙的Id");
    }

    /// Issue #901 第十六轮问题1: 验证 paragraph_id_to_slot 映射方法。
    #[test]
    fn paragraph_id_to_slot_mapping() {
        let mut store = ParagraphLayoutStore::new();
        store.rebuild_from_text("aaa\nbbb\nccc", 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        let id0 = store.paragraph_id_at(0).unwrap();
        let id1 = store.paragraph_id_at(1).unwrap();
        let id2 = store.paragraph_id_at(2).unwrap();

        assert_eq!(store.paragraph_id_to_slot(id0), Some(0));
        assert_eq!(store.paragraph_id_to_slot(id1), Some(1));
        assert_eq!(store.paragraph_id_to_slot(id2), Some(2));
        assert_eq!(store.paragraph_id_to_slot(ParagraphId::INVALID), None);
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
                logical_slot: 0,
                paragraph_id: ParagraphId::INVALID,
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
                logical_slot: 1,
                paragraph_id: ParagraphId::INVALID,
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

    // ── Issue #901 第五轮阻塞2 + 第二十三轮问题2: compute_old_new_paragraph_ranges 测试 ──
    // 段落身份比较包含 hard_break，(content, hard_break) 完全相同才视为同一段。

    #[test]
    fn compute_old_new_ranges_enter_split() {
        // Enter 拆段：旧 1 段 "helloworld" → 新 2 段 "hello" + "world"
        let old: Vec<(&str, bool)> = vec![("helloworld", false)];
        let new: Vec<(&str, bool)> = vec![("hello", true), ("world", false)];
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
        let old: Vec<(&str, bool)> = vec![("hello", true), ("world", false)];
        let new: Vec<(&str, bool)> = vec![("helloworld", false)];
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
        let old: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", true), ("ccc", false)];
        let new: Vec<(&str, bool)> = vec![("aaa", true), ("BBB", true), ("ccc", false)];
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
        // 末尾新增段落：old "aaa\nbbb" → new "aaa\nbbb\nccc"
        // Issue #901 第二十三轮问题2: "bbb" 的 hard_break 从 false→true（后面新增了换行），
        // 这是结构变化，应被检测为不同段。前缀公共 1 段，后缀 0 段。
        let old: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", false)];
        let new: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", true), ("ccc", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 1 段（"bbb" hard_break 变化），后缀 0 段
        // 旧删 [1,2)，新建 [1,3)
        assert_eq!(ranges.old_splice_start, 1);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 1);
        assert_eq!(ranges.new_layout_end, 3);
    }

    #[test]
    fn compute_old_new_ranges_remove_trailing() {
        // 末尾删除段落：old "aaa\nbbb\nccc" → new "aaa\nbbb"
        // Issue #901 第二十三轮问题2: "bbb" 的 hard_break 从 true→false（后面删了换行），
        // 这是结构变化，应被检测为不同段。前缀公共 1 段，后缀 0 段。
        let old: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", true), ("ccc", false)];
        let new: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 1 段（"bbb" hard_break 变化），后缀 0 段
        // 旧删 [1,3)，新建 [1,2)
        assert_eq!(ranges.old_splice_start, 1);
        assert_eq!(ranges.old_splice_end, 3);
        assert_eq!(ranges.new_layout_start, 1);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_no_change() {
        // 无变化：old "aaa\nbbb" → new "aaa\nbbb"，hard_break 完全一致
        let old: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", false)];
        let new: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 全部是前缀公共，旧删和新建都是空范围
        assert_eq!(ranges.old_splice_start, 2);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 2);
        assert_eq!(ranges.new_layout_end, 2);
    }

    #[test]
    fn compute_old_new_ranges_insert_newline_between() {
        // 在第一、二段之间新插空行：old "aaa\nbbb\nccc" → new "aaa\n\nbbb\nccc"
        let old: Vec<(&str, bool)> = vec![("aaa", true), ("bbb", true), ("ccc", false)];
        let new: Vec<(&str, bool)> = vec![("aaa", true), ("", true), ("bbb", true), ("ccc", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        // 前缀公共 1 段（"aaa"），后缀公共 2 段（"bbb","ccc"）
        // 旧删 [1,1)（空），新建 [1,2)（空行）
        assert_eq!(ranges.old_splice_start, 1);
        assert_eq!(ranges.old_splice_end, 1);
        assert_eq!(ranges.new_layout_start, 1);
        assert_eq!(ranges.new_layout_end, 2);
    }

    /// Issue #901 第二十三轮问题2: 验证仅插入末尾回车（甲 → 甲\n）被检测为有变化。
    /// old "甲"（1段 hb=false）→ new "甲\n"（2段 hb=[true,false]）。
    /// "甲" 的 hard_break 从 false→true，前缀 0，后缀 0，旧删 [0,1)，新建 [0,2)。
    #[test]
    fn compute_old_new_ranges_insert_trailing_newline() {
        let old: Vec<(&str, bool)> = vec![("甲", false)];
        let new: Vec<(&str, bool)> = vec![("甲", true), ("", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        assert_eq!(ranges.old_splice_start, 0);
        assert_eq!(ranges.old_splice_end, 1);
        assert_eq!(ranges.new_layout_start, 0);
        assert_eq!(ranges.new_layout_end, 2);
    }

    /// Issue #901 第二十三轮问题2: 验证仅删除末尾回车（甲\n → 甲）被检测为有变化。
    #[test]
    fn compute_old_new_ranges_delete_trailing_newline() {
        let old: Vec<(&str, bool)> = vec![("甲", true), ("", false)];
        let new: Vec<(&str, bool)> = vec![("甲", false)];
        let ranges = ParagraphLayoutStore::compute_old_new_paragraph_ranges(&old, &new);
        assert_eq!(ranges.old_splice_start, 0);
        assert_eq!(ranges.old_splice_end, 2);
        assert_eq!(ranges.new_layout_start, 0);
        assert_eq!(ranges.new_layout_end, 1);
    }
}

/// 辅助 trait：检查段落是否需要重排（仅供测试）。
#[cfg(test)]
impl ParagraphLayoutEntry {
    fn needs_relayout_check(&self) -> bool {
        !self.is_laid_out()
    }
}
