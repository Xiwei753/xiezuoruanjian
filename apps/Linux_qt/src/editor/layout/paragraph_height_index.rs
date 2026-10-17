//! 段落高度索引 — 分块前缀和保存段落前缀高度。
//!
//! Issue #901: 增量排版子系统。
//!
//! # 核心设计
//!
//! - 用分块前缀和（[`ChunkedPrefixSum`]）维护段落高度的前缀和，
//!   支持 O(B + n/B) 的局部 splice 和前缀和查询。
//! - **整章纵向平移不需要遍历全文改每段 y** — y 由前缀和计算，不逐段存储。
//! - **段落高度变化只改受影响段的数据** — 分块前缀和单点更新 O(B + n/B)。
//! - **段落插入/删除只 splice 受影响块** — O(B + n/B)，不再 O(n) 重建 BIT。
//!
//! Issue #901 第三十三轮问题3: 从 Fenwick BIT（数组）改为分块前缀和。
//! BIT 的中间插删需 O(n) 重建（索引编码子树区间），分块前缀和的中间插删
//! 只影响受影响块 O(B + n/B)。splice 从 O(n) 降到 O(sqrt(n))，
//! prefix_sum 从 O(log n) 升到 O(sqrt(n))，整体编辑体验显著改善。
//!
//! # 操作复杂度
//!
//! | 操作 | 复杂度 |
//! |------|--------|
//! | `insert` | O(B + n/B) |
//! | `remove` | O(B + n/B) |
//! | `splice` | O(B + n/B) |
//! | `update_height` | O(B + n/B) |
//! | `y_to_paragraph` | O(B + n/B) |
//! | `paragraph_top_y` | O(n/B) |
//! | `total_height` | O(1) |
//! | `height_at` | O(n/B) |

use super::chunked_vec::ChunkedPrefixSum;

/// 段落高度索引 — 分块前缀和保存段落前缀高度。
///
/// 维护一个高度数组的分块前缀和。
/// `prefix_sum(k)` = `heights[0] + heights[1] + ... + heights[k-1]`（前 k 个段落的高度和）。
///
/// 段落 `i`（0-indexed）的 y 范围：`[paragraph_top_y(i), paragraph_top_y(i) + heights[i])`。
#[derive(Clone, Debug, Default)]
pub struct ParagraphHeightIndex {
    /// 分块前缀和（f64 高度）。
    cps: ChunkedPrefixSum<f64>,
}

impl ParagraphHeightIndex {
    /// 创建空的高度索引。
    pub fn new() -> Self {
        Self {
            cps: ChunkedPrefixSum::new(),
        }
    }

    /// 从高度数组创建高度索引。
    pub fn from_heights(heights: Vec<f64>) -> Self {
        Self {
            cps: ChunkedPrefixSum::from_values(heights),
        }
    }

    /// 段落数量。
    pub fn len(&self) -> usize {
        self.cps.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cps.is_empty()
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.cps.clear();
    }

    /// 总高度（所有段落高度之和）。
    pub fn total_height(&self) -> f64 {
        self.cps.total_sum()
    }

    /// 获取段落 `index` 的高度。越界返回 0.0。
    pub fn height_at(&self, index: usize) -> f64 {
        self.cps.get(index).unwrap_or(0.0)
    }

    // ── 查询操作 ──

    /// 查段顶 — 段落 `index` 的顶部 y 坐标（文档坐标系）。
    ///
    /// 即 `heights[0] + heights[1] + ... + heights[index-1]`。
    /// `index == 0` 返回 0.0（文档顶部）。越界返回总高度。
    pub fn paragraph_top_y(&self, index: usize) -> f64 {
        if index >= self.cps.len() {
            return self.cps.total_sum();
        }
        self.cps.prefix_sum(index)
    }

    /// 查段底 — 段落 `index` 的底部 y 坐标（文档坐标系）。
    ///
    /// 即 `paragraph_top_y(index) + heights[index]`。
    pub fn paragraph_bottom_y(&self, index: usize) -> f64 {
        self.paragraph_top_y(index) + self.height_at(index)
    }

    /// 查 y→段落 — 找到包含 y 坐标的段落索引（0-indexed）。
    ///
    /// 段落 `i` 的 y 范围：`[paragraph_top_y(i), paragraph_bottom_y(i))`。
    /// - `y < 0` → 返回 0（第一个段落）
    /// - `y >= total_height` → 返回最后一个段落
    /// - 空索引 → 返回 0
    ///
    /// 使用分块前缀和上的累积查找，O(B + n/B)。
    pub fn y_to_paragraph(&self, y: f64) -> usize {
        if self.cps.is_empty() {
            return 0;
        }
        if y < 0.0 {
            return 0;
        }
        let total = self.cps.total_sum();
        if y >= total {
            return self.cps.len() - 1;
        }
        // find_by_accumulated 返回最大 index 使得 prefix_sum(index) <= y。
        let pos = self.cps.find_by_accumulated(y);
        pos.min(self.cps.len().saturating_sub(1))
    }

    // ── 更新操作 ──

    /// 改段高 — 更新段落 `index` 的高度为 `new_height`。
    ///
    /// O(B + n/B)。只改受影响段的数据，不影响其他段落的相对位置。
    /// 返回旧高度，越界返回 `None`。
    pub fn update_height(&mut self, index: usize, new_height: f64) -> Option<f64> {
        self.cps.update(index, new_height)
    }

    /// 段落插入 — 在 `index` 处插入高度为 `height` 的新段落。
    ///
    /// O(B + n/B)。后续段落的 y 坐标自动右移（由前缀和计算）。
    pub fn insert(&mut self, index: usize, height: f64) {
        let insert_pos = index.min(self.cps.len());
        self.cps.insert(insert_pos, height);
    }

    /// 段落删除 — 删除段落 `index`。
    ///
    /// O(B + n/B)。后续段落的 y 坐标自动左移。
    /// 返回被删除段落的高度，越界返回 `None`。
    pub fn remove(&mut self, index: usize) -> Option<f64> {
        self.cps.remove(index)
    }

    /// 批量更新高度 — 从高度数组重建整个索引。
    ///
    /// 用于全篇重排后一次性更新所有段落高度。
    pub fn rebuild(&mut self, heights: Vec<f64>) {
        self.cps.rebuild(heights);
    }

    /// Issue #901 第三十轮问题4: 局部 splice — 删除 `[start, end)` 范围的段落高度，
    /// 在 `start` 处插入新段落高度 `new_heights`。
    ///
    /// 前缀段 `[0, start)` 和后缀段 `[end, len)` 的高度数据保留不动，
    /// 只替换中间段。
    ///
    /// Issue #901 第三十三轮问题3: 用分块前缀和的局部 splice 替代
    /// `Vec::splice + rebuild_bit`（O(n)）。分块前缀和的 splice 只影响
    /// 受影响块 O(B + n/B)，不再全量重建 BIT。
    pub fn splice(&mut self, start: usize, end: usize, new_heights: Vec<f64>) {
        self.cps.splice(start, end, new_heights);
    }

    /// 批量更新单个段落高度（不重建，逐个 `update_height`）。
    ///
    /// 当多个段落高度变化时，逐个调用 `update_height`。
    pub fn update_heights(&mut self, updates: &[(usize, f64)]) {
        for &(index, new_height) in updates {
            self.cps.update(index, new_height);
        }
    }

    /// Issue #901: 确保索引长度匹配段落数，只在段落数变化时才重建。
    ///
    /// 返回 `true` 表示发生了重建（段落数变化），`false` 表示长度已匹配无需重建。
    pub fn ensure_len(&mut self, expected_len: usize, default_height: f64) -> bool {
        self.cps.ensure_len(expected_len, default_height)
    }

    /// Issue #901: 增量更新受影响段落的高度。
    ///
    /// 对 `updates` 中列出的段落逐个调用 `update_height`，
    /// 不扫描全章。只在段落数量不匹配时才调用 `ensure_len` 重建。
    pub fn update_heights_incremental(
        &mut self,
        updates: &[(usize, f64)],
        expected_len: usize,
        default_height: f64,
    ) {
        // 先确保长度匹配
        self.cps.ensure_len(expected_len, default_height);
        // 逐个更新受影响段落
        for &(index, new_height) in updates {
            self.cps.update(index, new_height);
        }
    }

    /// Issue #901 第二轮复核: 段落数变化时增量更新 — 只在结构变化位置插入/删除高度，
    /// 保留未变化段的高度数据。
    pub fn update_for_structural_change(
        &mut self,
        insert_at: Option<(usize, f64)>,
        remove_at: Option<usize>,
        height_updates: &[(usize, f64)],
    ) {
        // 先处理结构变化（插入/删除），保留未变化段高度
        if let Some((pos, height)) = insert_at {
            self.insert(pos, height);
        }
        if let Some(pos) = remove_at {
            self.remove(pos);
        }
        // 再更新受影响段落的高度
        for &(index, new_height) in height_updates {
            self.cps.update(index, new_height);
        }
    }

    /// 获取段落 `index` 的 y 范围 `(top_y, bottom_y)`。
    ///
    /// 越界返回 `None`。
    pub fn paragraph_y_range(&self, index: usize) -> Option<(f64, f64)> {
        if index >= self.cps.len() {
            return None;
        }
        let top = self.paragraph_top_y(index);
        let bottom = top + self.height_at(index);
        Some((top, bottom))
    }

    /// Issue #901 第五轮阻塞5: 枚举 y 范围内的段落索引范围。
    ///
    /// 返回 `[start, end)`，使得段落 `start` 是第一个与 `[y_start, y_end]`
    /// 相交的段落，段落 `end - 1` 是最后一个相交的段落。
    pub fn paragraphs_in_y_range(&self, y_start: f64, y_end: f64) -> (usize, usize) {
        if self.cps.is_empty() || y_end < 0.0 {
            return (0, 0);
        }
        // Issue #901 第五轮阻塞2: 防御性处理 y_start > y_end。
        let (y_start, y_end) = if y_start > y_end {
            (y_end, y_start)
        } else {
            (y_start, y_end)
        };
        let start = self.y_to_paragraph(y_start);
        let end = (self.y_to_paragraph(y_end) + 1).min(self.cps.len());
        if start >= end {
            return (start, start);
        }
        (start, end)
    }
}

// ── Issue #901 第二十一轮问题2: 段落行数前缀索引 ──

/// 段落行数索引 — 分块前缀和保存段落前缀视觉行数。
///
/// Issue #901 第三十三轮问题3: 从 Fenwick BIT 改为分块前缀和。
///
/// # 操作复杂度
///
/// | 操作 | 复杂度 |
/// |------|--------|
/// | `insert` | O(B + n/B) |
/// | `remove` | O(B + n/B) |
/// | `splice` | O(B + n/B) |
/// | `update_count` | O(B + n/B) |
/// | `paragraph_line_start` | O(n/B) |
/// | `total_lines` | O(1) |
/// | `count_at` | O(n/B) |
#[derive(Clone, Debug, Default)]
pub struct ParagraphLineCountIndex {
    /// 分块前缀和（usize 行数）。
    cps: ChunkedPrefixSum<usize>,
}

impl ParagraphLineCountIndex {
    /// 创建空的行数索引。
    pub fn new() -> Self {
        Self {
            cps: ChunkedPrefixSum::new(),
        }
    }

    /// 从行数数组创建行数索引。
    pub fn from_counts(counts: Vec<usize>) -> Self {
        Self {
            cps: ChunkedPrefixSum::from_values(counts),
        }
    }

    /// 段落数量。
    pub fn len(&self) -> usize {
        self.cps.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cps.is_empty()
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.cps.clear();
    }

    /// 总行数（所有段落行数之和）。
    pub fn total_lines(&self) -> usize {
        self.cps.total_sum()
    }

    /// 获取段落 `index` 的行数。越界返回 0。
    pub fn count_at(&self, index: usize) -> usize {
        self.cps.get(index).unwrap_or(0)
    }

    // ── 查询操作 ──

    /// 查段首行索引 — 段落 `index` 的首行在扁平 `visual_lines` 数组中的下标。
    ///
    /// 即 `counts[0] + counts[1] + ... + counts[index-1]`。
    /// `index == 0` 返回 0（首段首行）。越界返回总行数。
    pub fn paragraph_line_start(&self, index: usize) -> usize {
        if index >= self.cps.len() {
            return self.cps.total_sum();
        }
        self.cps.prefix_sum(index)
    }

    /// 查段尾行索引 — 段落 `index` 的尾后行在扁平 `visual_lines` 数组中的下标。
    ///
    /// 即 `paragraph_line_start(index) + counts[index]`。
    pub fn paragraph_line_end(&self, index: usize) -> usize {
        self.paragraph_line_start(index) + self.count_at(index)
    }

    /// 段落 `index` 的行范围 `[start, end)`。
    ///
    /// 越界返回 `(total, total)`。
    pub fn paragraph_line_range(&self, index: usize) -> (usize, usize) {
        let start = self.paragraph_line_start(index);
        let end = start + self.count_at(index);
        (start, end)
    }

    // ── 更新操作 ──

    /// 改段行数 — 更新段落 `index` 的行数为 `new_count`。
    ///
    /// O(B + n/B)。只改受影响段的数据，不影响其他段落的相对行范围。
    /// 返回旧行数，越界返回 `None`。
    pub fn update_count(&mut self, index: usize, new_count: usize) -> Option<usize> {
        self.cps.update(index, new_count)
    }

    /// 段落插入 — 在 `index` 处插入行数为 `count` 的新段落。
    ///
    /// O(B + n/B)。后续段落的行范围自动右移。
    pub fn insert(&mut self, index: usize, count: usize) {
        let insert_pos = index.min(self.cps.len());
        self.cps.insert(insert_pos, count);
    }

    /// 段落删除 — 删除段落 `index`。
    ///
    /// O(B + n/B)。后续段落的行范围自动左移。
    /// 返回被删除段落的行数，越界返回 `None`。
    pub fn remove(&mut self, index: usize) -> Option<usize> {
        self.cps.remove(index)
    }

    /// 批量更新行数 — 从行数数组重建整个索引。
    pub fn rebuild(&mut self, counts: Vec<usize>) {
        self.cps.rebuild(counts);
    }

    /// Issue #901 第三十轮问题4: 局部 splice — 删除 `[start, end)` 范围的段落行数，
    /// 在 `start` 处插入新段落行数 `new_counts`。
    ///
    /// 前缀段 `[0, start)` 和后缀段 `[end, len)` 的行数保留不动。
    ///
    /// Issue #901 第三十三轮问题3: 用分块前缀和的局部 splice 替代
    /// `Vec::splice + rebuild_bit`（O(n)）。
    pub fn splice(&mut self, start: usize, end: usize, new_counts: Vec<usize>) {
        self.cps.splice(start, end, new_counts);
    }

    /// Issue #901: 确保索引长度匹配段落数，只在段落数变化时才重建。
    ///
    /// 返回 `true` 表示发生了重建，`false` 表示长度已匹配无需重建。
    pub fn ensure_len(&mut self, expected_len: usize, default_count: usize) -> bool {
        self.cps.ensure_len(expected_len, default_count)
    }

    /// Issue #901: 增量更新受影响段落的行数。
    ///
    /// 对 `updates` 中列出的段落逐个调用 `update_count`。
    /// 只在段落数量不匹配时才调用 `ensure_len` 重建。
    pub fn update_counts_incremental(
        &mut self,
        updates: &[(usize, usize)],
        expected_len: usize,
        default_count: usize,
    ) {
        self.cps.ensure_len(expected_len, default_count);
        for &(index, new_count) in updates {
            self.cps.update(index, new_count);
        }
    }
}

// ── Issue #901 第十轮问题5: 段落前缀偏移索引 ──

/// 段落前缀偏移索引 — 维护段落文本长度（byte 和 qchar）的前缀和。
///
/// Issue #901 第三十三轮问题3: 从 Fenwick BIT 改为分块前缀和。
/// byte 和 qchar 各用一个 `ChunkedPrefixSum<usize>`。
///
/// # 操作复杂度
///
/// | 操作 | 复杂度 |
/// |------|--------|
/// | `insert` | O(B + n/B) |
/// | `remove` | O(B + n/B) |
/// | `splice` | O(B + n/B) |
/// | `update_paragraph_length` | O(B + n/B) |
/// | `paragraph_byte_start` | O(n/B) |
/// | `byte_to_paragraph` | O(B + n/B) |
/// | `total_byte_length` | O(1) |
#[derive(Clone, Debug, Default)]
pub struct ParagraphOffsetIndex {
    /// byte 长度的分块前缀和。
    byte_cps: ChunkedPrefixSum<usize>,
    /// qchar (UTF-16) 长度的分块前缀和。
    qchar_cps: ChunkedPrefixSum<usize>,
}

impl ParagraphOffsetIndex {
    /// 创建空的偏移索引。
    pub fn new() -> Self {
        Self::default()
    }

    /// 段落数量。
    pub fn len(&self) -> usize {
        self.byte_cps.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.byte_cps.is_empty()
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.byte_cps.clear();
        self.qchar_cps.clear();
    }

    /// 从段落长度数组重建偏移索引。
    ///
    /// `byte_lengths[i]` / `qchar_lengths[i]` 是段落 `i` 的文本长度（含尾部 `'\n'`）。
    pub fn rebuild(&mut self, byte_lengths: Vec<usize>, qchar_lengths: Vec<usize>) {
        debug_assert_eq!(
            byte_lengths.len(),
            qchar_lengths.len(),
            "byte_lengths 和 qchar_lengths 长度必须一致"
        );
        self.byte_cps.rebuild(byte_lengths);
        self.qchar_cps.rebuild(qchar_lengths);
    }

    /// 查询段落 `index` 的文档 byte 起始偏移（前缀和）。
    ///
    /// 即 `byte_lengths[0] + ... + byte_lengths[index-1]`。
    /// `index == 0` 返回 0。越界返回总 byte 长度。
    pub fn paragraph_byte_start(&self, index: usize) -> usize {
        if index >= self.byte_cps.len() {
            self.byte_cps.total_sum()
        } else {
            self.byte_cps.prefix_sum(index)
        }
    }

    /// 查询段落 `index` 的文档 qchar 起始偏移（前缀和）。
    pub fn paragraph_qchar_start(&self, index: usize) -> usize {
        if index >= self.qchar_cps.len() {
            self.qchar_cps.total_sum()
        } else {
            self.qchar_cps.prefix_sum(index)
        }
    }

    /// 获取段落 `index` 的 byte 长度。越界返回 0。
    pub fn byte_length_at(&self, index: usize) -> usize {
        self.byte_cps.get(index).unwrap_or(0)
    }

    /// 获取段落 `index` 的 qchar 长度。越界返回 0。
    pub fn qchar_length_at(&self, index: usize) -> usize {
        self.qchar_cps.get(index).unwrap_or(0)
    }

    /// 更新段落 `index` 的 byte 和 qchar 长度。
    ///
    /// 段前插入/删除只改受影响段的长度，后续段的起始偏移由前缀和自动反映。
    /// 返回 false 表示越界未更新。
    pub fn update_paragraph_length(
        &mut self,
        index: usize,
        byte_length: usize,
        qchar_length: usize,
    ) -> bool {
        if index >= self.byte_cps.len() {
            return false;
        }
        self.byte_cps.update(index, byte_length);
        self.qchar_cps.update(index, qchar_length);
        true
    }

    /// 段落插入 — 在 `index` 处插入长度为 `(byte_length, qchar_length)` 的新段落。
    ///
    /// O(B + n/B)。后续段落的起始偏移自动右移。
    pub fn insert(&mut self, index: usize, byte_length: usize, qchar_length: usize) {
        let pos = index.min(self.byte_cps.len());
        self.byte_cps.insert(pos, byte_length);
        self.qchar_cps.insert(pos, qchar_length);
    }

    /// 段落删除 — 删除段落 `index`。
    ///
    /// O(B + n/B)。后续段落的起始偏移自动左移。
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.byte_cps.len() {
            return false;
        }
        self.byte_cps.remove(index);
        self.qchar_cps.remove(index);
        true
    }

    /// Issue #901 第三十轮问题4: 局部 splice — 删除 `[start, end)` 范围的段落长度，
    /// 在 `start` 处插入新段落长度 `(new_byte_lengths, new_qchar_lengths)`。
    ///
    /// 前缀段 `[0, start)` 和后缀段 `[end, len)` 的长度数据保留不动。
    ///
    /// Issue #901 第三十三轮问题3: 用分块前缀和的局部 splice 替代
    /// `Vec::splice + rebuild_fenwick`（O(n)）。
    pub fn splice(
        &mut self,
        start: usize,
        end: usize,
        new_byte_lengths: Vec<usize>,
        new_qchar_lengths: Vec<usize>,
    ) {
        debug_assert_eq!(
            new_byte_lengths.len(),
            new_qchar_lengths.len(),
            "new_byte_lengths 和 new_qchar_lengths 长度必须一致"
        );
        self.byte_cps.splice(start, end, new_byte_lengths);
        self.qchar_cps.splice(start, end, new_qchar_lengths);
    }

    /// 总 byte 长度（所有段落之和）。O(1)。
    pub fn total_byte_length(&self) -> usize {
        self.byte_cps.total_sum()
    }

    /// 总 qchar 长度（所有段落之和）。O(1)。
    pub fn total_qchar_length(&self) -> usize {
        self.qchar_cps.total_sum()
    }

    /// Issue #901 第十六轮问题2: 查 byte→段落 — 找到包含 byte offset 的段落索引。
    ///
    /// 段落 `i` 的 byte 范围：`[paragraph_byte_start(i), paragraph_byte_start(i+1))`。
    /// - `byte_offset >= total` → 返回最后一个段落
    /// - 空索引 → 返回 0
    ///
    /// 使用分块前缀和上的累积查找，O(B + n/B)。
    pub fn byte_to_paragraph(&self, byte_offset: usize) -> usize {
        let n = self.byte_cps.len();
        if n == 0 {
            return 0;
        }
        if byte_offset >= self.byte_cps.total_sum() {
            return n - 1;
        }
        let pos = self.byte_cps.find_by_accumulated(byte_offset);
        pos.min(n.saturating_sub(1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ── Issue #901 第二十一轮问题2: ParagraphLineCountIndex 测试 ──

    #[test]
    fn line_count_index_empty() {
        let idx = ParagraphLineCountIndex::new();
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        assert_eq!(idx.total_lines(), 0);
        assert_eq!(idx.paragraph_line_start(0), 0);
    }

    #[test]
    fn line_count_index_from_counts_basic() {
        let idx = ParagraphLineCountIndex::from_counts(vec![1, 2, 3]);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.total_lines(), 6);
        assert_eq!(idx.count_at(0), 1);
        assert_eq!(idx.count_at(1), 2);
        assert_eq!(idx.count_at(2), 3);
    }

    #[test]
    fn line_count_index_paragraph_line_range() {
        // 三段各 1,2,3 行 → 行范围 [0,1),[1,3),[3,6)
        let idx = ParagraphLineCountIndex::from_counts(vec![1, 2, 3]);
        assert_eq!(idx.paragraph_line_range(0), (0, 1));
        assert_eq!(idx.paragraph_line_range(1), (1, 3));
        assert_eq!(idx.paragraph_line_range(2), (3, 6));
    }

    #[test]
    fn line_count_index_update_count_prefix_shifts() {
        // Issue #901 第二十一轮问题2 核心场景：
        // 三段各 1 行 → [0,1),[1,2),[2,3)。第一段变成 2 行后，
        // 后缀段范围应自动平移为 [2,3),[3,4)，不需要逐段更新。
        let mut idx = ParagraphLineCountIndex::from_counts(vec![1, 1, 1]);
        assert_eq!(idx.paragraph_line_range(0), (0, 1));
        assert_eq!(idx.paragraph_line_range(1), (1, 2));
        assert_eq!(idx.paragraph_line_range(2), (2, 3));

        idx.update_count(0, 2);
        // 后缀段范围由前缀和自动平移
        assert_eq!(idx.paragraph_line_range(0), (0, 2));
        assert_eq!(idx.paragraph_line_range(1), (2, 3));
        assert_eq!(idx.paragraph_line_range(2), (3, 4));
        assert_eq!(idx.total_lines(), 4);
    }

    #[test]
    fn line_count_index_update_count_decrease() {
        let mut idx = ParagraphLineCountIndex::from_counts(vec![3, 2]);
        assert_eq!(idx.paragraph_line_range(0), (0, 3));
        assert_eq!(idx.paragraph_line_range(1), (3, 5));
        idx.update_count(0, 1);
        assert_eq!(idx.paragraph_line_range(0), (0, 1));
        assert_eq!(idx.paragraph_line_range(1), (1, 3));
    }

    #[test]
    fn line_count_index_insert_remove() {
        let mut idx = ParagraphLineCountIndex::from_counts(vec![1, 3]);
        idx.insert(1, 2);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.paragraph_line_range(1), (1, 3));
        assert_eq!(idx.paragraph_line_range(2), (3, 6));
        idx.remove(1);
        assert_eq!(idx.paragraph_line_range(1), (1, 4));
    }

    #[test]
    fn line_count_index_out_of_bounds() {
        let idx = ParagraphLineCountIndex::from_counts(vec![1, 2]);
        assert_eq!(idx.paragraph_line_start(5), 3); // 越界返回 total
        assert_eq!(idx.count_at(5), 0);
    }

    #[test]
    fn empty_index() {
        let idx = ParagraphHeightIndex::new();
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        assert_eq!(idx.total_height(), 0.0);
        assert_eq!(idx.y_to_paragraph(100.0), 0);
    }

    #[test]
    fn from_heights_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.total_height(), 60.0);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.height_at(2), 30.0);
    }

    #[test]
    fn paragraph_top_y_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        assert_eq!(idx.paragraph_top_y(0), 0.0);
        assert_eq!(idx.paragraph_top_y(1), 10.0);
        assert_eq!(idx.paragraph_top_y(2), 30.0);
        // 越界返回总高度
        assert_eq!(idx.paragraph_top_y(3), 60.0);
    }

    #[test]
    fn paragraph_bottom_y_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        assert_eq!(idx.paragraph_bottom_y(0), 10.0);
        assert_eq!(idx.paragraph_bottom_y(1), 30.0);
        assert_eq!(idx.paragraph_bottom_y(2), 60.0);
    }

    #[test]
    fn y_to_paragraph_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 段落 0: [0, 10)
        assert_eq!(idx.y_to_paragraph(0.0), 0);
        assert_eq!(idx.y_to_paragraph(5.0), 0);
        assert_eq!(idx.y_to_paragraph(9.9), 0);
        // 段落 1: [10, 30)
        assert_eq!(idx.y_to_paragraph(10.0), 1);
        assert_eq!(idx.y_to_paragraph(20.0), 1);
        assert_eq!(idx.y_to_paragraph(29.9), 1);
        // 段落 2: [30, 60)
        assert_eq!(idx.y_to_paragraph(30.0), 2);
        assert_eq!(idx.y_to_paragraph(50.0), 2);
        assert_eq!(idx.y_to_paragraph(59.9), 2);
    }

    #[test]
    fn y_to_paragraph_edge_cases() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        // 负 y → 第一个段落
        assert_eq!(idx.y_to_paragraph(-5.0), 0);
        // y >= total → 最后一个段落
        assert_eq!(idx.y_to_paragraph(30.0), 1);
        assert_eq!(idx.y_to_paragraph(100.0), 1);
    }

    #[test]
    fn y_to_paragraph_single_paragraph() {
        let idx = ParagraphHeightIndex::from_heights(vec![42.0]);
        assert_eq!(idx.y_to_paragraph(0.0), 0);
        assert_eq!(idx.y_to_paragraph(41.0), 0);
        assert_eq!(idx.y_to_paragraph(42.0), 0);
    }

    #[test]
    fn update_height_basic() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        let old = idx.update_height(1, 25.0).unwrap();
        assert_eq!(old, 20.0);
        assert_eq!(idx.height_at(1), 25.0);
        assert_eq!(idx.total_height(), 65.0);
        // 段落 1 的 y 范围变了
        assert_eq!(idx.paragraph_top_y(1), 10.0);
        assert_eq!(idx.paragraph_bottom_y(1), 35.0);
        // 段落 2 的 top 也变了
        assert_eq!(idx.paragraph_top_y(2), 35.0);
    }

    #[test]
    fn update_height_out_of_bounds() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0]);
        assert!(idx.update_height(5, 20.0).is_none());
    }

    #[test]
    fn insert_basic() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 30.0]);
        // 在 index 1 处插入高度 20.0
        idx.insert(1, 20.0);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.height_at(2), 30.0);
        assert_eq!(idx.total_height(), 60.0);
        // 后续段落 y 自动右移
        assert_eq!(idx.paragraph_top_y(2), 30.0);
    }

    #[test]
    fn insert_at_end() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        idx.insert(2, 30.0);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.height_at(2), 30.0);
    }

    #[test]
    fn remove_basic() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        let removed = idx.remove(1).unwrap();
        assert_eq!(removed, 20.0);
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 30.0);
        assert_eq!(idx.total_height(), 40.0);
    }

    #[test]
    fn remove_out_of_bounds() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0]);
        assert!(idx.remove(5).is_none());
    }

    #[test]
    fn rebuild_from_new_heights() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        idx.rebuild(vec![5.0, 15.0, 25.0, 35.0]);
        assert_eq!(idx.len(), 4);
        assert_eq!(idx.total_height(), 80.0);
        assert_eq!(idx.paragraph_top_y(2), 20.0);
    }

    #[test]
    fn update_heights_batch() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        idx.update_heights(&[(0, 15.0), (2, 35.0)]);
        assert_eq!(idx.height_at(0), 15.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.height_at(2), 35.0);
        assert_eq!(idx.total_height(), 70.0);
    }

    #[test]
    fn paragraph_y_range_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        assert_eq!(idx.paragraph_y_range(0), Some((0.0, 10.0)));
        assert_eq!(idx.paragraph_y_range(1), Some((10.0, 30.0)));
        assert_eq!(idx.paragraph_y_range(2), Some((30.0, 60.0)));
        assert_eq!(idx.paragraph_y_range(3), None);
    }

    #[test]
    fn clear_resets_all() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        idx.clear();
        assert!(idx.is_empty());
        assert_eq!(idx.total_height(), 0.0);
    }

    #[test]
    fn y_to_paragraph_after_update() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 10.0, 10.0]);
        // 改段落 1 的高度为 30.0
        idx.update_height(1, 30.0);
        // 段落 0: [0, 10), 段落 1: [10, 40), 段落 2: [40, 50)
        assert_eq!(idx.y_to_paragraph(5.0), 0);
        assert_eq!(idx.y_to_paragraph(10.0), 1);
        assert_eq!(idx.y_to_paragraph(35.0), 1);
        assert_eq!(idx.y_to_paragraph(40.0), 2);
        assert_eq!(idx.y_to_paragraph(45.0), 2);
    }

    #[test]
    fn many_paragraphs() {
        // 测试大量段落的正确性
        let heights: Vec<f64> = (0..100).map(|i| (i + 1) as f64).collect();
        let idx = ParagraphHeightIndex::from_heights(heights);
        // 总高度 = 1+2+...+100 = 5050
        assert_eq!(idx.total_height(), 5050.0);
        // 段落 49 的 top = 1+2+...+49 = 1225
        assert_eq!(idx.paragraph_top_y(49), 1225.0);
        // y=1225 应该在段落 49
        assert_eq!(idx.y_to_paragraph(1225.0), 49);
        // y=1224.9 应该在段落 48
        assert_eq!(idx.y_to_paragraph(1224.9), 48);
    }

    // Issue #901 第三十三轮问题3: lowbit 函数已随 Fenwick BIT 移除，
    // lowbit_function 测试相应删除。

    // ── Issue #901: 增量更新测试 ──

    #[test]
    fn ensure_len_no_change() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 长度已匹配，不需要重建
        assert!(!idx.ensure_len(3, 0.0));
        assert_eq!(idx.total_height(), 60.0);
    }

    #[test]
    fn ensure_len_grow() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        // 长度从 2 增到 4，需要重建
        assert!(idx.ensure_len(4, 15.0));
        assert_eq!(idx.len(), 4);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.height_at(2), 15.0);
        assert_eq!(idx.height_at(3), 15.0);
        assert_eq!(idx.total_height(), 60.0);
    }

    #[test]
    fn ensure_len_shrink() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 长度从 3 减到 2，需要重建
        assert!(idx.ensure_len(2, 0.0));
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.total_height(), 30.0);
    }

    #[test]
    fn update_heights_incremental_no_len_change() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 段落数不变，只更新段落 1 的高度
        idx.update_heights_incremental(&[(1, 25.0)], 3, 0.0);
        assert_eq!(idx.height_at(0), 10.0);
        assert_eq!(idx.height_at(1), 25.0);
        assert_eq!(idx.height_at(2), 30.0);
        assert_eq!(idx.total_height(), 65.0);
    }

    #[test]
    fn update_heights_incremental_with_len_change() {
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        // 段落数从 2 增到 3，同时更新段落 0 的高度
        idx.update_heights_incremental(&[(0, 15.0)], 3, 12.0);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.height_at(0), 15.0);
        assert_eq!(idx.height_at(1), 20.0);
        assert_eq!(idx.height_at(2), 12.0);
        assert_eq!(idx.total_height(), 47.0);
    }

    #[test]
    fn update_for_structural_change_insert_preserves_unchanged() {
        // Issue #901 第二轮复核: Enter 拆段时在中间位置插入高度，
        // 保留未变化段的高度数据。
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 30.0]);
        // 在 index 1 处插入高度 20.0（拆段），并更新原段高度
        idx.update_for_structural_change(Some((1, 20.0)), None, &[(0, 15.0)]);
        assert_eq!(idx.len(), 3);
        // 段落 0 高度被更新为 15.0
        assert_eq!(idx.height_at(0), 15.0);
        // 新插入的段落 1 高度为 20.0
        assert_eq!(idx.height_at(1), 20.0);
        // 原段落 1（现在是段落 2）高度保留为 30.0
        assert_eq!(idx.height_at(2), 30.0);
        assert_eq!(idx.total_height(), 65.0);
    }

    #[test]
    fn update_for_structural_change_remove_preserves_unchanged() {
        // Issue #901 第二轮复核: Backspace 合段时删除中间位置的高度，
        // 保留未变化段的高度数据。
        let mut idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 删除 index 1（合段），并更新合并后段落 0 的高度
        idx.update_for_structural_change(None, Some(1), &[(0, 25.0)]);
        assert_eq!(idx.len(), 2);
        // 合并后段落 0 高度为 25.0
        assert_eq!(idx.height_at(0), 25.0);
        // 段落 1（原段落 2）高度保留为 30.0
        assert_eq!(idx.height_at(1), 30.0);
        assert_eq!(idx.total_height(), 55.0);
    }

    // ── Issue #901 第五轮阻塞5: paragraphs_in_y_range 测试 ──

    #[test]
    fn paragraphs_in_y_range_basic() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 段落 0: [0, 10), 段落 1: [10, 30), 段落 2: [30, 60)
        // 视口 [5, 25] 覆盖段落 0 和 1
        let (start, end) = idx.paragraphs_in_y_range(5.0, 25.0);
        assert_eq!(start, 0);
        assert_eq!(end, 2);
    }

    #[test]
    fn paragraphs_in_y_range_full() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 视口覆盖全部
        let (start, end) = idx.paragraphs_in_y_range(0.0, 60.0);
        assert_eq!(start, 0);
        assert_eq!(end, 3);
    }

    #[test]
    fn paragraphs_in_y_range_single() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0, 30.0]);
        // 视口只在段落 2 内
        let (start, end) = idx.paragraphs_in_y_range(35.0, 45.0);
        assert_eq!(start, 2);
        assert_eq!(end, 3);
    }

    #[test]
    fn paragraphs_in_y_range_empty() {
        let idx = ParagraphHeightIndex::new();
        let (start, end) = idx.paragraphs_in_y_range(0.0, 100.0);
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn paragraphs_in_y_range_negative_y() {
        let idx = ParagraphHeightIndex::from_heights(vec![10.0, 20.0]);
        // 负 y 从段落 0 开始
        let (start, end) = idx.paragraphs_in_y_range(-10.0, 15.0);
        assert_eq!(start, 0);
        assert_eq!(end, 2);
    }

    // ── Issue #901 第十四轮问题1: ParagraphOffsetIndex Fenwick 树测试 ──

    #[test]
    fn offset_index_empty() {
        let idx = ParagraphOffsetIndex::new();
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        assert_eq!(idx.total_byte_length(), 0);
        assert_eq!(idx.total_qchar_length(), 0);
    }

    #[test]
    fn offset_index_rebuild_basic() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![3, 5, 7]);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.total_byte_length(), 15);
        assert_eq!(idx.total_qchar_length(), 15);
    }

    #[test]
    fn offset_index_paragraph_starts() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![2, 4, 6]);
        assert_eq!(idx.paragraph_byte_start(0), 0);
        assert_eq!(idx.paragraph_byte_start(1), 3);
        assert_eq!(idx.paragraph_byte_start(2), 8);
        assert_eq!(idx.paragraph_byte_start(3), 15); // 越界返回总长度

        assert_eq!(idx.paragraph_qchar_start(0), 0);
        assert_eq!(idx.paragraph_qchar_start(1), 2);
        assert_eq!(idx.paragraph_qchar_start(2), 6);
        assert_eq!(idx.paragraph_qchar_start(3), 12); // 越界返回总长度
    }

    #[test]
    fn offset_index_update_paragraph_length() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![3, 5, 7]);
        // 更新段落 1 的长度
        assert!(idx.update_paragraph_length(1, 10, 10));
        assert_eq!(idx.byte_length_at(1), 10);
        assert_eq!(idx.qchar_length_at(1), 10);
        // 段落 2 的起始偏移应自动反映
        assert_eq!(idx.paragraph_byte_start(2), 13); // 3 + 10
        assert_eq!(idx.paragraph_qchar_start(2), 13);
        assert_eq!(idx.total_byte_length(), 20); // 3 + 10 + 7
        assert_eq!(idx.total_qchar_length(), 20);
    }

    #[test]
    fn offset_index_update_paragraph_length_shrink() {
        // 删除文字：new < old，验证 Fenwick 树不会因 wrapping_sub 产生
        // usize::MAX 而在无符号加法中溢出 panic。
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![3, 5, 7]);
        // 段落 1 从 5 缩短到 2（删除 3 byte / 3 qchar）
        assert!(idx.update_paragraph_length(1, 2, 2));
        assert_eq!(idx.byte_length_at(1), 2);
        assert_eq!(idx.qchar_length_at(1), 2);
        // 段落 2 的起始偏移应自动反映：3 + 2 = 5
        assert_eq!(idx.paragraph_byte_start(2), 5);
        assert_eq!(idx.paragraph_qchar_start(2), 5);
        // 总长度：3 + 2 + 7 = 12
        assert_eq!(idx.total_byte_length(), 12);
        assert_eq!(idx.total_qchar_length(), 12);
        // 再次缩短到 0（清空该段内容）
        assert!(idx.update_paragraph_length(1, 0, 0));
        assert_eq!(idx.paragraph_byte_start(2), 3);
        assert_eq!(idx.total_byte_length(), 10);
        // 再变长回来，验证增减分支交替正确
        assert!(idx.update_paragraph_length(1, 8, 8));
        assert_eq!(idx.paragraph_byte_start(2), 11);
        assert_eq!(idx.total_byte_length(), 18);
    }

    #[test]
    fn offset_index_update_paragraph_length_shrink_multi() {
        // 多段交替增减，验证 Fenwick 树在多次 shrink 后仍正确。
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![10, 20, 30, 40], vec![10, 20, 30, 40]);
        // 缩短段落 2：30 -> 5
        assert!(idx.update_paragraph_length(2, 5, 5));
        assert_eq!(idx.paragraph_byte_start(3), 35); // 10 + 20 + 5
        assert_eq!(idx.total_byte_length(), 75); // 10 + 20 + 5 + 40
        // 缩短段落 0：10 -> 1
        assert!(idx.update_paragraph_length(0, 1, 1));
        assert_eq!(idx.paragraph_byte_start(1), 1);
        assert_eq!(idx.paragraph_byte_start(3), 26); // 1 + 20 + 5
        assert_eq!(idx.total_byte_length(), 66); // 1 + 20 + 5 + 40
        // 缩短段落 3：40 -> 3
        assert!(idx.update_paragraph_length(3, 3, 3));
        assert_eq!(idx.paragraph_byte_start(3), 26);
        assert_eq!(idx.total_byte_length(), 29); // 1 + 20 + 5 + 3
        // 前缀和全量校验
        assert_eq!(idx.paragraph_byte_start(0), 0);
        assert_eq!(idx.paragraph_byte_start(1), 1);
        assert_eq!(idx.paragraph_byte_start(2), 21); // 1 + 20
        assert_eq!(idx.paragraph_byte_start(4), 29); // 越界返回 total
    }

    #[test]
    fn offset_index_update_out_of_bounds() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5], vec![3, 5]);
        assert!(!idx.update_paragraph_length(5, 10, 10));
    }

    #[test]
    fn offset_index_insert() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 7], vec![3, 7]);
        idx.insert(1, 5, 5);
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.byte_length_at(1), 5);
        assert_eq!(idx.paragraph_byte_start(2), 8); // 3 + 5
        assert_eq!(idx.total_byte_length(), 15);
    }

    #[test]
    fn offset_index_remove() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![3, 5, 7]);
        assert!(idx.remove(1));
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.byte_length_at(1), 7);
        assert_eq!(idx.paragraph_byte_start(1), 3);
        assert_eq!(idx.total_byte_length(), 10);
    }

    #[test]
    fn offset_index_many_updates() {
        // 测试多次更新后 Fenwick 树正确性
        let mut idx = ParagraphOffsetIndex::new();
        let n = 100;
        idx.rebuild((1..=n).collect(), (1..=n).collect());
        // 多次更新
        for i in 0..n {
            assert!(idx.update_paragraph_length(i, i + 1, i + 1));
        }
        // 验证前缀和
        let mut expected = 0usize;
        for i in 0..n {
            assert_eq!(idx.paragraph_byte_start(i), expected);
            expected += i + 1;
        }
        assert_eq!(idx.total_byte_length(), expected);
    }

    #[test]
    fn offset_index_clear() {
        let mut idx = ParagraphOffsetIndex::new();
        idx.rebuild(vec![3, 5, 7], vec![3, 5, 7]);
        idx.clear();
        assert!(idx.is_empty());
        assert_eq!(idx.total_byte_length(), 0);
    }
}
