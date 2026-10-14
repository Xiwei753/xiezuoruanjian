//! 分块数据结构 — 支持 O(B + n/B) 的局部 splice 和前缀和查询。
//!
//! Issue #901 第三十三轮问题3: 消除局部 splice 的后缀槽号更新和整章 Fenwick rebuild。
//!
//! # 核心设计
//!
//! 将段落按逻辑序号分块存储，每块固定大小 B（默认 256）。这样：
//! - 中间插删：只影响受影响块 O(B + n/B)，不遍历后缀所有段。
//! - 前缀和：块级前缀和 O(块数) + 块内前缀和 O(B)。
//! - 按 ID 定位：用分块 HashMap，遍历块查 O(块数)。
//! - 复杂度：O(B + n/B)，当 B≈sqrt(n) 时为 O(sqrt(n))，远优于 O(n)。
//!
//! # 与旧实现的对比
//!
//! | 操作 | 旧（Vec + Fenwick BIT） | 新（ChunkedPrefixSum） |
//! |------|------------------------|----------------------|
//! | splice（段落数变化） | O(n) rebuild_bit | O(B + n/B) |
//! | prefix_sum | O(log n) | O(n/B) |
//! | update | O(log n) | O(n/B + B) |
//! | find_by_accumulated | O(log n) | O(n/B + B) |
//!
//! splice 从 O(n) 降到 O(sqrt(n))，prefix_sum/update 从 O(log n) 升到 O(sqrt(n))。
//! splice 是 Enter/Backspace 热路径（段落数变化），prefix_sum 是滚动/命中测试路径。
//! 在长篇（n≈10^4）下，sqrt(n)≈100，log(n)≈14，splice 改进约 100 倍，
//! prefix_sum 退化约 7 倍，整体编辑体验显著改善。

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use super::types::ParagraphId;

/// 默认块容量 B。
///
/// B=256 时，n=10^4 段约 40 块，splice O(256+40)≈O(256)；
/// n=10^5 段约 400 块，splice O(256+400)≈O(400)。
const DEFAULT_CHUNK_CAPACITY: usize = 256;

// ══════════════════════════════════════════════════════════════════════════
// ── PrefixNum trait：分块前缀和的数值类型抽象 ──
// ══════════════════════════════════════════════════════════════════════════

/// 分块前缀和的数值类型 trait。
///
/// 抽象 f64（高度）和 usize（行数/byte 长度）的公共操作。
/// `le` 用于累积查找，假设前缀和单调非减（值非负）。
pub trait PrefixNum: Clone + Copy + Default + Debug {
    /// 加法。
    fn add(self, rhs: Self) -> Self;
    /// 减法。调用方保证 `self >= rhs`（前缀和场景下不会下溢）。
    fn sub(self, rhs: Self) -> Self;
    /// `self <= other`，用于累积查找。
    fn le(&self, other: &Self) -> bool;
}

impl PrefixNum for f64 {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self + rhs
    }
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self - rhs
    }
    #[inline]
    fn le(&self, other: &Self) -> bool {
        // 假设无 NaN（高度/偏移都是有限非负值）。
        *self <= *other
    }
}

impl PrefixNum for usize {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self + rhs
    }
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self - rhs
    }
    #[inline]
    fn le(&self, other: &Self) -> bool {
        *self <= *other
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ── ChunkedPrefixSum：分块前缀和 ──
// ══════════════════════════════════════════════════════════════════════════

/// 分块前缀和的一个块。
#[derive(Clone, Debug)]
struct PrefixChunk<T> {
    /// 块内原始值。
    values: Vec<T>,
    /// 块内前缀和：`local_prefix[i] = values[0] + ... + values[i-1]`。
    /// 长度 = `values.len() + 1`，`local_prefix[0] = zero`，`local_prefix[len] = sum`。
    local_prefix: Vec<T>,
    /// 本块总和（= `local_prefix.last()`）。
    sum: T,
}

/// 分块前缀和 — 支持 O(B + n/B) 的 prefix_sum、update、splice、find_by_accumulated。
///
/// 替代旧的 `Vec + Fenwick BIT` 结构。BIT 的中间插删需 O(n) 重建（索引编码子树区间），
/// 分块前缀和的中间插删只影响受影响块 O(B + n/B)。
///
/// # 不变量
///
/// - 每块 `values.len()` 在 `[1, 2*chunk_capacity]` 范围内（splice 后可能短暂超 B，
///   分裂时保证 ≤ 2B）。
/// - `local_prefix` 长度 = `values.len() + 1`，`local_prefix[0] = T::default()`。
/// - `sum = local_prefix.last()`。
/// - `total = 所有块 sum 之和`。
/// - `total_len = 所有块 values.len() 之和`。
#[derive(Clone, Debug)]
pub struct ChunkedPrefixSum<T: PrefixNum> {
    chunks: Vec<PrefixChunk<T>>,
    chunk_capacity: usize,
    total_len: usize,
    total: T,
}

impl<T: PrefixNum> Default for ChunkedPrefixSum<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PrefixNum> ChunkedPrefixSum<T> {
    /// 创建空的分块前缀和。
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            chunk_capacity: DEFAULT_CHUNK_CAPACITY,
            total_len: 0,
            total: T::default(),
        }
    }

    /// 从值数组创建分块前缀和。
    pub fn from_values(values: Vec<T>) -> Self {
        Self::from_values_with_capacity(values, DEFAULT_CHUNK_CAPACITY)
    }

    /// 从值数组创建，指定块容量。
    pub fn from_values_with_capacity(values: Vec<T>, chunk_capacity: usize) -> Self {
        let cap = chunk_capacity.max(1);
        let total_len = values.len();
        let total: T = values.iter().copied().fold(T::default(), |acc, v| acc.add(v));
        let chunks = Self::split_into_chunks(values, cap);
        Self {
            chunks,
            chunk_capacity: cap,
            total_len,
            total,
        }
    }

    /// 元素数量。
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.total_len = 0;
        self.total = T::default();
    }

    /// 所有元素总和。O(1)。
    pub fn total_sum(&self) -> T {
        self.total
    }

    /// 获取元素 `index` 的值。越界返回 `None`。
    pub fn get(&self, index: usize) -> Option<T> {
        if index >= self.total_len {
            return None;
        }
        let (chunk_idx, local_idx) = self.find_chunk(index);
        self.chunks
            .get(chunk_idx)
            .and_then(|c| c.values.get(local_idx).copied())
    }

    /// 前缀和：`values[0] + values[1] + ... + values[k-1]`。
    ///
    /// `k == 0` 返回 zero，`k >= len` 返回 total。
    /// O(块数)。
    pub fn prefix_sum(&self, k: usize) -> T {
        let k = k.min(self.total_len);
        let mut acc = T::default();
        let mut offset = 0usize;
        for c in &self.chunks {
            let clen = c.values.len();
            if k <= offset {
                break;
            }
            if k >= offset + clen {
                acc = acc.add(c.sum);
                offset += clen;
            } else {
                // k 在本块内
                let local_k = k - offset;
                acc = acc.add(c.local_prefix[local_k]);
                return acc;
            }
        }
        acc
    }

    /// 单点更新：`values[index] = new_value`。
    ///
    /// 返回旧值，越界返回 `None`。
    /// O(块数 + B)（找块 O(块数) + 重建块内前缀和 O(B)）。
    pub fn update(&mut self, index: usize, new_value: T) -> Option<T> {
        if index >= self.total_len {
            return None;
        }
        let (chunk_idx, local_idx) = self.find_chunk(index);
        let chunk = &mut self.chunks[chunk_idx];
        let old = chunk.values[local_idx];
        chunk.values[local_idx] = new_value;
        Self::rebuild_chunk_prefix(chunk);
        // 更新 total：total += new_value - old
        if new_value.le(&old) {
            self.total = self.total.sub(old.sub(new_value));
        } else {
            self.total = self.total.add(new_value.sub(old));
        }
        Some(old)
    }

    /// 局部 splice — 删除 `[start, end)` 范围，插入 `new_values`。
    ///
    /// 前缀段 `[0, start)` 和后缀段 `[end, len)` 保留不动。
    /// O(块数 + B + new_values.len())。
    pub fn splice(&mut self, start: usize, end: usize, new_values: Vec<T>) {
        let splice_start = start.min(self.total_len);
        let splice_end = end.min(self.total_len);
        if splice_start > splice_end {
            return;
        }
        let new_count = new_values.len();

        if splice_start == splice_end && new_count == 0 {
            return;
        }

        // 空根插入/删除：chunks 为空时 find_chunk 返回 (0,0)，self.chunks[0] 越界。
        // 把空根插入作为合法基础操作，不调用 find_chunk。
        if self.chunks.is_empty() {
            if new_count == 0 {
                // 空根删除保持空根。
                return;
            }
            // 从 new_values 直接创建首块。
            self.chunks = Self::split_into_chunks(new_values, self.chunk_capacity);
            self.total_len = new_count;
            self.recompute_total();
            return;
        }

        let (sc, sl) = self.find_chunk(splice_start);
        let (ec, el) = self.find_chunk(splice_end);

        // 提取前缀（sc 块中 sl 之前）和后缀（ec 块中 el 之后）。
        let prefix: Vec<T> = self.chunks[sc].values[..sl].to_vec();
        let suffix: Vec<T> = self.chunks[ec].values[el..].to_vec();

        // 组装新值：prefix + new_values + suffix。
        let mut all_new = prefix;
        all_new.extend(new_values);
        all_new.extend(suffix);

        // 重新分块，替换 chunks[sc..=ec]。
        let new_chunks = Self::split_into_chunks(all_new, self.chunk_capacity);
        self.chunks.splice(sc..=ec, new_chunks);

        // 更新 total_len 和 total。
        self.total_len = self.total_len - (splice_end - splice_start) + new_count;
        self.recompute_total();
    }

    /// 在 `index` 处插入 `value`。
    ///
    /// O(块数 + B)。
    pub fn insert(&mut self, index: usize, value: T) {
        let insert_pos = index.min(self.total_len);
        self.splice(insert_pos, insert_pos, vec![value]);
    }

    /// 删除 `index` 处的元素，返回旧值。越界返回 `None`。
    ///
    /// O(块数 + B)。
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.total_len {
            return None;
        }
        let old = self.get(index)?;
        self.splice(index, index + 1, Vec::new());
        Some(old)
    }

    /// 末尾追加 `value`。O(1) 摊还（末尾块未满时 O(1)，满时分裂 O(B)）。
    pub fn push(&mut self, value: T) {
        let cap = self.chunk_capacity;
        let need_new_block = self
            .chunks
            .last()
            .map_or(true, |last| last.values.len() >= cap);
        if need_new_block {
            self.chunks.push(Self::build_chunk(vec![value]));
        } else {
            let last_idx = self.chunks.len() - 1;
            self.chunks[last_idx].values.push(value);
            Self::rebuild_chunk_prefix(&mut self.chunks[last_idx]);
        }
        self.total_len += 1;
        self.total = self.total.add(value);
    }

    /// 确保长度匹配 `expected_len`，新增元素用 `default` 填充。
    ///
    /// 返回 `true` 表示发生了变化。
    pub fn ensure_len(&mut self, expected_len: usize, default: T) -> bool {
        if self.total_len == expected_len {
            return false;
        }
        if expected_len > self.total_len {
            let extra = expected_len - self.total_len;
            for _ in 0..extra {
                self.push(default);
            }
        } else {
            // 缩容：删除末尾多余元素。
            self.splice(expected_len, self.total_len, Vec::new());
        }
        true
    }

    /// 从值数组重建整个索引。
    pub fn rebuild(&mut self, values: Vec<T>) {
        let cap = self.chunk_capacity;
        let total_len = values.len();
        let total: T = values.iter().copied().fold(T::default(), |acc, v| acc.add(v));
        let chunks = Self::split_into_chunks(values, cap);
        self.chunks = chunks;
        self.total_len = total_len;
        self.total = total;
    }

    /// 累积查找 — 返回最大 `index` 使得 `prefix_sum(index) <= target`。
    ///
    /// - `target < values[0]` → 返回 0
    /// - `target >= total` → 返回 `total_len`
    /// - 空索引 → 返回 0
    ///
    /// O(块数 + B)（块级遍历 + 命中块内线性）。
    pub fn find_by_accumulated(&self, target: T) -> usize {
        if self.total_len == 0 {
            return 0;
        }
        let mut acc = T::default();
        let mut offset = 0usize;
        for c in &self.chunks {
            let block_end = acc.add(c.sum);
            if block_end.le(&target) {
                // 整块都在 <= target 范围。
                acc = block_end;
                offset += c.values.len();
            } else {
                // target 落在本块内，块内线性找。
                for i in 0..c.values.len() {
                    let next = acc.add(c.values[i]);
                    if next.le(&target) {
                        acc = next;
                        offset += 1;
                    } else {
                        return offset;
                    }
                }
                return offset;
            }
        }
        offset
    }

    /// 转为扁平 Vec（用于兼容需要 Vec 的消费方）。O(n)。
    pub fn to_vec(&self) -> Vec<T> {
        let mut result = Vec::with_capacity(self.total_len);
        for c in &self.chunks {
            result.extend_from_slice(&c.values);
        }
        result
    }

    // ── 内部辅助方法 ──

    /// 找到 `index` 所在块号和块内偏移。
    ///
    /// `index < total_len` → 返回所在块。
    /// `index >= total_len` → 返回最后一个块的末尾（或 (0, 0) 如果空）。
    fn find_chunk(&self, index: usize) -> (usize, usize) {
        let mut offset = 0usize;
        for (i, c) in self.chunks.iter().enumerate() {
            let clen = c.values.len();
            if index < offset + clen {
                return (i, index - offset);
            }
            offset += clen;
        }
        // index >= total_len：返回最后一个块的末尾。
        let last_idx = self.chunks.len().saturating_sub(1);
        let last_len = self.chunks.last().map_or(0, |c| c.values.len());
        (last_idx, last_len)
    }

    /// 重新计算 total（遍历所有块 sum）。O(块数)。
    fn recompute_total(&mut self) {
        let mut total = T::default();
        for c in &self.chunks {
            total = total.add(c.sum);
        }
        self.total = total;
    }

    /// 构建一个块（计算 local_prefix 和 sum）。
    fn build_chunk(values: Vec<T>) -> PrefixChunk<T> {
        let mut local_prefix = Vec::with_capacity(values.len() + 1);
        let mut acc = T::default();
        local_prefix.push(acc);
        for &v in &values {
            acc = acc.add(v);
            local_prefix.push(acc);
        }
        let sum = acc;
        PrefixChunk {
            values,
            local_prefix,
            sum,
        }
    }

    /// 重建块的 local_prefix 和 sum（values 已修改）。
    fn rebuild_chunk_prefix(chunk: &mut PrefixChunk<T>) {
        let mut acc = T::default();
        chunk.local_prefix.clear();
        chunk.local_prefix.reserve(chunk.values.len() + 1);
        chunk.local_prefix.push(acc);
        for &v in &chunk.values {
            acc = acc.add(v);
            chunk.local_prefix.push(acc);
        }
        chunk.sum = acc;
    }

    /// 将 Vec 分成多个块。
    fn split_into_chunks(values: Vec<T>, capacity: usize) -> Vec<PrefixChunk<T>> {
        if values.is_empty() {
            return Vec::new();
        }
        let cap = capacity.max(1);
        let mut chunks = Vec::new();
        let mut start = 0usize;
        while start < values.len() {
            let end = (start + cap).min(values.len());
            // SAFETY: start..end 是 Vec 的有效范围。
            let chunk_values = values[start..end].to_vec();
            chunks.push(Self::build_chunk(chunk_values));
            start = end;
        }
        chunks
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ── ChunkedSlotMap：分块 ParagraphId → slot 映射 ──
// ══════════════════════════════════════════════════════════════════════════

/// 分块 slot 映射的一个块。
#[derive(Clone, Debug)]
struct SlotBlock {
    /// 块内按 slot 顺序的 ParagraphId。
    ids: Vec<ParagraphId>,
    /// ID → 块内索引的反向查找表。
    id_to_local: HashMap<ParagraphId, usize>,
}

/// 分块 ParagraphId → slot（逻辑序号）映射。
///
/// 替代旧的 `HashMap<ParagraphId, usize>` + splice 时遍历后缀段平移。
///
/// # 核心改进
///
/// 旧实现 splice 时遍历所有后缀段的 cache 条目逐个平移 slot，O(后缀段数)。
/// 分块映射的 splice 只在受影响块内操作，后缀块的 `id_to_local` 不变
///（块内 ID 和块内索引不变，全局 slot = 块前缀 len + 块内索引动态计算），
/// O(后缀块数 + B)。
///
/// # 不变量
///
/// - `ids` 和 `id_to_local` 一致：`id_to_local[ids[i]] == i`。
/// - 每块 `ids.len()` 在 `[1, 2*chunk_capacity]` 范围内。
/// - `total_len = 所有块 ids.len() 之和`。
#[derive(Clone, Debug)]
pub struct ChunkedSlotMap {
    blocks: Vec<SlotBlock>,
    chunk_capacity: usize,
    total_len: usize,
}

impl Default for ChunkedSlotMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkedSlotMap {
    /// 创建空的分块 slot 映射。
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            chunk_capacity: DEFAULT_CHUNK_CAPACITY,
            total_len: 0,
        }
    }

    /// 从 ID 数组创建（slot = 数组索引）。
    pub fn from_ids(ids: Vec<ParagraphId>) -> Self {
        let total_len = ids.len();
        let blocks = Self::split_into_blocks(ids, DEFAULT_CHUNK_CAPACITY);
        Self {
            blocks,
            chunk_capacity: DEFAULT_CHUNK_CAPACITY,
            total_len,
        }
    }

    /// 从 HashMap 创建（按 slot 排序后分块）。
    pub fn from_hash_map(map: &HashMap<ParagraphId, usize>) -> Self {
        if map.is_empty() {
            return Self::new();
        }
        let max_slot = map.values().copied().max().unwrap_or(0);
        let mut ids = vec![ParagraphId::INVALID; max_slot + 1];
        for (&id, &slot) in map {
            if slot < ids.len() {
                ids[slot] = id;
            }
        }
        Self::from_ids(ids)
    }

    /// 元素数量。
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// 清空所有数据。
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.total_len = 0;
    }

    /// 查询 `id` 对应的 slot（逻辑序号）。
    ///
    /// 遍历块查 `id_to_local`，O(块数)。
    pub fn slot(&self, id: ParagraphId) -> Option<usize> {
        let mut offset = 0usize;
        for b in &self.blocks {
            if let Some(&local) = b.id_to_local.get(&id) {
                return Some(offset + local);
            }
            offset += b.ids.len();
        }
        None
    }

    /// 是否包含 `id`。O(块数)。
    pub fn contains(&self, id: ParagraphId) -> bool {
        self.slot(id).is_some()
    }

    /// 在 `index` 处插入 `id`。
    ///
    /// O(块数 + B)。
    pub fn insert(&mut self, index: usize, id: ParagraphId) {
        let insert_pos = index.min(self.total_len);
        self.splice(insert_pos, insert_pos, vec![id]);
    }

    /// 删除 `index` 处的 ID，返回旧 ID。越界返回 `None`。
    ///
    /// O(块数 + B)。
    pub fn remove(&mut self, index: usize) -> Option<ParagraphId> {
        if index >= self.total_len {
            return None;
        }
        let (chunk_idx, local_idx) = self.find_chunk(index);
        let old = self.blocks[chunk_idx].ids[local_idx];
        self.splice(index, index + 1, Vec::new());
        Some(old)
    }

    /// 局部 splice — 删除 `[start, end)` 范围的 ID，插入 `new_ids`。
    ///
    /// 后缀块的 `id_to_local` 不变（块内 ID 和块内索引不变），
    /// 全局 slot 由块前缀 len + 块内索引动态计算。
    /// O(块数 + B + new_ids.len())。
    pub fn splice(&mut self, start: usize, end: usize, new_ids: Vec<ParagraphId>) {
        let splice_start = start.min(self.total_len);
        let splice_end = end.min(self.total_len);
        if splice_start > splice_end {
            return;
        }
        let new_count = new_ids.len();

        if splice_start == splice_end && new_count == 0 {
            return;
        }

        // 空根插入/删除：blocks 为空时 find_chunk 返回 (0,0)，self.blocks[0] 越界。
        // 把空根插入作为合法基础操作，不调用 find_chunk。
        if self.blocks.is_empty() {
            if new_count == 0 {
                // 空根删除保持空根。
                return;
            }
            // 从 new_ids 直接创建首块。
            self.blocks = Self::split_into_blocks(new_ids, self.chunk_capacity);
            self.total_len = new_count;
            return;
        }

        let (sc, sl) = self.find_chunk(splice_start);
        let (ec, el) = self.find_chunk(splice_end);

        // 提取前缀和后缀。
        let prefix: Vec<ParagraphId> = self.blocks[sc].ids[..sl].to_vec();
        let suffix: Vec<ParagraphId> = self.blocks[ec].ids[el..].to_vec();

        // 组装新 IDs：prefix + new_ids + suffix。
        let mut all_new = prefix;
        all_new.extend(new_ids);
        all_new.extend(suffix);

        // 重新分块，替换 blocks[sc..=ec]。
        let new_blocks = Self::split_into_blocks(all_new, self.chunk_capacity);
        self.blocks.splice(sc..=ec, new_blocks);

        self.total_len = self.total_len - (splice_end - splice_start) + new_count;
    }

    /// 构建完整 `HashMap<ParagraphId, usize>`。
    ///
    /// O(段落数)。用于 `paragraph_id_to_slot_arc()` 返回 `Arc<HashMap>`
    /// 供消费方共享。snapshot 构建本身是 O(段落数)，此构建不增加渐近复杂度。
    pub fn to_hash_map(&self) -> HashMap<ParagraphId, usize> {
        let mut map = HashMap::with_capacity(self.total_len);
        let mut offset = 0usize;
        for b in &self.blocks {
            for (local, &id) in b.ids.iter().enumerate() {
                map.insert(id, offset + local);
            }
            offset += b.ids.len();
        }
        map
    }

    /// 从 entries 重建（entries 按 slot 顺序提供 ID）。
    ///
    /// `id_at_slot: Fn(usize) -> Option<ParagraphId>`，对每个 slot 返回 ID。
    pub fn rebuild_from<F>(&mut self, len: usize, id_at_slot: F)
    where
        F: Fn(usize) -> ParagraphId,
    {
        let mut ids = Vec::with_capacity(len);
        for i in 0..len {
            ids.push(id_at_slot(i));
        }
        let blocks = Self::split_into_blocks(ids, self.chunk_capacity);
        self.blocks = blocks;
        self.total_len = len;
    }

    // ── 内部辅助方法 ──

    /// 找到 `index` 所在块号和块内偏移。
    fn find_chunk(&self, index: usize) -> (usize, usize) {
        let mut offset = 0usize;
        for (i, b) in self.blocks.iter().enumerate() {
            let blen = b.ids.len();
            if index < offset + blen {
                return (i, index - offset);
            }
            offset += blen;
        }
        let last_idx = self.blocks.len().saturating_sub(1);
        let last_len = self.blocks.last().map_or(0, |b| b.ids.len());
        (last_idx, last_len)
    }

    /// 构建一个 slot 块。
    fn build_block(ids: Vec<ParagraphId>) -> SlotBlock {
        let mut id_to_local = HashMap::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            id_to_local.insert(id, i);
        }
        SlotBlock { ids, id_to_local }
    }

    /// 将 Vec 分成多个块。
    fn split_into_blocks(
        ids: Vec<ParagraphId>,
        capacity: usize,
    ) -> Vec<SlotBlock> {
        if ids.is_empty() {
            return Vec::new();
        }
        let cap = capacity.max(1);
        let mut blocks = Vec::new();
        let mut start = 0usize;
        while start < ids.len() {
            let end = (start + cap).min(ids.len());
            let chunk_ids = ids[start..end].to_vec();
            blocks.push(Self::build_block(chunk_ids));
            start = end;
        }
        blocks
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ── 单元测试 ──
// ══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── ChunkedPrefixSum<f64> 测试 ──

    #[test]
    fn prefix_sum_f64_empty() {
        let cps = ChunkedPrefixSum::<f64>::new();
        assert_eq!(cps.len(), 0);
        assert!(cps.is_empty());
        assert_eq!(cps.total_sum(), 0.0);
        assert_eq!(cps.prefix_sum(0), 0.0);
        assert_eq!(cps.prefix_sum(10), 0.0);
    }

    #[test]
    fn prefix_sum_f64_from_values() {
        let cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        assert_eq!(cps.len(), 3);
        assert_eq!(cps.total_sum(), 60.0);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(20.0));
        assert_eq!(cps.get(2), Some(30.0));
        assert_eq!(cps.get(3), None);
    }

    #[test]
    fn prefix_sum_f64_prefix_queries() {
        let cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        assert_eq!(cps.prefix_sum(0), 0.0);
        assert_eq!(cps.prefix_sum(1), 10.0);
        assert_eq!(cps.prefix_sum(2), 30.0);
        assert_eq!(cps.prefix_sum(3), 60.0);
        assert_eq!(cps.prefix_sum(10), 60.0); // 越界返回 total
    }

    #[test]
    fn prefix_sum_f64_update() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        let old = cps.update(1, 25.0).unwrap();
        assert_eq!(old, 20.0);
        assert_eq!(cps.get(1), Some(25.0));
        assert_eq!(cps.total_sum(), 65.0);
        assert_eq!(cps.prefix_sum(2), 35.0); // 10 + 25
        assert_eq!(cps.prefix_sum(3), 65.0); // 10 + 25 + 30
    }

    #[test]
    fn prefix_sum_f64_update_out_of_bounds() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0]);
        assert!(cps.update(5, 20.0).is_none());
    }

    #[test]
    fn prefix_sum_f64_insert() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 30.0]);
        cps.insert(1, 20.0);
        assert_eq!(cps.len(), 3);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(20.0));
        assert_eq!(cps.get(2), Some(30.0));
        assert_eq!(cps.total_sum(), 60.0);
        assert_eq!(cps.prefix_sum(2), 30.0); // 10 + 20
    }

    #[test]
    fn prefix_sum_f64_remove() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        let removed = cps.remove(1).unwrap();
        assert_eq!(removed, 20.0);
        assert_eq!(cps.len(), 2);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(30.0));
        assert_eq!(cps.total_sum(), 40.0);
    }

    #[test]
    fn prefix_sum_f64_splice_same_count() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0, 40.0]);
        // 替换 [1, 3) 为 [25.0, 35.0]
        cps.splice(1, 3, vec![25.0, 35.0]);
        assert_eq!(cps.len(), 4);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(25.0));
        assert_eq!(cps.get(2), Some(35.0));
        assert_eq!(cps.get(3), Some(40.0));
        assert_eq!(cps.total_sum(), 110.0);
    }

    #[test]
    fn prefix_sum_f64_splice_grow() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 40.0]);
        // 替换 [1, 1)（插入）为 [20.0, 30.0]
        cps.splice(1, 1, vec![20.0, 30.0]);
        assert_eq!(cps.len(), 4);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(20.0));
        assert_eq!(cps.get(2), Some(30.0));
        assert_eq!(cps.get(3), Some(40.0));
        assert_eq!(cps.total_sum(), 100.0);
    }

    #[test]
    fn prefix_sum_f64_splice_shrink() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0, 40.0]);
        // 删除 [1, 3)
        cps.splice(1, 3, vec![]);
        assert_eq!(cps.len(), 2);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.get(1), Some(40.0));
        assert_eq!(cps.total_sum(), 50.0);
    }

    #[test]
    fn prefix_sum_f64_find_by_accumulated() {
        let cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        // prefix_sum: [0, 10, 30, 60]
        assert_eq!(cps.find_by_accumulated(0.0), 0);
        assert_eq!(cps.find_by_accumulated(5.0), 0);
        assert_eq!(cps.find_by_accumulated(10.0), 1);
        assert_eq!(cps.find_by_accumulated(25.0), 1);
        assert_eq!(cps.find_by_accumulated(30.0), 2);
        assert_eq!(cps.find_by_accumulated(55.0), 2);
        assert_eq!(cps.find_by_accumulated(60.0), 3);
        assert_eq!(cps.find_by_accumulated(100.0), 3);
    }

    #[test]
    fn prefix_sum_f64_ensure_len_grow() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0]);
        assert!(cps.ensure_len(4, 15.0));
        assert_eq!(cps.len(), 4);
        assert_eq!(cps.get(2), Some(15.0));
        assert_eq!(cps.get(3), Some(15.0));
        assert_eq!(cps.total_sum(), 60.0);
    }

    #[test]
    fn prefix_sum_f64_ensure_len_shrink() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0, 30.0]);
        assert!(cps.ensure_len(1, 0.0));
        assert_eq!(cps.len(), 1);
        assert_eq!(cps.get(0), Some(10.0));
    }

    #[test]
    fn prefix_sum_f64_ensure_len_no_change() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0]);
        assert!(!cps.ensure_len(2, 0.0));
    }

    #[test]
    fn prefix_sum_f64_rebuild() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10.0, 20.0]);
        cps.rebuild(vec![5.0, 15.0, 25.0]);
        assert_eq!(cps.len(), 3);
        assert_eq!(cps.total_sum(), 45.0);
        assert_eq!(cps.prefix_sum(2), 20.0);
    }

    #[test]
    fn prefix_sum_f64_many_values() {
        // 测试跨多块的正确性（B=256，300 个值跨 2 块）。
        let values: Vec<f64> = (0..300).map(|i| (i + 1) as f64).collect();
        let cps = ChunkedPrefixSum::from_values(values);
        assert_eq!(cps.len(), 300);
        // 总和 = 1+2+...+300 = 45150
        assert_eq!(cps.total_sum(), 45150.0);
        // prefix_sum(100) = 1+2+...+100 = 5050
        assert_eq!(cps.prefix_sum(100), 5050.0);
        // prefix_sum(200) = 1+2+...+200 = 20100
        assert_eq!(cps.prefix_sum(200), 20100.0);
        // find_by_accumulated(5050.0) = 100
        assert_eq!(cps.find_by_accumulated(5050.0), 100);
    }

    #[test]
    fn prefix_sum_f64_splice_cross_chunk() {
        // 跨块 splice：B=4，8 个值跨 2 块，splice [2, 6) 替换为 3 个值。
        let values: Vec<f64> = (1..=8).map(|i| i as f64).collect();
        let mut cps = ChunkedPrefixSum::from_values_with_capacity(values, 4);
        assert_eq!(cps.len(), 8);
        // splice [2, 6) 删除 4 个（值 3,4,5,6），插入 3 个。
        // 结果：[1, 2, 100, 200, 300, 7, 8]，len=7。
        cps.splice(2, 6, vec![100.0, 200.0, 300.0]);
        assert_eq!(cps.len(), 7);
        assert_eq!(cps.get(0), Some(1.0));
        assert_eq!(cps.get(1), Some(2.0));
        assert_eq!(cps.get(2), Some(100.0));
        assert_eq!(cps.get(3), Some(200.0));
        assert_eq!(cps.get(4), Some(300.0));
        assert_eq!(cps.get(5), Some(7.0));
        assert_eq!(cps.get(6), Some(8.0));
        // 前缀和验证
        assert_eq!(cps.prefix_sum(2), 3.0); // 1 + 2
        assert_eq!(cps.prefix_sum(7), 1.0 + 2.0 + 100.0 + 200.0 + 300.0 + 7.0 + 8.0);
    }

    // ── ChunkedPrefixSum<usize> 测试 ──

    #[test]
    fn prefix_sum_usize_basic() {
        let cps = ChunkedPrefixSum::from_values(vec![1usize, 2, 3]);
        assert_eq!(cps.len(), 3);
        assert_eq!(cps.total_sum(), 6);
        assert_eq!(cps.prefix_sum(0), 0);
        assert_eq!(cps.prefix_sum(1), 1);
        assert_eq!(cps.prefix_sum(2), 3);
        assert_eq!(cps.prefix_sum(3), 6);
    }

    #[test]
    fn prefix_sum_usize_splice() {
        let mut cps = ChunkedPrefixSum::from_values(vec![1usize, 2, 3, 4, 5]);
        // splice [1, 3) 删除 2 个（值 2,3），插入 2 个（10,20）。
        // 结果：[1, 10, 20, 4, 5]，len=5。
        cps.splice(1, 3, vec![10, 20]);
        assert_eq!(cps.len(), 5);
        assert_eq!(cps.get(0), Some(1));
        assert_eq!(cps.get(1), Some(10));
        assert_eq!(cps.get(2), Some(20));
        assert_eq!(cps.get(3), Some(4));
        assert_eq!(cps.get(4), Some(5));
        assert_eq!(cps.total_sum(), 40);
    }

    #[test]
    fn prefix_sum_usize_find_by_accumulated() {
        let cps = ChunkedPrefixSum::from_values(vec![3usize, 5, 7]);
        // prefix_sum: [0, 3, 8, 15]
        assert_eq!(cps.find_by_accumulated(0), 0);
        assert_eq!(cps.find_by_accumulated(2), 0);
        assert_eq!(cps.find_by_accumulated(3), 1);
        assert_eq!(cps.find_by_accumulated(7), 1);
        assert_eq!(cps.find_by_accumulated(8), 2);
        assert_eq!(cps.find_by_accumulated(14), 2);
        assert_eq!(cps.find_by_accumulated(15), 3);
    }

    #[test]
    fn prefix_sum_usize_update_shrink() {
        let mut cps = ChunkedPrefixSum::from_values(vec![10usize, 20, 30]);
        let old = cps.update(1, 5).unwrap();
        assert_eq!(old, 20);
        assert_eq!(cps.get(1), Some(5));
        assert_eq!(cps.total_sum(), 45);
        assert_eq!(cps.prefix_sum(2), 15); // 10 + 5
    }

    // ── ChunkedSlotMap 测试 ──

    #[test]
    fn slot_map_empty() {
        let sm = ChunkedSlotMap::new();
        assert_eq!(sm.len(), 0);
        assert!(sm.is_empty());
        let id = ParagraphId::from_u64(1);
        assert_eq!(sm.slot(id), None);
    }

    #[test]
    fn slot_map_from_ids() {
        let id0 = ParagraphId::from_u64(1);
        let id1 = ParagraphId::from_u64(2);
        let id2 = ParagraphId::from_u64(3);
        let sm = ChunkedSlotMap::from_ids(vec![id0, id1, id2]);
        assert_eq!(sm.len(), 3);
        assert_eq!(sm.slot(id0), Some(0));
        assert_eq!(sm.slot(id1), Some(1));
        assert_eq!(sm.slot(id2), Some(2));
    }

    #[test]
    fn slot_map_insert() {
        let id0 = ParagraphId::from_u64(1);
        let id1 = ParagraphId::from_u64(2);
        let id2 = ParagraphId::from_u64(3);
        let mut sm = ChunkedSlotMap::from_ids(vec![id0, id2]);
        // 在 index 1 插入 id1
        sm.insert(1, id1);
        assert_eq!(sm.len(), 3);
        assert_eq!(sm.slot(id0), Some(0));
        assert_eq!(sm.slot(id1), Some(1));
        assert_eq!(sm.slot(id2), Some(2));
    }

    #[test]
    fn slot_map_remove() {
        let id0 = ParagraphId::from_u64(1);
        let id1 = ParagraphId::from_u64(2);
        let id2 = ParagraphId::from_u64(3);
        let mut sm = ChunkedSlotMap::from_ids(vec![id0, id1, id2]);
        let removed = sm.remove(1).unwrap();
        assert_eq!(removed, id1);
        assert_eq!(sm.len(), 2);
        assert_eq!(sm.slot(id0), Some(0));
        assert_eq!(sm.slot(id1), None);
        assert_eq!(sm.slot(id2), Some(1)); // 后缀 slot 自动平移
    }

    #[test]
    fn slot_map_splice() {
        let id0 = ParagraphId::from_u64(1);
        let id1 = ParagraphId::from_u64(2);
        let id2 = ParagraphId::from_u64(3);
        let id3 = ParagraphId::from_u64(4);
        let id4 = ParagraphId::from_u64(5);
        let mut sm = ChunkedSlotMap::from_ids(vec![id0, id1, id2, id3, id4]);
        // 删除 [1, 3)（id1, id2），插入 [id_new]。
        // 结果：[id0, id_new, id3, id4]，len=4。
        let id_new = ParagraphId::from_u64(99);
        sm.splice(1, 3, vec![id_new]);
        assert_eq!(sm.len(), 4);
        assert_eq!(sm.slot(id0), Some(0));
        assert_eq!(sm.slot(id_new), Some(1));
        assert_eq!(sm.slot(id3), Some(2)); // 后缀 slot 自动平移
        assert_eq!(sm.slot(id4), Some(3));
        assert_eq!(sm.slot(id1), None); // 已删除
        assert_eq!(sm.slot(id2), None); // 已删除
    }

    #[test]
    fn slot_map_to_hash_map() {
        let id0 = ParagraphId::from_u64(1);
        let id1 = ParagraphId::from_u64(2);
        let id2 = ParagraphId::from_u64(3);
        let sm = ChunkedSlotMap::from_ids(vec![id0, id1, id2]);
        let map = sm.to_hash_map();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&id0), Some(&0));
        assert_eq!(map.get(&id1), Some(&1));
        assert_eq!(map.get(&id2), Some(&2));
    }

    #[test]
    fn slot_map_splice_cross_chunk_correct() {
        // 跨块 splice：B=4，8 个 ID 跨 2 块。
        let ids: Vec<ParagraphId> = (1..=8).map(ParagraphId::from_u64).collect();
        let mut sm = ChunkedSlotMap {
            blocks: ChunkedSlotMap::split_into_blocks(ids, 4),
            chunk_capacity: 4,
            total_len: 8,
        };
        // splice [2, 6) 删除 ids[2..6] = [3,4,5,6]，插入 [id_new]。
        // 保留 ids[0,1]（前缀）+ ids[6,7]（后缀）= [1,2,7,8]。
        // 重组后：[1, 2, 99, 7, 8]，len=5。
        let id_new = ParagraphId::from_u64(99);
        sm.splice(2, 6, vec![id_new]);
        assert_eq!(sm.len(), 5);
        assert_eq!(sm.slot(ParagraphId::from_u64(1)), Some(0));
        assert_eq!(sm.slot(ParagraphId::from_u64(2)), Some(1));
        assert_eq!(sm.slot(id_new), Some(2));
        assert_eq!(sm.slot(ParagraphId::from_u64(7)), Some(3));
        assert_eq!(sm.slot(ParagraphId::from_u64(8)), Some(4));
        // 已删除
        assert_eq!(sm.slot(ParagraphId::from_u64(3)), None);
        assert_eq!(sm.slot(ParagraphId::from_u64(6)), None);
    }

    #[test]
    fn slot_map_rebuild_from() {
        let id0 = ParagraphId::from_u64(10);
        let id1 = ParagraphId::from_u64(20);
        let mut sm = ChunkedSlotMap::new();
        sm.rebuild_from(2, |i| if i == 0 { id0 } else { id1 });
        assert_eq!(sm.len(), 2);
        assert_eq!(sm.slot(id0), Some(0));
        assert_eq!(sm.slot(id1), Some(1));
    }

    #[test]
    fn slot_map_many_ids_splice() {
        // 大量 ID 测试跨块 splice 正确性。
        let ids: Vec<ParagraphId> = (1..=600).map(ParagraphId::from_u64).collect();
        let mut sm = ChunkedSlotMap::from_ids(ids);
        assert_eq!(sm.len(), 600);
        // splice [100, 500) 删除 400 个，插入 1 个。
        // 结果 len = 600 - 400 + 1 = 201。
        let id_new = ParagraphId::from_u64(999);
        sm.splice(100, 500, vec![id_new]);
        assert_eq!(sm.len(), 201); // 600 - 400 + 1
        assert_eq!(sm.slot(ParagraphId::from_u64(1)), Some(0));
        assert_eq!(sm.slot(ParagraphId::from_u64(100)), Some(99));
        assert_eq!(sm.slot(id_new), Some(100));
        assert_eq!(sm.slot(ParagraphId::from_u64(500)), None); // 500 在删除范围 [100,500) 内
        assert_eq!(sm.slot(ParagraphId::from_u64(501)), Some(101)); // 后缀从 501 开始
        assert_eq!(sm.slot(ParagraphId::from_u64(600)), Some(200)); // 600 在 slot 200
        // 验证 to_hash_map
        let map = sm.to_hash_map();
        assert_eq!(map.len(), 201);
        assert_eq!(map.get(&id_new), Some(&100));
    }

    // ── 空集合 splice/insert 回归测试（Issue #901 缺口4）──
    //
    // 空根 splice 时 find_chunk 返回 (0,0)，旧实现直接索引 self.chunks[0]/self.blocks[0]
    // 越界 panic。修复后空根插入作为合法基础操作，直接从 new_values/new_ids 创建首块。

    #[test]
    fn prefix_sum_f64_splice_empty_insert() {
        // 空集合 splice(0, 0, vec![10.0]) 不 panic，结果 len=1, get(0)=Some(10.0)。
        let mut cps = ChunkedPrefixSum::<f64>::new();
        cps.splice(0, 0, vec![10.0]);
        assert_eq!(cps.len(), 1);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.total_sum(), 10.0);
        assert_eq!(cps.prefix_sum(1), 10.0);
    }

    #[test]
    fn prefix_sum_f64_insert_into_empty() {
        // 空集合 insert(0, 10.0) 不 panic。
        let mut cps = ChunkedPrefixSum::<f64>::new();
        cps.insert(0, 10.0);
        assert_eq!(cps.len(), 1);
        assert_eq!(cps.get(0), Some(10.0));
        assert_eq!(cps.total_sum(), 10.0);
    }

    #[test]
    fn prefix_sum_f64_splice_empty_noop() {
        // 空集合 splice(0, 0, vec![]) 不 panic，仍为空。
        let mut cps = ChunkedPrefixSum::<f64>::new();
        cps.splice(0, 0, vec![]);
        assert_eq!(cps.len(), 0);
        assert!(cps.is_empty());
        assert_eq!(cps.total_sum(), 0.0);
    }

    #[test]
    fn slot_map_splice_empty_insert() {
        // 空集合 splice(0, 0, vec![some_id]) 不 panic。
        let id = ParagraphId::from_u64(1);
        let mut sm = ChunkedSlotMap::new();
        sm.splice(0, 0, vec![id]);
        assert_eq!(sm.len(), 1);
        assert_eq!(sm.slot(id), Some(0));
    }

    #[test]
    fn slot_map_insert_into_empty() {
        // 空集合 insert(0, some_id) 不 panic。
        let id = ParagraphId::from_u64(1);
        let mut sm = ChunkedSlotMap::new();
        sm.insert(0, id);
        assert_eq!(sm.len(), 1);
        assert_eq!(sm.slot(id), Some(0));
    }

    #[test]
    fn slot_map_splice_empty_noop() {
        // 空集合 splice(0, 0, vec![]) 不 panic。
        let mut sm = ChunkedSlotMap::new();
        sm.splice(0, 0, vec![]);
        assert_eq!(sm.len(), 0);
        assert!(sm.is_empty());
    }
}
