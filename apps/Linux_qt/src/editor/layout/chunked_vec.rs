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
use std::sync::Arc;

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

/// 稳定叶块标识。
///
/// Issue #901 第三十七轮问题1: 引入稳定 `BlockId` 替代以位置编号为值的 `id_to_block`。
/// 一次拆段/合段只改变受影响叶块的 ID 定位；后缀块身份不变，**不**遍历后缀更新其
/// 全局块编号。`BlockId` 由 `ChunkedSlotMap.next_block_id` 递增分配，永不复用。
type BlockId = u64;

/// 分块 slot 映射的一个块。
#[derive(Clone, Debug)]
struct SlotBlock {
    /// 本块的稳定标识，splice 后后缀块的 `block_id` 不变。
    block_id: BlockId,
    /// 块内按 slot 顺序的 ParagraphId。
    ids: Vec<ParagraphId>,
    /// ID → 块内索引的反向查找表。
    id_to_local: HashMap<ParagraphId, usize>,
}

/// 分块 ParagraphId → slot（逻辑序号）映射 — 结构共享持久分块树。
///
/// Issue #901 第三十七轮问题1: 消除 `id_to_block` COW 全图复制和后缀 ID 重新定位。
///
/// # 核心设计
///
/// `id_to_block: Arc<HashMap<ParagraphId, BlockId>>` 的值是**稳定叶块标识** `BlockId`
/// 而非位置索引。splice 时后缀块的 `BlockId` 不变，因此**不遍历后缀块**更新其 ID 映射，
/// 消除了旧实现 O(后缀段数) 的后缀 ID 重新定位。
///
/// `block_id_to_index: Arc<HashMap<BlockId, usize>>` 维护 `BlockId → blocks Vec 索引`。
/// 其键数量是块数 O(n/B) 而非段落数 O(n)，splice 时后缀块索引平移是 O(后缀块数) =
/// O(后缀段数/B)，比旧实现 O(后缀段数) 好一个 B 因子。
///
/// `slot(id)` 通过 `id_to_block` O(1) 得 `BlockId`，`block_id_to_index` O(1) 得块索引，
/// `block_starts[block_idx]` O(1) 取块起始 slot，`block.id_to_local[id]` O(1) 取块内偏移，
/// 总 O(1)。
///
/// `blocks` / `block_starts` / `id_to_block` / `block_id_to_index` 均用 `Arc` 包装，
/// `Clone` 仅 `Arc::clone` O(1)，不深拷任何块内容。
///
/// # 不变量
///
/// - `blocks[i]` 的 `ids` 和 `id_to_local` 一致：`id_to_local[ids[j]] == j`。
/// - `block_starts[i] == blocks[0..i] 的 ids.len() 之和`，`block_starts[0] == 0`。
/// - `id_to_block[id] == block_id` 当且仅当存在 `i` 使 `blocks[i].block_id == block_id`
///   且 `blocks[i].id_to_local` 含 `id`。
/// - `block_id_to_index[block_id] == i` 当且仅当 `blocks[i].block_id == block_id`。
/// - 每块 `ids.len()` 在 `[1, 2*chunk_capacity]` 范围内。
/// - `total_len == 所有块 ids.len() 之和`。
pub struct ChunkedSlotMap {
    /// 结构共享的块列表。`Arc<Vec<Arc<SlotBlock>>>`：外层 Arc 共享块列表，
    /// 内层 Arc 共享单个块。Clone O(1)，splice 时未改块 Arc::clone 共享。
    blocks: std::sync::Arc<Vec<std::sync::Arc<SlotBlock>>>,
    /// 块起始 slot 前缀和：`block_starts[i]` = 前 i 块的 ids 总 len。
    /// 长度 = `blocks.len() + 1`，`block_starts[0] == 0`。
    block_starts: std::sync::Arc<Vec<usize>>,
    /// ID → 稳定叶块标识定位。值是 `BlockId` 而非位置索引，splice 后后缀块映射不变。
    id_to_block: std::sync::Arc<HashMap<ParagraphId, BlockId>>,
    /// 稳定叶块标识 → blocks Vec 索引。键数量 O(块数)，splice 时后缀索引平移 O(后缀块数)。
    block_id_to_index: std::sync::Arc<HashMap<BlockId, usize>>,
    /// 下一个待分配的 `BlockId`，递增不复用。
    next_block_id: BlockId,
    chunk_capacity: usize,
    total_len: usize,
}

impl Clone for ChunkedSlotMap {
    /// O(1) 结构共享 clone — 四个 `Arc::clone` + 标量，不深拷任何块内容。
    fn clone(&self) -> Self {
        Self {
            blocks: std::sync::Arc::clone(&self.blocks),
            block_starts: std::sync::Arc::clone(&self.block_starts),
            id_to_block: std::sync::Arc::clone(&self.id_to_block),
            block_id_to_index: std::sync::Arc::clone(&self.block_id_to_index),
            next_block_id: self.next_block_id,
            chunk_capacity: self.chunk_capacity,
            total_len: self.total_len,
        }
    }
}

impl Debug for ChunkedSlotMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkedSlotMap")
            .field("block_count", &self.blocks.len())
            .field("chunk_capacity", &self.chunk_capacity)
            .field("total_len", &self.total_len)
            .finish()
    }
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
            blocks: std::sync::Arc::new(Vec::new()),
            block_starts: std::sync::Arc::new(vec![0]),
            id_to_block: std::sync::Arc::new(HashMap::new()),
            block_id_to_index: std::sync::Arc::new(HashMap::new()),
            next_block_id: 1,
            chunk_capacity: DEFAULT_CHUNK_CAPACITY,
            total_len: 0,
        }
    }

    /// 从 ID 数组创建（slot = 数组索引）。
    pub fn from_ids(ids: Vec<ParagraphId>) -> Self {
        Self::from_ids_with_capacity(ids, DEFAULT_CHUNK_CAPACITY)
    }

    /// 从 ID 数组创建，指定块容量。
    pub fn from_ids_with_capacity(ids: Vec<ParagraphId>, chunk_capacity: usize) -> Self {
        let cap = chunk_capacity.max(1);
        let total_len = ids.len();
        let slot_blocks = Self::split_into_blocks(ids, cap);
        let mut next_block_id: BlockId = 1;
        let blocks: Vec<std::sync::Arc<SlotBlock>> = slot_blocks
            .into_iter()
            .map(|sb| {
                let block_id = next_block_id;
                next_block_id += 1;
                std::sync::Arc::new(SlotBlock {
                    block_id,
                    ids: sb.ids,
                    id_to_local: sb.id_to_local,
                })
            })
            .collect();
        let block_starts = Self::compute_block_starts(&blocks);
        let id_to_block = Self::compute_id_to_block(&blocks);
        let block_id_to_index = Self::compute_block_id_to_index(&blocks);
        Self {
            blocks: std::sync::Arc::new(blocks),
            block_starts: std::sync::Arc::new(block_starts),
            id_to_block: std::sync::Arc::new(id_to_block),
            block_id_to_index: std::sync::Arc::new(block_id_to_index),
            next_block_id,
            chunk_capacity: cap,
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
        self.blocks = std::sync::Arc::new(Vec::new());
        self.block_starts = std::sync::Arc::new(vec![0]);
        self.id_to_block = std::sync::Arc::new(HashMap::new());
        self.block_id_to_index = std::sync::Arc::new(HashMap::new());
        // next_block_id 继续递增，不复用旧 BlockId，保证跨 clear 的标识唯一性。
        self.total_len = 0;
    }

    /// 查询 `id` 对应的 slot（逻辑序号）。
    ///
    /// Issue #901 第三十七轮问题1: 通过 `id_to_block` O(1) 得稳定 `BlockId`，
    /// `block_id_to_index` O(1) 得块索引，`block_starts[block_idx]` O(1) 取块起始 slot，
    /// `block.id_to_local[id]` O(1) 取块内偏移，总 O(1)。
    pub fn slot(&self, id: ParagraphId) -> Option<usize> {
        let &block_id = self.id_to_block.get(&id)?;
        let &block_idx = self.block_id_to_index.get(&block_id)?;
        let block = &self.blocks[block_idx];
        let &local = block.id_to_local.get(&id)?;
        Some(self.block_starts[block_idx] + local)
    }

    /// 是否包含 `id`。O(1)。
    pub fn contains(&self, id: ParagraphId) -> bool {
        self.id_to_block.contains_key(&id)
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
    /// Issue #901 第三十七轮问题1: 消除后缀 ID 重新定位。
    ///
    /// `id_to_block` 的值是稳定 `BlockId` 而非位置索引。splice 时只删除/添加受影响块中
    /// ID 的映射，**后缀块的 `BlockId` 不变，不遍历后缀更新 `id_to_block`**，消除了旧实现
    /// O(后缀段数) 的后缀 ID 重新定位。
    ///
    /// `block_id_to_index` 的后缀块索引平移是 O(后缀块数) = O(后缀段数/B)，比旧实现
    /// O(后缀段数) 好一个 B 因子。重建 `block_starts` O(块数)。
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

        // 空根插入/删除。
        if self.blocks.is_empty() {
            if new_count == 0 {
                return;
            }
            let slot_blocks = Self::split_into_blocks(new_ids, self.chunk_capacity);
            let blocks: Vec<std::sync::Arc<SlotBlock>> = slot_blocks
                .into_iter()
                .map(|sb| {
                    let block_id = self.next_block_id;
                    self.next_block_id += 1;
                    std::sync::Arc::new(SlotBlock {
                        block_id,
                        ids: sb.ids,
                        id_to_local: sb.id_to_local,
                    })
                })
                .collect();
            let block_starts = Self::compute_block_starts(&blocks);
            let id_to_block = Self::compute_id_to_block(&blocks);
            let block_id_to_index = Self::compute_block_id_to_index(&blocks);
            self.blocks = std::sync::Arc::new(blocks);
            self.block_starts = std::sync::Arc::new(block_starts);
            self.id_to_block = std::sync::Arc::new(id_to_block);
            self.block_id_to_index = std::sync::Arc::new(block_id_to_index);
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

        // 重新分块受影响区域，并为每个新块分配稳定 BlockId。
        let new_slot_blocks = Self::split_into_blocks(all_new, self.chunk_capacity);
        let new_blocks: Vec<std::sync::Arc<SlotBlock>> = new_slot_blocks
            .into_iter()
            .map(|sb| {
                let block_id = self.next_block_id;
                self.next_block_id += 1;
                std::sync::Arc::new(SlotBlock {
                    block_id,
                    ids: sb.ids,
                    id_to_local: sb.id_to_local,
                })
            })
            .collect();
        let new_block_count = new_blocks.len();
        let old_block_count = ec - sc + 1;

        // 保存旧受影响块 [sc, ec] 的 ID 列表，供增量删除 id_to_block 映射。
        // 在 make_mut 之前用不可变借用收集，避免借用冲突。
        let old_affected_ids: Vec<ParagraphId> = (sc..=ec)
            .flat_map(|i| self.blocks[i].ids.iter().copied())
            .collect();

        // 保存旧受影响块 [sc, ec] 的 BlockId，供增量删除 block_id_to_index 映射。
        // Issue #901 第三十八轮缺口2: next_block_id 单调增长且从不复用，旧 BlockId 必须
        // 显式 remove，否则 block_id_to_index 会累积已删除块的 key 导致 Map 随编辑次数增长。
        let old_affected_block_ids: Vec<BlockId> = (sc..=ec)
            .map(|i| self.blocks[i].block_id)
            .collect();

        // 结构共享 splice：make_mut blocks（refcount==1 原地改，refcount>1 O(块数) 浅拷），
        // 替换受影响块。未改块（前缀块 [0,sc) 和后缀块 [ec+1, len)）Arc::clone 共享。
        let blocks_mut = std::sync::Arc::make_mut(&mut self.blocks);
        blocks_mut.splice(sc..=ec, new_blocks);

        // 增量更新 id_to_block：只改受影响块的 ID 映射，**不遍历后缀块**。
        // 后缀块的 BlockId 不变，id_to_block 中后缀 ID 的映射保持正确。
        let id_map = std::sync::Arc::make_mut(&mut self.id_to_block);
        // 1. 删除旧受影响块 [sc, ec] 中所有 ID 的映射。
        for &id in &old_affected_ids {
            id_map.remove(&id);
        }
        // 2. 添加新受影响块中所有 ID 的映射（值是稳定 BlockId）。
        for b in blocks_mut[sc..sc + new_block_count].iter() {
            for &id in &b.ids {
                id_map.insert(id, b.block_id);
            }
        }
        // 不遍历后缀块更新 id_to_block — BlockId 不变，映射保持正确。

        // 更新 block_id_to_index：受影响块 + 后缀块索引平移。
        // 键数量是块数 O(n/B)，后缀平移 O(后缀块数) = O(后缀段数/B)。
        let idx_map = std::sync::Arc::make_mut(&mut self.block_id_to_index);
        // 1. 删除旧受影响块的 BlockId 映射（Issue #901 第三十八轮缺口2: 避免长期内存增长）。
        for &old_bid in &old_affected_block_ids {
            idx_map.remove(&old_bid);
        }
        // 2. 受影响块：新 BlockId → 新块索引。
        for (i, b) in blocks_mut[sc..sc + new_block_count].iter().enumerate() {
            idx_map.insert(b.block_id, sc + i);
        }
        // 3. 后缀块索引平移：BlockId 不变，但 blocks Vec 中位置变了。
        let delta = (new_block_count as isize) - (old_block_count as isize);
        if delta != 0 {
            let suffix_start = sc + new_block_count;
            for i in suffix_start..blocks_mut.len() {
                idx_map.insert(blocks_mut[i].block_id, i);
            }
        }

        // 增量更新 block_starts：前缀 [0, sc] 不变，从 sc 开始重新累加后续块。
        let starts = std::sync::Arc::make_mut(&mut self.block_starts);
        let total_blocks = blocks_mut.len();
        starts.truncate(sc + 1);
        for i in sc..total_blocks {
            let prev = *starts.last().unwrap();
            starts.push(prev + blocks_mut[i].ids.len());
        }

        self.total_len = self.total_len - (splice_end - splice_start) + new_count;
    }

    /// 构建完整 `HashMap<ParagraphId, usize>`。
    ///
    /// O(段落数)。
    pub fn to_hash_map(&self) -> HashMap<ParagraphId, usize> {
        let mut map = HashMap::with_capacity(self.total_len);
        for (i, b) in self.blocks.iter().enumerate() {
            let offset = self.block_starts[i];
            for (local, &id) in b.ids.iter().enumerate() {
                map.insert(id, offset + local);
            }
        }
        map
    }

    /// 从 entries 重建（entries 按 slot 顺序提供 ID）。
    ///
    /// `id_at_slot: Fn(usize) -> ParagraphId`，对每个 slot 返回 ID。
    pub fn rebuild_from<F>(&mut self, len: usize, id_at_slot: F)
    where
        F: Fn(usize) -> ParagraphId,
    {
        let mut ids = Vec::with_capacity(len);
        for i in 0..len {
            ids.push(id_at_slot(i));
        }
        *self = Self::from_ids_with_capacity(ids, self.chunk_capacity);
    }

    // ── 内部辅助方法 ──

    /// 找到 `index` 所在块号和块内偏移。用 `block_starts` 二分 O(log 块数)。
    fn find_chunk(&self, index: usize) -> (usize, usize) {
        if self.blocks.is_empty() {
            return (0, 0);
        }
        if index >= self.total_len {
            let last_idx = self.blocks.len() - 1;
            let last_len = self.blocks[last_idx].ids.len();
            return (last_idx, last_len);
        }
        // block_starts[i] = 前 i 块总 len，递增。
        // 找最大 i 使得 block_starts[i] <= index。
        let starts = &*self.block_starts;
        let n = self.blocks.len();
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if starts[mid] <= index {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let chunk_idx = lo;
        let local_idx = index - starts[chunk_idx];
        (chunk_idx, local_idx)
    }

    /// 计算块起始 slot 前缀和。`block_starts[i]` = 前 i 块 ids 总 len。
    fn compute_block_starts(blocks: &[std::sync::Arc<SlotBlock>]) -> Vec<usize> {
        let mut starts = Vec::with_capacity(blocks.len() + 1);
        let mut acc = 0usize;
        starts.push(acc);
        for b in blocks {
            acc += b.ids.len();
            starts.push(acc);
        }
        starts
    }

    /// 计算 ID → 稳定叶块标识定位映射。
    fn compute_id_to_block(blocks: &[std::sync::Arc<SlotBlock>]) -> HashMap<ParagraphId, BlockId> {
        let total: usize = blocks.iter().map(|b| b.ids.len()).sum();
        let mut map = HashMap::with_capacity(total);
        for b in blocks.iter() {
            for &id in &b.ids {
                map.insert(id, b.block_id);
            }
        }
        map
    }

    /// 计算 稳定叶块标识 → blocks Vec 索引映射。
    fn compute_block_id_to_index(
        blocks: &[std::sync::Arc<SlotBlock>],
    ) -> HashMap<BlockId, usize> {
        let mut map = HashMap::with_capacity(blocks.len());
        for (i, b) in blocks.iter().enumerate() {
            map.insert(b.block_id, i);
        }
        map
    }

    /// 构建一个 slot 块（不带 block_id，由调用方补充）。
    fn build_block(ids: Vec<ParagraphId>) -> SlotBlock {
        let mut id_to_local = HashMap::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            id_to_local.insert(id, i);
        }
        // block_id 由 from_ids_with_capacity / splice 调用方分配，此处填 0 占位。
        SlotBlock {
            block_id: 0,
            ids,
            id_to_local,
        }
    }

    /// 将 Vec 分成多个块（返回裸 SlotBlock，供 from_ids 等包装 Arc）。
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
// ── PersistentChunkedVec：结构共享分块向量 ──
// ══════════════════════════════════════════════════════════════════════════

/// 持久化分块向量 — O(1) clone、O(B + n/B) splice、O(log(n/B)) 随机访问。
///
/// Issue #901 第三十六轮问题3: 消除 entries/visual_lines/prev_paragraphs
/// 的 Vec::splice 后缀 memmove。
///
/// # 核心设计
///
/// 将元素按固定块大小 B 分块存储，每块 `Arc<Vec<T>>`。
/// clone 仅 Arc::clone 块列表和前缀和数组，O(1)。
/// splice 只重建受影响块（1-2 块），前缀/后缀块 Arc::clone 共享，O(B + n/B)。
///
/// # 复杂度
///
/// | 操作 | Vec | PersistentChunkedVec |
/// |------|-----|---------------------|
/// | clone | O(n) | O(1) |
/// | splice | O(n) memmove | O(n/B + k*B) |
/// | get/get_mut | O(1) | O(log(n/B)) |
/// | iter | O(1) | O(1) per element |
/// | push | O(1) amortized | O(1) amortized |
pub struct PersistentChunkedVec<T: Clone> {
    /// 分块列表，每块 `Arc<Vec<T>>`，clone 时 Arc 共享。
    chunks: Arc<Vec<Arc<Vec<T>>>>,
    /// 块起始索引前缀和：`chunk_starts[i]` = 前 i 块的元素总数。
    /// `chunk_starts[0] == 0`，`chunk_starts[len] == total_len`。
    chunk_starts: Arc<Vec<usize>>,
    /// 每块最大容量 B。
    chunk_capacity: usize,
    /// 元素总数。
    total_len: usize,
}

impl<T: Clone> PersistentChunkedVec<T> {
    /// 创建空向量，使用默认块容量。
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHUNK_CAPACITY)
    }

    /// 创建空向量，指定块容量。
    pub fn with_capacity(chunk_capacity: usize) -> Self {
        let cap = chunk_capacity.max(1);
        Self {
            chunks: Arc::new(Vec::new()),
            chunk_starts: Arc::new(vec![0]),
            chunk_capacity: cap,
            total_len: 0,
        }
    }

    /// 元素总数。
    #[inline]
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// 是否为空。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// 块容量 B。
    #[inline]
    pub fn chunk_capacity(&self) -> usize {
        self.chunk_capacity
    }

    /// 清空向量。
    pub fn clear(&mut self) {
        let chunks = Arc::make_mut(&mut self.chunks);
        chunks.clear();
        let starts = Arc::make_mut(&mut self.chunk_starts);
        starts.clear();
        starts.push(0);
        self.total_len = 0;
    }

    /// 从 `Vec<T>` 构造，将元素按块容量分块。元素 move，不 clone。
    pub fn from_vec(vec: Vec<T>) -> Self {
        Self::from_vec_with_capacity(vec, DEFAULT_CHUNK_CAPACITY)
    }

    /// 从 `Vec<T>` 构造，指定块容量。元素 move，不 clone。
    pub fn from_vec_with_capacity(vec: Vec<T>, chunk_capacity: usize) -> Self {
        let cap = chunk_capacity.max(1);
        let total_len = vec.len();
        if total_len == 0 {
            return Self::with_capacity(cap);
        }
        let num_chunks = (total_len + cap - 1) / cap;
        let mut chunks: Vec<Arc<Vec<T>>> = Vec::with_capacity(num_chunks);
        let mut chunk_starts: Vec<usize> = Vec::with_capacity(num_chunks + 1);
        chunk_starts.push(0);
        // 用 into_iter 线性消费，避免 drain(..take) 从前部移除导致的 O(n²/B) 搬移。
        let mut iter = vec.into_iter();
        loop {
            let chunk: Vec<T> = iter.by_ref().take(cap).collect();
            if chunk.is_empty() {
                break;
            }
            chunk_starts.push(chunk_starts.last().unwrap() + chunk.len());
            chunks.push(Arc::new(chunk));
        }
        Self {
            chunks: Arc::new(chunks),
            chunk_starts: Arc::new(chunk_starts),
            chunk_capacity: cap,
            total_len,
        }
    }

    /// 消费 self，将所有元素收集到 `Vec<T>`。元素 move，不 clone。
    pub fn into_vec(self) -> Vec<T> {
        let total_len = self.total_len;
        let mut result: Vec<T> = Vec::with_capacity(total_len);
        // Arc::try_unwrap 优先 move，若共享则 clone。
        let chunks = match Arc::try_unwrap(self.chunks) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        };
        for chunk in chunks {
            match Arc::try_unwrap(chunk) {
                Ok(mut owned) => result.append(&mut owned),
                Err(arc) => {
                    for item in arc.iter().cloned() {
                        result.push(item);
                    }
                }
            }
        }
        result
    }

    /// 克隆所有元素到 `Vec<T>`。需要 T: Clone。
    pub fn to_vec(&self) -> Vec<T> {
        let mut result: Vec<T> = Vec::with_capacity(self.total_len);
        for chunk in self.chunks.iter() {
            for item in chunk.iter() {
                result.push(item.clone());
            }
        }
        result
    }

    /// 二分查找块索引：返回 `chunk_idx` 使得 `chunk_starts[chunk_idx] <= index < chunk_starts[chunk_idx+1]`。
    /// 若 `index >= total_len`，返回最后一个块索引。
    #[inline]
    fn find_chunk(&self, index: usize) -> usize {
        // chunk_starts 是单调递增的，用 partition_point 找第一个 > index 的位置。
        let starts = &*self.chunk_starts;
        if index >= self.total_len {
            return starts.len().saturating_sub(2); // 最后一个块
        }
        // partition_point 返回第一个 > index 的位置，减 1 得到块索引。
        starts.partition_point(|&s| s <= index).saturating_sub(1)
    }

    /// 按索引获取只读引用。
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.total_len {
            return None;
        }
        let chunk_idx = self.find_chunk(index);
        let local = index - self.chunk_starts[chunk_idx];
        self.chunks[chunk_idx].get(local)
    }

    /// 按索引获取可变引用。若块被共享（Arc refcount > 1），COW clone 该块。
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.total_len {
            return None;
        }
        let chunk_idx = self.find_chunk(index);
        let local = index - self.chunk_starts[chunk_idx];
        let chunks = Arc::make_mut(&mut self.chunks);
        let chunk = Arc::make_mut(&mut chunks[chunk_idx]);
        chunk.get_mut(local)
    }

    /// 末尾追加元素。
    pub fn push(&mut self, value: T) {
        let chunks = Arc::make_mut(&mut self.chunks);
        let starts = Arc::make_mut(&mut self.chunk_starts);
        // 若最后一块未满，追加到最后一块；否则新建一块。
        let need_new_chunk = chunks
            .last()
            .map_or(true, |c| c.len() >= self.chunk_capacity);
        self.total_len += 1;
        if need_new_chunk {
            chunks.push(Arc::new(vec![value]));
            // 新建块：push 新的末端，保持 starts.len() == chunks.len() + 1
            // 且 starts[0] == 0 不变。
            starts.push(self.total_len);
        } else {
            let last_chunk = chunks.last_mut().unwrap();
            Arc::make_mut(last_chunk).push(value);
            // 追加到已有块：仅更新最后一位前缀和。
            *starts.last_mut().unwrap() = self.total_len;
        }
    }

    /// 迭代所有元素的只读引用。
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            chunks: self.chunks.iter(),
            current: [].iter(),
        }
    }

    /// 迭代所有元素的可变引用。被共享的块会被 COW clone。
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let chunks = Arc::make_mut(&mut self.chunks);
        IterMut {
            chunks: chunks.iter_mut(),
            current: [].iter_mut(),
        }
    }

    /// 最后一个元素的只读引用。
    pub fn last(&self) -> Option<&T> {
        if self.total_len == 0 {
            return None;
        }
        let last_chunk = self.chunks.last()?;
        last_chunk.last()
    }

    /// 截断到指定长度。多余元素被丢弃。
    pub fn truncate(&mut self, len: usize) {
        if len >= self.total_len {
            return;
        }
        if len == 0 {
            self.clear();
            return;
        }
        // 先计算截断点所在块（不可变借用），再取可变借用。
        let keep_chunk_idx = self.find_chunk(len);
        let local = len - self.chunk_starts[keep_chunk_idx];
        let chunks = Arc::make_mut(&mut self.chunks);
        let starts = Arc::make_mut(&mut self.chunk_starts);
        if local == 0 {
            // 块边界截断：len 恰好是 keep_chunk_idx 块的起点，
            // 该块及其后块全部丢弃，只保留前 keep_chunk_idx 个块。
            // starts[keep_chunk_idx] == len，保留 starts[0..=keep_chunk_idx]。
            chunks.truncate(keep_chunk_idx);
            starts.truncate(keep_chunk_idx + 1);
        } else {
            // 块内截断：截短目标块到 local 个元素，保留 keep_chunk_idx+1 个块。
            let chunk = Arc::make_mut(&mut chunks[keep_chunk_idx]);
            chunk.truncate(local);
            chunks.truncate(keep_chunk_idx + 1);
            starts.truncate(keep_chunk_idx + 1);
            starts.push(len);
        }
        self.total_len = len;
    }

    /// 局部 splice — 删除 `[start, end)` 范围的元素，插入 `replacement`。
    ///
    /// Issue #901 第三十七轮问题2: 邻接块合并/重平衡容量不变量。
    ///
    /// 收集受影响区域元素（部分前缀 + replacement + 部分后缀）重新分块后，对边界执行
    /// 合并/重平衡：若新分块末块 < B/2 且有完整后缀块，纳入重新分块（循环直到末块 ≥ B/2
    /// 或无后缀块）；首块同理。确保除首尾外叶块保持 B/2..B 填充率，连续编辑不退化为
    /// 大量单元素块。完整前缀/后缀块通过 Arc::clone 共享，不 clone 元素。
    pub fn splice<R>(&mut self, range: R, replacement: Vec<T>)
    where
        R: std::ops::RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&s) => s,
            std::ops::Bound::Excluded(&s) => s + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&e) => e + 1,
            std::ops::Bound::Excluded(&e) => e,
            std::ops::Bound::Unbounded => self.total_len,
        };
        let start = start.min(self.total_len);
        let end = end.min(self.total_len);
        if start > end {
            return;
        }
        if start == 0 && end == self.total_len && replacement.is_empty() {
            self.clear();
            return;
        }

        let old_chunks = &*self.chunks;
        let old_starts = &*self.chunk_starts;
        let cap = self.chunk_capacity;

        // 空根插入：old_chunks 为空时直接从 replacement 创建。
        if old_chunks.is_empty() {
            if replacement.is_empty() {
                return;
            }
            let new_chunks = Self::split_vec_into_arcs(replacement, cap);
            let new_len: usize = new_chunks.iter().map(|c| c.len()).sum();
            let new_starts = Self::compute_starts(&new_chunks);
            self.chunks = Arc::new(new_chunks);
            self.chunk_starts = Arc::new(new_starts);
            self.total_len = new_len;
            return;
        }

        // 受影响块范围：first_affected..=last_affected。
        // find_chunk(start) 当 start==0 返回 0；find_chunk(end) 当 end==total_len 返回最后一块。
        let first_affected = self.find_chunk(start);
        let last_affected = self.find_chunk(end);
        let local_start = start - old_starts[first_affected];
        let local_end = end - old_starts[last_affected];

        // 收集受影响元素：部分前缀 + replacement + 部分后缀。
        let mut affected: Vec<T> = Vec::new();
        affected.extend(old_chunks[first_affected][..local_start].iter().cloned());
        affected.extend(replacement);
        affected.extend(old_chunks[last_affected][local_end..].iter().cloned());

        // 重新分块受影响元素。
        let mut affected_chunks = Self::split_vec_into_arcs(affected, cap);

        // 完整前缀块范围 [0, prefix_end)，完整后缀块范围 [suffix_start, len)。
        let mut prefix_end = first_affected;
        let mut suffix_start = last_affected + 1;

        // 边界合并：受影响块末尾与完整后缀块合并。
        // 循环直到末块 ≥ B/2 或无更多完整后缀块。
        Self::merge_suffix_boundaries(
            &mut affected_chunks,
            old_chunks,
            &mut suffix_start,
            cap,
        );

        // 边界合并：受影响块首部与完整前缀块合并。
        // 循环直到首块 ≥ B/2 或无更多完整前缀块。
        Self::merge_prefix_boundaries(
            &mut affected_chunks,
            old_chunks,
            &mut prefix_end,
            cap,
        );

        // 拼接：完整前缀块 + 受影响块 + 完整后缀块。
        let mut new_chunks: Vec<Arc<Vec<T>>> =
            Vec::with_capacity(prefix_end + affected_chunks.len() + (old_chunks.len() - suffix_start));
        for i in 0..prefix_end {
            new_chunks.push(Arc::clone(&old_chunks[i]));
        }
        new_chunks.extend(affected_chunks);
        for i in suffix_start..old_chunks.len() {
            new_chunks.push(Arc::clone(&old_chunks[i]));
        }

        let new_len: usize = new_chunks.iter().map(|c| c.len()).sum();
        let new_starts = Self::compute_starts(&new_chunks);

        self.chunks = Arc::new(new_chunks);
        self.chunk_starts = Arc::new(new_starts);
        self.total_len = new_len;
    }

    /// 扩展向量。
    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }

    /// 在 `index` 处插入一个元素。后续元素右移。
    /// 用 splice 实现，O(B + n/B)。
    pub fn insert(&mut self, index: usize, value: T) {
        let idx = index.min(self.total_len);
        self.splice(idx..idx, vec![value]);
    }

    /// 移除并返回 `index` 处的元素。后续元素左移。
    /// 用 splice 实现，O(B + n/B)。需要 clone 取出元素。
    pub fn remove(&mut self, index: usize) -> T {
        let value = self[index].clone();
        self.splice(index..index + 1, Vec::new());
        value
    }

    /// 二分分区点 — 返回第一个使 `pred` 返回 false 的位置。
    /// 等价于 `[T]::partition_point`，逐块查找，O(n/B + B)。
    pub fn partition_point<P: Fn(&T) -> bool>(&self, pred: P) -> usize {
        let mut count = 0usize;
        for chunk in self.chunks.iter() {
            let p = chunk.partition_point(&pred);
            count += p;
            if p < chunk.len() {
                break;
            }
        }
        count
    }

    // ── Issue #901 第三十七轮问题2: 邻接块合并/重平衡辅助方法 ──

    /// 将 `Vec<T>` 按块容量分成多个 `Arc<Vec<T>>`。
    fn split_vec_into_arcs(vec: Vec<T>, capacity: usize) -> Vec<Arc<Vec<T>>> {
        if vec.is_empty() {
            return Vec::new();
        }
        let cap = capacity.max(1);
        let mut chunks = Vec::new();
        let mut iter = vec.into_iter();
        loop {
            let chunk: Vec<T> = iter.by_ref().take(cap).collect();
            if chunk.is_empty() {
                break;
            }
            chunks.push(Arc::new(chunk));
        }
        chunks
    }

    /// 计算块起始索引前缀和。`starts[i]` = 前 i 块元素总数，`starts[0] == 0`。
    fn compute_starts(chunks: &[Arc<Vec<T>>]) -> Vec<usize> {
        let mut starts = Vec::with_capacity(chunks.len() + 1);
        let mut acc = 0usize;
        starts.push(acc);
        for c in chunks {
            acc += c.len();
            starts.push(acc);
        }
        starts
    }

    /// 合并受影响块末尾与完整后缀块。
    ///
    /// 若受影响块末块元素数 < B/2 且有完整后缀块，将末块与第一个完整后缀块合并后
    /// 重新分块。循环直到末块 ≥ B/2 或无更多完整后缀块。每次合并 clone 一个完整
    /// 后缀块的元素 O(B)，但合并后末块 ≥ B/2，循环最多 1-2 次。
    fn merge_suffix_boundaries(
        affected: &mut Vec<Arc<Vec<T>>>,
        old_chunks: &[Arc<Vec<T>>],
        suffix_start: &mut usize,
        capacity: usize,
    ) {
        let half_cap = (capacity / 2).max(1);
        loop {
            let need_merge = affected
                .last()
                .map_or(false, |c| c.len() < half_cap);
            if !need_merge || *suffix_start >= old_chunks.len() {
                break;
            }
            // 取出受影响块末块（优先 move，共享则 clone）。
            let last_arc = affected.pop().expect("need_merge 为 true 时 affected 非空");
            let last_owned: Vec<T> = match Arc::try_unwrap(last_arc) {
                Ok(v) => v,
                Err(arc) => (*arc).clone(),
            };
            // 合并末块与第一个完整后缀块。
            let suffix_chunk = &old_chunks[*suffix_start];
            let mut merged: Vec<T> =
                Vec::with_capacity(last_owned.len() + suffix_chunk.len());
            merged.extend(last_owned);
            merged.extend(suffix_chunk.iter().cloned());
            *suffix_start += 1;
            // 重新分块合并后的元素。
            let new_chunks = Self::split_vec_into_arcs(merged, capacity);
            affected.extend(new_chunks);
        }
    }

    /// 合并受影响块首部与完整前缀块。
    ///
    /// 若受影响块首块元素数 < B/2 且有完整前缀块，将首块与最后一个完整前缀块合并后
    /// 重新分块。循环直到首块 ≥ B/2 或无更多完整前缀块。
    fn merge_prefix_boundaries(
        affected: &mut Vec<Arc<Vec<T>>>,
        old_chunks: &[Arc<Vec<T>>],
        prefix_end: &mut usize,
        capacity: usize,
    ) {
        let half_cap = (capacity / 2).max(1);
        loop {
            let need_merge = affected
                .first()
                .map_or(false, |c| c.len() < half_cap);
            if !need_merge || *prefix_end == 0 {
                break;
            }
            // 取出受影响块首块（优先 move，共享则 clone）。
            let first_arc = affected.remove(0);
            let first_owned: Vec<T> = match Arc::try_unwrap(first_arc) {
                Ok(v) => v,
                Err(arc) => (*arc).clone(),
            };
            // 合并最后一个完整前缀块与首块。
            let prefix_chunk = &old_chunks[*prefix_end - 1];
            let mut merged: Vec<T> =
                Vec::with_capacity(prefix_chunk.len() + first_owned.len());
            merged.extend(prefix_chunk.iter().cloned());
            merged.extend(first_owned);
            *prefix_end -= 1;
            // 重新分块合并后的元素。
            let new_chunks = Self::split_vec_into_arcs(merged, capacity);
            // 在 affected 前面插入新分块。
            affected.splice(0..0, new_chunks);
        }
    }
}

// ── 具名迭代器类型 ──

/// `PersistentChunkedVec` 的只读迭代器。
pub struct Iter<'a, T: Clone> {
    chunks: std::slice::Iter<'a, Arc<Vec<T>>>,
    current: std::slice::Iter<'a, T>,
}

impl<'a, T: Clone> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        loop {
            if let Some(item) = self.current.next() {
                return Some(item);
            }
            match self.chunks.next() {
                Some(chunk) => self.current = chunk.iter(),
                None => return None,
            }
        }
    }
}

/// `PersistentChunkedVec` 的可变迭代器。共享块会被 COW clone。
pub struct IterMut<'a, T: Clone> {
    chunks: std::slice::IterMut<'a, Arc<Vec<T>>>,
    current: std::slice::IterMut<'a, T>,
}

impl<'a, T: Clone> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<&'a mut T> {
        loop {
            if let Some(item) = self.current.next() {
                return Some(item);
            }
            match self.chunks.next() {
                Some(chunk) => self.current = Arc::make_mut(chunk).iter_mut(),
                None => return None,
            }
        }
    }
}

// ── trait 实现 ──

impl<T: Clone> Default for PersistentChunkedVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for PersistentChunkedVec<T> {
    fn clone(&self) -> Self {
        Self {
            chunks: Arc::clone(&self.chunks),
            chunk_starts: Arc::clone(&self.chunk_starts),
            chunk_capacity: self.chunk_capacity,
            total_len: self.total_len,
        }
    }
}

impl<T: Clone + Debug> Debug for PersistentChunkedVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Clone> std::ops::Index<usize> for PersistentChunkedVec<T> {
    type Output = T;
    #[inline]
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("PersistentChunkedVec index out of bounds")
    }
}

impl<T: Clone> std::ops::IndexMut<usize> for PersistentChunkedVec<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("PersistentChunkedVec index out of bounds")
    }
}

impl<T: Clone> From<Vec<T>> for PersistentChunkedVec<T> {
    fn from(vec: Vec<T>) -> Self {
        Self::from_vec(vec)
    }
}

impl<T: Clone + PartialEq> PartialEq for PersistentChunkedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.total_len != other.total_len {
            return false;
        }
        self.iter().eq(other.iter())
    }
}

impl<T: Clone + Eq> Eq for PersistentChunkedVec<T> {}

impl<'a, T: Clone> IntoIterator for &'a PersistentChunkedVec<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut PersistentChunkedVec<T> {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
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
        let mut sm = ChunkedSlotMap::from_ids_with_capacity(ids, 4);
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

    // ── PersistentChunkedVec 测试 ──

    #[test]
    fn pcv_empty() {
        let v: PersistentChunkedVec<i32> = PersistentChunkedVec::new();
        assert!(v.is_empty());
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn pcv_from_vec_and_access() {
        let v = PersistentChunkedVec::from_vec_with_capacity(vec![10, 20, 30, 40, 50], 2);
        assert_eq!(v.len(), 5);
        assert!(!v.is_empty());
        assert_eq!(v[0], 10);
        assert_eq!(v[4], 50);
        assert_eq!(v.get(5), None);
        assert_eq!(v.get(2), Some(&30));
    }

    #[test]
    fn pcv_clone_is_o1() {
        let v = PersistentChunkedVec::from_vec_with_capacity((0..1000).collect(), 10);
        let c = v.clone();
        assert_eq!(v.len(), c.len());
        assert_eq!(v[500], c[500]);
    }

    #[test]
    fn pcv_splice_replace_middle() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7, 8], 3);
        v.splice(2..5, vec![100, 200]);
        assert_eq!(v.to_vec(), vec![1, 2, 100, 200, 6, 7, 8]);
    }

    #[test]
    fn pcv_splice_insert_at_start() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3], 2);
        v.splice(0..0, vec![10, 20]);
        assert_eq!(v.to_vec(), vec![10, 20, 1, 2, 3]);
    }

    #[test]
    fn pcv_splice_insert_at_end() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3], 2);
        v.splice(3..3, vec![10, 20]);
        assert_eq!(v.to_vec(), vec![1, 2, 3, 10, 20]);
    }

    #[test]
    fn pcv_splice_delete_all() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5], 2);
        v.splice(0..5, vec![]);
        assert!(v.is_empty());
    }

    #[test]
    fn pcv_splice_cross_chunk() {
        // 块容量 3，8 个元素分成 [0,1,2],[3,4,5],[6,7]
        // splice 1..7 替换为 [100,200]
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![0, 1, 2, 3, 4, 5, 6, 7], 3);
        v.splice(1..7, vec![100, 200]);
        assert_eq!(v.to_vec(), vec![0, 100, 200, 7]);
    }

    #[test]
    fn pcv_get_mut() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5], 2);
        *v.get_mut(2).unwrap() = 30;
        assert_eq!(v[2], 30);
    }

    #[test]
    fn pcv_iter() {
        let v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5], 2);
        let collected: Vec<i32> = v.iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn pcv_iter_mut() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4], 2);
        for x in v.iter_mut() {
            *x *= 10;
        }
        assert_eq!(v.to_vec(), vec![10, 20, 30, 40]);
    }

    #[test]
    fn pcv_push() {
        let mut v = PersistentChunkedVec::with_capacity(3);
        for i in 0..10 {
            v.push(i);
        }
        assert_eq!(v.len(), 10);
        assert_eq!(v.to_vec(), (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn pcv_truncate() {
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7], 3);
        v.truncate(4);
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn pcv_into_vec() {
        let v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5], 2);
        assert_eq!(v.into_vec(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn pcv_partition_point() {
        let v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 3, 5, 7, 9, 11], 2);
        // 找第一个 >= 6 的位置
        let pp = v.partition_point(|x| *x < 6);
        assert_eq!(pp, 3); // [1,3,5] < 6, [7,...] >= 6
        assert_eq!(v[pp], 7);
    }

    #[test]
    fn pcv_last() {
        let v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3], 2);
        assert_eq!(v.last(), Some(&3));
    }

    #[test]
    fn pcv_clear() {
        let mut v = PersistentChunkedVec::from_vec(vec![1, 2, 3]);
        v.clear();
        assert!(v.is_empty());
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn pcv_splice_preserves_suffix_sharing() {
        // splice 后前缀/后缀块应通过 Arc 共享，不深拷。
        let v = PersistentChunkedVec::from_vec_with_capacity(
            (0..100).collect::<Vec<_>>(),
            10,
        );
        let mut v2 = v.clone();
        // splice 中间一小段：删除 [40,42)（2 个元素），插入 1 个。
        // 净变化 -1，后缀左移 1。
        v2.splice(40..42, vec![999]);
        // 前缀不变
        assert_eq!(v[0], v2[0]);
        assert_eq!(v[39], v2[39]);
        // splice 点
        assert_eq!(v2[40], 999);
        // 后缀值保留（原 index 42 现在在 index 41）
        assert_eq!(v[42], v2[41]);
        assert_eq!(v[50], v2[49]);
        assert_eq!(v2.len(), 99);
    }

    // ── Issue #901 push/truncate 跨块不变量测试 ──

    #[test]
    fn pcv_push_cross_chunk() {
        // 块容量 2：push 3 个元素应跨 2 块，验证 chunk_starts 不变量。
        let mut v = PersistentChunkedVec::with_capacity(2);
        v.push(10);
        assert_eq!(v.get(0), Some(&10));
        assert_eq!(v.to_vec(), vec![10]);
        v.push(20);
        v.push(30); // 跨块：第 2 块
        assert_eq!(v[0], 10);
        assert_eq!(v[1], 20);
        assert_eq!(v[2], 30);
        assert_eq!(v.to_vec(), vec![10, 20, 30]);
        assert_eq!(v.last(), Some(&30));
        // 继续跨块
        v.push(40);
        v.push(50); // 第 3 块
        assert_eq!(v.to_vec(), vec![10, 20, 30, 40, 50]);
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn pcv_push_preserves_starts_invariant() {
        // 大量 push 验证 chunk_starts[0]==0 且 len==chunks+1。
        let mut v = PersistentChunkedVec::with_capacity(3);
        for i in 0..20 {
            v.push(i);
        }
        assert_eq!(v.len(), 20);
        assert_eq!(v.to_vec(), (0..20).collect::<Vec<_>>());
        // 随机访问跨块
        assert_eq!(v[0], 0);
        assert_eq!(v[3], 3);
        assert_eq!(v[6], 6);
        assert_eq!(v[19], 19);
    }

    #[test]
    fn pcv_truncate_block_boundary() {
        // 容量 3：[1,2,3],[4,5,6],[7] → truncate(3) 应只保留 [1,2,3]。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7], 3);
        v.truncate(3);
        assert_eq!(v.len(), 3);
        assert_eq!(v.to_vec(), vec![1, 2, 3]);
        assert_eq!(v.last(), Some(&3));
        // get 越界返回 None
        assert_eq!(v.get(3), None);
    }

    #[test]
    fn pcv_truncate_within_block() {
        // 容量 3：[1,2,3],[4,5,6],[7] → truncate(4) 应保留 [1,2,3,4]。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7], 3);
        v.truncate(4);
        assert_eq!(v.len(), 4);
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4]);
        assert_eq!(v.last(), Some(&4));
    }

    #[test]
    fn pcv_truncate_then_access() {
        // truncate 后继续 get/get_mut/index 应正确。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7, 8, 9], 3);
        v.truncate(6); // 块边界：保留 [1,2,3],[4,5,6]
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(v[5], 6);
        *v.get_mut(5).unwrap() = 60;
        assert_eq!(v[5], 60);
        // 再 truncate 到块内
        v.truncate(4);
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn pcv_truncate_block_boundary_multi() {
        // 多块边界截断：容量 2，10 个元素 5 块。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(
            (1..=10).collect::<Vec<_>>(),
            2,
        );
        v.truncate(6); // 块边界：保留前 3 块 [1,2],[3,4],[5,6]
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(v.len(), 6);
        v.truncate(4); // 块边界：保留前 2 块
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4]);
        v.truncate(2); // 块边界：保留前 1 块
        assert_eq!(v.to_vec(), vec![1, 2]);
    }

    // ── Issue #901 第三十七轮问题1: BlockId 稳定标识，消除后缀 ID 重新定位 ──

    #[test]
    fn slot_map_splice_suffix_block_id_stable() {
        // 间接验证：splice 后后缀 ID 的 slot 正确，说明 BlockId 不变、不遍历后缀。
        // B=4，8 个 ID 跨 2 块：[1,2,3,4],[5,6,7,8]。
        let ids: Vec<ParagraphId> = (1..=8).map(ParagraphId::from_u64).collect();
        let mut sm = ChunkedSlotMap::from_ids_with_capacity(ids, 4);
        // splice [0, 0) 插入 1 个新 ID（块数增加，后缀块索引平移）。
        let id_new = ParagraphId::from_u64(99);
        sm.splice(0, 0, vec![id_new]);
        // 后缀 ID 的 slot 应正确平移。
        assert_eq!(sm.slot(id_new), Some(0));
        assert_eq!(sm.slot(ParagraphId::from_u64(1)), Some(1));
        assert_eq!(sm.slot(ParagraphId::from_u64(8)), Some(8));
        assert_eq!(sm.len(), 9);
    }

    #[test]
    fn slot_map_splice_clone_then_splice_suffix_correct() {
        // clone 后 splice，两个副本的 slot 都正确。
        // ids = 1..=100，ParagraphId(k) 在 slot k-1。
        let ids: Vec<ParagraphId> = (1..=100).map(ParagraphId::from_u64).collect();
        let sm = ChunkedSlotMap::from_ids(ids);
        let mut sm2 = sm.clone();
        // 在 sm2 中 splice 删除 slot [40, 60)（即 ParagraphId(41)..ParagraphId(60)），插入 999。
        sm2.splice(40, 60, vec![ParagraphId::from_u64(999)]);
        // 原始 sm 不受影响：ParagraphId(50) 在 slot 49。
        assert_eq!(sm.slot(ParagraphId::from_u64(50)), Some(49));
        // sm2 后缀 slot 正确：999 在 slot 40，ParagraphId(61) 在 slot 41，ParagraphId(100) 在 slot 80。
        assert_eq!(sm2.slot(ParagraphId::from_u64(999)), Some(40));
        assert_eq!(sm2.slot(ParagraphId::from_u64(61)), Some(41));
        assert_eq!(sm2.slot(ParagraphId::from_u64(100)), Some(80));
    }

    #[test]
    fn slot_map_splice_repeated_no_suffix_relocation() {
        // 连续 splice 验证后缀 slot 始终正确（间接验证不遍历后缀更新 id_to_block）。
        let ids: Vec<ParagraphId> = (1..=500).map(ParagraphId::from_u64).collect();
        let mut sm = ChunkedSlotMap::from_ids(ids);
        // 连续在头部插入 100 个新 ID（1000..1099）。
        // 插入顺序：1000 先插入（最终在 slot 99），1099 最后插入（在 slot 0）。
        for i in 0..100 {
            let id_new = ParagraphId::from_u64(1000 + i);
            sm.splice(0, 0, vec![id_new]);
        }
        assert_eq!(sm.len(), 600);
        // 验证后缀 ID slot 正确：ParagraphId(1) 原在 slot 0，现平移到 slot 100。
        assert_eq!(sm.slot(ParagraphId::from_u64(1)), Some(100));
        assert_eq!(sm.slot(ParagraphId::from_u64(500)), Some(599));
        // 验证新插入的 ID：1099 最后插入在 slot 0，1000 最先插入在 slot 99。
        assert_eq!(sm.slot(ParagraphId::from_u64(1099)), Some(0));
        assert_eq!(sm.slot(ParagraphId::from_u64(1000)), Some(99));
    }

    #[test]
    fn slot_map_splice_block_id_consistency_after_multiple_splices() {
        // 多次 splice 后，所有 ID 的 slot 与 to_hash_map 一致。
        let ids: Vec<ParagraphId> = (1..=200).map(ParagraphId::from_u64).collect();
        let mut sm = ChunkedSlotMap::from_ids_with_capacity(ids, 32);
        // 第一次 splice：删除 [50, 150)，插入 3 个新 ID。
        sm.splice(
            50,
            150,
            vec![
                ParagraphId::from_u64(1001),
                ParagraphId::from_u64(1002),
                ParagraphId::from_u64(1003),
            ],
        );
        // 第二次 splice：在头部插入 5 个。
        let new_head: Vec<ParagraphId> =
            (2000..2005).map(ParagraphId::from_u64).collect();
        sm.splice(0, 0, new_head.clone());
        // 第三次 splice：删除尾部 10 个。
        sm.splice(sm.len().saturating_sub(10), sm.len(), Vec::new());
        // 验证 slot 与 to_hash_map 一致。
        let map = sm.to_hash_map();
        for (&id, &slot) in &map {
            assert_eq!(sm.slot(id), Some(slot), "slot({:?}) 不一致", id);
        }
        assert_eq!(map.len(), sm.len());
    }

    // ── Issue #901 第三十七轮问题2: 邻接块合并/重平衡，消除单元素块退化 ──

    #[test]
    fn pcv_repeated_head_insert_block_count_linear() {
        // 连续 n 次头部插入后，块数应约为 n/B 而非 n。
        let b = 64;
        let n = 1000;
        let mut v = PersistentChunkedVec::with_capacity(b);
        // 先放 1 个元素。
        v.push(0);
        // 连续头部插入 n 个元素。
        for i in 1..=n {
            v.splice(0..0, vec![i]);
        }
        assert_eq!(v.len(), n + 1);
        // 验证正确性。
        let vec = v.to_vec();
        assert_eq!(vec[0], n);
        assert_eq!(vec[n], 0);
        // 块数应约为 (n+1)/B，远小于 n。
        let block_count = v.chunks.len();
        // 允许 4 倍裕度：块数 ≤ 4*(n/B) + 4（首尾边界块 + 合并裕度）。
        let expected = (n + 1 + b - 1) / b;
        assert!(
            block_count <= expected * 4 + 4,
            "块数 {} 远超预期 {}，连续头部插入退化为大量单元素块",
            block_count,
            expected
        );
    }

    #[test]
    fn pcv_repeated_head_insert_no_single_element_blocks() {
        // 连续头部插入后，除首尾外不应有大量单元素块。
        let b = 32;
        let n = 500;
        let mut v = PersistentChunkedVec::with_capacity(b);
        v.push(0);
        for i in 1..=n {
            v.insert(0, i);
        }
        assert_eq!(v.len(), n + 1);
        // 验证正确性。
        for i in 0..=n {
            assert_eq!(v[i], n - i);
        }
        // 统计 < B/2 的块数（首尾除外）。
        let half_b = b / 2;
        let small_blocks = v
            .chunks
            .iter()
            .skip(1) // 跳过首块
            .take(v.chunks.len().saturating_sub(2)) // 跳过尾块
            .filter(|c| c.len() < half_b)
            .count();
        // 中间块不应有大量小块。
        assert!(
            small_blocks <= 2,
            "中间有 {} 个块 < B/2={}，邻接块合并未生效",
            small_blocks,
            half_b
        );
    }

    #[test]
    fn pcv_splice_merge_suffix_boundary() {
        // 验证 splice 后末块与后缀块合并。
        // B=4，初始 [1,2,3,4],[5,6,7,8]（2 块）。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7, 8], 4);
        // splice [0, 0) 插入 [10]：受影响 = [10] + [1,2,3,4] = [10,1,2,3,4]，
        // 分块 = [10,1,2,3],[4]。末块 [4] (1 < B/2=2)，合并后缀 [5,6,7,8]
        // → [4,5,6,7,8]，分块 = [4,5,6,7],[8]。末块 [8] (1 < 2)，无更多后缀块，停。
        v.splice(0..0, vec![10]);
        assert_eq!(v.to_vec(), vec![10, 1, 2, 3, 4, 5, 6, 7, 8]);
        // 块数应 ≤ 4（不会退化为 4+ 个单元素块）。
        assert!(v.chunks.len() <= 4, "块数 {} 过多", v.chunks.len());
    }

    #[test]
    fn pcv_splice_merge_prefix_boundary() {
        // 验证 splice 后首块与前缀块合并。
        // B=4，初始 [1,2,3,4],[5,6,7,8]（2 块）。
        let mut v = PersistentChunkedVec::from_vec_with_capacity(vec![1, 2, 3, 4, 5, 6, 7, 8], 4);
        // splice [7, 8) 删除 8，插入 [10,20,30]。
        v.splice(7..8, vec![10, 20, 30]);
        assert_eq!(v.to_vec(), vec![1, 2, 3, 4, 5, 6, 7, 10, 20, 30]);
    }

    #[test]
    fn pcv_splice_repeated_tail_insert_block_count() {
        // 连续尾部插入也不应退化为单元素块。
        let b = 64;
        let n = 1000;
        let mut v = PersistentChunkedVec::with_capacity(b);
        for i in 0..n {
            v.splice(i..i, vec![i]);
        }
        assert_eq!(v.len(), n);
        let vec = v.to_vec();
        for i in 0..n {
            assert_eq!(vec[i], i);
        }
        let block_count = v.chunks.len();
        let expected = (n + b - 1) / b;
        assert!(
            block_count <= expected * 4 + 4,
            "尾部插入块数 {} 远超预期 {}",
            block_count,
            expected
        );
    }

    #[test]
    fn pcv_splice_repeated_middle_insert_block_count() {
        // 连续中间插入也不应退化为单元素块。
        let b = 64;
        let n = 500;
        let mut v = PersistentChunkedVec::with_capacity(b);
        v.push(0);
        for i in 1..=n {
            // 在中间位置插入。
            let mid = i / 2;
            v.splice(mid..mid, vec![i]);
        }
        assert_eq!(v.len(), n + 1);
        let block_count = v.chunks.len();
        let expected = (n + 1 + b - 1) / b;
        assert!(
            block_count <= expected * 4 + 4,
            "中间插入块数 {} 远超预期 {}",
            block_count,
            expected
        );
    }

    #[test]
    fn pcv_splice_alternating_insert_delete_no_degradation() {
        // 交替插入删除不应退化为单元素块。
        let b = 32;
        let mut v = PersistentChunkedVec::with_capacity(b);
        // 先填充 200 个元素。
        for i in 0..200 {
            v.push(i);
        }
        // 交替头部插入和尾部删除。
        for i in 0..200 {
            v.splice(0..0, vec![1000 + i]);
            if v.len() > 200 {
                v.splice(v.len() - 1..v.len(), Vec::new());
            }
        }
        assert_eq!(v.len(), 200);
        let block_count = v.chunks.len();
        let expected = (200 + b - 1) / b;
        assert!(
            block_count <= expected * 4 + 4,
            "交替插入删除块数 {} 远超预期 {}",
            block_count,
            expected
        );
    }
}
