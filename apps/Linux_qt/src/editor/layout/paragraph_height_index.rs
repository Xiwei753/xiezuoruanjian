//! 段落高度索引 — Fenwick 树（Binary Indexed Tree）保存段落前缀高度。
//!
//! Issue #901: 增量排版子系统。
//!
//! # 核心设计
//!
//! - 用 Fenwick 树维护段落高度的前缀和，支持 O(log n) 查询和单点更新。
//! - **整章纵向平移不需要遍历全文改每段 y** — y 由前缀和计算，不逐段存储。
//! - **段落高度变化只改受影响段的数据** — Fenwick 树单点更新 O(log n)。
//! - 段落插入/删除时重建 BIT（O(n)），这在实际场景中可接受：
//!   段落级插入/删除不频繁（只在 Enter/Backspace 换行符时发生），
//!   而高度更新和 y→段落查询频繁（每次排版/滚动/命中测试）。
//!
//! # 操作复杂度
//!
//! | 操作 | 复杂度 |
//! |------|--------|
//! | `insert` | O(n) 重建 |
//! | `remove` | O(n) 重建 |
//! | `update_height` | O(log n) |
//! | `y_to_paragraph` | O(log n) |
//! | `paragraph_top_y` | O(log n) |
//! | `total_height` | O(1) |
//! | `height_at` | O(1) |

/// 段落高度索引 — Fenwick 树保存段落前缀高度。
///
/// 维护一个高度数组 `heights[0..n]` 和对应的 Fenwick 树 `bit[1..=n]`。
/// `prefix_sum(k)` = `heights[0] + heights[1] + ... + heights[k-1]`（前 k 个段落的高度和）。
///
/// 段落 `i`（0-indexed）的 y 范围：`[paragraph_top_y(i), paragraph_top_y(i) + heights[i])`。
#[derive(Clone, Debug, Default)]
pub struct ParagraphHeightIndex {
    /// Fenwick 树数组（1-indexed，`bit[1..=n]`）。
    bit: Vec<f64>,
    /// 原始高度数组（0-indexed），用于直接访问和重建。
    heights: Vec<f64>,
    /// 段落数量。
    len: usize,
    /// 缓存的总高度（`bit` 的根和），O(1) 查询。
    total: f64,
}

impl ParagraphHeightIndex {
    /// 创建空的高度索引。
    pub fn new() -> Self {
        Self {
            bit: Vec::new(),
            heights: Vec::new(),
            len: 0,
            total: 0.0,
        }
    }

    /// 从高度数组创建高度索引。
    pub fn from_heights(heights: Vec<f64>) -> Self {
        let len = heights.len();
        let total: f64 = heights.iter().sum();
        let mut bit = vec![0.0; len + 1];
        // 构建 Fenwick 树
        for i in 0..len {
            let val = heights[i];
            let mut j = i + 1;
            while j <= len {
                bit[j] += val;
                j += lowbit(j);
            }
        }
        Self {
            bit,
            heights,
            len,
            total,
        }
    }

    /// 段落数量。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.bit.clear();
        self.heights.clear();
        self.len = 0;
        self.total = 0.0;
    }

    /// 总高度（所有段落高度之和）。
    pub fn total_height(&self) -> f64 {
        self.total
    }

    /// 获取段落 `index` 的高度。越界返回 0.0。
    pub fn height_at(&self, index: usize) -> f64 {
        self.heights.get(index).copied().unwrap_or(0.0)
    }

    // ── Fenwick 树核心操作 ──

    /// 前缀和：`heights[0] + heights[1] + ... + heights[k-1]`。
    ///
    /// `k` 为段落数量（0-indexed 的半开区间右端）。
    /// `k == 0` 返回 0.0，`k >= len` 返回总高度。
    fn prefix_sum(&self, k: usize) -> f64 {
        let k = k.min(self.len);
        let mut sum = 0.0;
        let mut j = k;
        while j > 0 {
            sum += self.bit[j];
            j -= lowbit(j);
        }
        sum
    }

    /// Fenwick 树单点加：`heights[index] += delta`，同步更新 BIT。
    fn bit_add(&mut self, index: usize, delta: f64) {
        let mut j = index + 1; // 转 1-indexed
        while j <= self.len {
            self.bit[j] += delta;
            j += lowbit(j);
        }
    }

    /// 重建整个 Fenwick 树（从 `heights` 数组）。
    fn rebuild_bit(&mut self) {
        let n = self.heights.len();
        self.bit = vec![0.0; n + 1];
        for i in 0..n {
            let val = self.heights[i];
            let mut j = i + 1;
            while j <= n {
                self.bit[j] += val;
                j += lowbit(j);
            }
        }
        self.len = n;
        self.total = self.heights.iter().sum();
    }

    // ── 查询操作 ──

    /// 查段顶 — 段落 `index` 的顶部 y 坐标（文档坐标系）。
    ///
    /// 即 `heights[0] + heights[1] + ... + heights[index-1]`。
    /// `index == 0` 返回 0.0（文档顶部）。越界返回总高度。
    pub fn paragraph_top_y(&self, index: usize) -> f64 {
        if index >= self.len {
            return self.total;
        }
        self.prefix_sum(index)
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
    /// 使用 Fenwick 树上的二分搜索，O(log n)。
    pub fn y_to_paragraph(&self, y: f64) -> usize {
        if self.len == 0 {
            return 0;
        }
        if y < 0.0 {
            return 0;
        }
        if y >= self.total {
            return self.len - 1;
        }

        // BIT 上的二分搜索：找最大的 pos 使得 prefix_sum(pos) <= y
        // pos 是 1-indexed 计数（0 <= pos <= len）
        // 段落索引 = pos（0-indexed）
        let mut pos = 0usize;
        let mut remaining = y;

        // 找最高位 2^k <= len
        let mut bit_mask = self.len.next_power_of_two() >> 1;
        while bit_mask > 0 {
            let next = pos + bit_mask;
            if next <= self.len && self.bit[next] <= remaining {
                remaining -= self.bit[next];
                pos = next;
            }
            bit_mask >>= 1;
        }

        // pos 是最大的满足 prefix_sum(pos) <= y 的值
        // 段落 pos（0-indexed）包含 y
        pos.min(self.len.saturating_sub(1))
    }

    // ── 更新操作 ──

    /// 改段高 — 更新段落 `index` 的高度为 `new_height`。
    ///
    /// O(log n)。只改受影响段的数据，不影响其他段落的相对位置。
    /// 返回旧高度，越界返回 `None`。
    pub fn update_height(&mut self, index: usize, new_height: f64) -> Option<f64> {
        if index >= self.len {
            return None;
        }
        let old_height = self.heights[index];
        let delta = new_height - old_height;
        self.heights[index] = new_height;
        self.bit_add(index, delta);
        self.total += delta;
        Some(old_height)
    }

    /// 段落插入 — 在 `index` 处插入高度为 `height` 的新段落。
    ///
    /// O(n) 重建 BIT。后续段落的 y 坐标自动右移（由前缀和计算），
    /// 不需要遍历改每段 y。
    pub fn insert(&mut self, index: usize, height: f64) {
        let insert_pos = index.min(self.len);
        self.heights.insert(insert_pos, height);
        self.rebuild_bit();
    }

    /// 段落删除 — 删除段落 `index`。
    ///
    /// O(n) 重建 BIT。后续段落的 y 坐标自动左移。
    /// 返回被删除段落的高度，越界返回 `None`。
    pub fn remove(&mut self, index: usize) -> Option<f64> {
        if index >= self.len {
            return None;
        }
        let height = self.heights.remove(index);
        self.rebuild_bit();
        Some(height)
    }

    /// 批量更新高度 — 从高度数组重建整个索引。
    ///
    /// 用于全篇重排后一次性更新所有段落高度。
    pub fn rebuild(&mut self, heights: Vec<f64>) {
        self.heights = heights;
        self.rebuild_bit();
    }

    /// 批量更新单个段落高度（不重建 BIT，逐个 `update_height`）。
    ///
    /// 当多个段落高度变化时，逐个调用 `update_height` 比 `rebuild` 更高效
    /// （O(k log n) vs O(n)，k 为变化段落数）。
    pub fn update_heights(&mut self, updates: &[(usize, f64)]) {
        for &(index, new_height) in updates {
            self.update_height(index, new_height);
        }
    }

    /// Issue #901: 确保索引长度匹配段落数，只在段落数变化时才重建。
    ///
    /// 用于增量排版路径：当段落数量没变时，不需要 `rebuild`（O(n)），
    /// 只需对受影响段落调用 `update_height`（O(k log n)）。
    /// 当段落数量变化（换行拆段/删回车合段）时，才需要重建。
    ///
    /// # 参数
    ///
    /// - `expected_len`：期望的段落数量。
    /// - `default_height`：新增段落的默认高度。
    ///
    /// # 返回
    ///
    /// 返回 `true` 表示发生了重建（段落数变化），`false` 表示长度已匹配无需重建。
    pub fn ensure_len(&mut self, expected_len: usize, default_height: f64) -> bool {
        if self.len == expected_len {
            return false;
        }
        // 段落数变化，重建
        self.heights.resize(expected_len, default_height);
        self.rebuild_bit();
        true
    }

    /// Issue #901: 增量更新受影响段落的高度。
    ///
    /// 对 `updates` 中列出的段落逐个调用 `update_height`（O(k log n)），
    /// 不扫描全章。只在段落数量不匹配时才调用 `ensure_len` 重建。
    ///
    /// # 参数
    ///
    /// - `updates`：`(段落索引, 新高度)` 列表。
    /// - `expected_len`：期望的段落数量。不匹配时先重建。
    /// - `default_height`：新增段落的默认高度。
    pub fn update_heights_incremental(
        &mut self,
        updates: &[(usize, f64)],
        expected_len: usize,
        default_height: f64,
    ) {
        // 先确保长度匹配
        self.ensure_len(expected_len, default_height);
        // 逐个更新受影响段落
        for &(index, new_height) in updates {
            self.update_height(index, new_height);
        }
    }

    /// Issue #901 第二轮复核: 段落数变化时增量更新 — 只在结构变化位置插入/删除高度，
    /// 保留未变化段的高度数据。
    ///
    /// 与 `ensure_len` + `rebuild_bit` 的区别：`ensure_len` 用 `resize` 在末尾
    /// 添加/截断，但 Enter 拆段/Backspace 合段的变化在中间位置，resize 会导致
    /// 高度错位。此方法用 `insert`/`remove` 在正确位置操作，保留未变化段高度。
    ///
    /// # 参数
    ///
    /// - `insert_at`：插入位置和高度（`Some` 表示插入新段落）。
    /// - `remove_at`：删除位置（`Some` 表示删除段落）。
    /// - `height_updates`：受影响段落的 `(索引, 新高度)` 列表。
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
            self.update_height(index, new_height);
        }
    }

    /// 获取所有段落高度（按文档顺序）。
    pub fn heights(&self) -> &[f64] {
        &self.heights
    }

    /// 获取段落 `index` 的 y 范围 `(top_y, bottom_y)`。
    ///
    /// 越界返回 `None`。
    pub fn paragraph_y_range(&self, index: usize) -> Option<(f64, f64)> {
        if index >= self.len {
            return None;
        }
        let top = self.paragraph_top_y(index);
        let bottom = top + self.heights[index];
        Some((top, bottom))
    }

    /// Issue #901 第五轮阻塞5: 枚举 y 范围内的段落索引范围。
    ///
    /// 返回 `[start, end)`，使得段落 `start` 是第一个与 `[y_start, y_end]`
    /// 相交的段落，段落 `end - 1` 是最后一个相交的段落。
    ///
    /// 用于视口枚举：`y_start = scroll_y`，`y_end = scroll_y + viewport_h`。
    /// O(log n) 用 Fenwick 树二分搜索，不遍历全部段落。
    ///
    /// # 边界
    ///
    /// - 空索引 → 返回 `(0, 0)`
    /// - `y_start < 0` → 从段落 0 开始
    /// - `y_end >= total_height` → 到最后一个段落
    pub fn paragraphs_in_y_range(&self, y_start: f64, y_end: f64) -> (usize, usize) {
        if self.len == 0 || y_end < 0.0 {
            return (0, 0);
        }
        // Issue #901 第五轮阻塞2: 防御性处理 y_start > y_end 的调用，
        // 自动交换以保证 start <= end，避免返回空范围或越界。
        let (y_start, y_end) = if y_start > y_end {
            (y_end, y_start)
        } else {
            (y_start, y_end)
        };
        let start = self.y_to_paragraph(y_start);
        // y_end 对应的段落 +1（因为 y_to_paragraph 返回包含 y 的段落，
        // 我们要的是最后一个相交段落的下一个）
        let end = (self.y_to_paragraph(y_end) + 1).min(self.len);
        if start >= end {
            return (start, start);
        }
        (start, end)
    }
}

/// 计算 `x` 的最低位（lowest set bit），即 `x & (-x)`。
///
/// 对于 `x > 0`，返回 `x` 的二进制表示中最低位的 1 所代表的值。
fn lowbit(x: usize) -> usize {
    x & x.wrapping_neg()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn lowbit_function() {
        assert_eq!(lowbit(1), 1);
        assert_eq!(lowbit(2), 2);
        assert_eq!(lowbit(3), 1);
        assert_eq!(lowbit(4), 4);
        assert_eq!(lowbit(6), 2);
        assert_eq!(lowbit(8), 8);
        assert_eq!(lowbit(12), 4);
        assert_eq!(lowbit(16), 16);
    }

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
}
