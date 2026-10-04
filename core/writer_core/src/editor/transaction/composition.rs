use serde::{Deserialize, Serialize};

use crate::editor::strong_types::Utf8ByteOffset;

/// #517: old virtualText → new virtualText 的字符身份映射。
///
/// 用于排版 reflow 时追踪哪些字符是同一逻辑对象（应做位移动画），
/// 哪些是新增/删除（应做插入/删除动画）。
///
/// 映射策略（最长公共前缀/后缀）：
/// - 前缀相同部分：old[i] → new[i]（identity）
/// - 中间差异部分：无映射（Insert/Delete/Crossfade）
/// - 后缀相同部分：old[i] → new[j]（shifted）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMap {
    /// 映射条目列表，按 old byte offset 排序
    pub entries: Vec<OffsetMapEntry>,
}

/// #517: 单个偏移映射条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMapEntry {
    /// old virtualText 中的 UTF-8 byte offset
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub old_byte_offset: Utf8ByteOffset,
    /// new virtualText 中的 UTF-8 byte offset
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub new_byte_offset: Utf8ByteOffset,
    /// 映射的字符数（UTF-8 bytes）
    pub length: usize,
    /// 映射类型
    pub kind: OffsetMapKind,
}

/// #517: 偏移映射类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OffsetMapKind {
    /// 文本和位置均相同（前缀/后缀静态部分）
    Identity,
    /// 文本相同但位置变化（后缀移动）
    Shifted,
}

impl OffsetMap {
    /// 从 old/new virtualText 构建偏移映射。
    ///
    /// 使用最长公共前缀/后缀算法确定映射区域。
    /// old == new 时返回 identity 映射 [0, len)，保证后续 reflow 算法
    /// 能通过 `map_new_range_to_old` 找到对应关系（如 IME 预输入文本不变、
    /// 只有光标/属性变化的场景）。
    pub fn build(old_text: &str, new_text: &str) -> Self {
        if old_text.is_empty() || new_text.is_empty() {
            return OffsetMap {
                entries: Vec::new(),
            };
        }

        if old_text == new_text {
            return OffsetMap {
                entries: vec![OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(0),
                    length: old_text.len(),
                    kind: OffsetMapKind::Identity,
                }],
            };
        }

        let prefix = common_prefix_byte_len(old_text, new_text);
        let suffix = common_suffix_byte_len(old_text, new_text, prefix);

        let mut entries = Vec::new();

        if prefix > 0 {
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: prefix,
                kind: OffsetMapKind::Identity,
            });
        }

        if suffix > 0 {
            let old_suffix_start = old_text.len() - suffix;
            let new_suffix_start = new_text.len() - suffix;
            let kind = if prefix > 0 || (old_text.len() != new_text.len()) {
                OffsetMapKind::Shifted
            } else {
                OffsetMapKind::Identity
            };
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(old_suffix_start),
                new_byte_offset: Utf8ByteOffset::unchecked(new_suffix_start),
                length: suffix,
                kind,
            });
        }

        OffsetMap { entries }
    }

    /// 单次编辑的偏移映射 — `[0,start)` Identity `[oldEnd,oldLen)` Shifted。
    ///
    /// `old_len` 是编辑前文本的 UTF-8 byte 长度，`old_range` 是编辑前被替换的
    /// 半开范围 `(start, end)`，`inserted_len` 是插入文本的 UTF-8 byte 长度。
    /// 普通按键不再比较 old/new 全文。
    pub fn from_single_edit(
        old_len: usize,
        old_range: (usize, usize),
        inserted_len: usize,
    ) -> Self {
        let (old_start, old_end) = old_range;
        debug_assert!(old_start <= old_end && old_end <= old_len);
        let mut entries = Vec::new();
        if old_start > 0 {
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: old_start,
                kind: OffsetMapKind::Identity,
            });
        }
        let suffix = old_len - old_end;
        if suffix > 0 {
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(old_end),
                new_byte_offset: Utf8ByteOffset::unchecked(old_start + inserted_len),
                length: suffix,
                kind: OffsetMapKind::Shifted,
            });
        }
        OffsetMap { entries }
    }

    /// 多次编辑（replace-all / delete-surrounding / undo 多 delta）的偏移映射。
    ///
    /// `edits` 为 `(old_start, old_end, new_start, new_end)` 元组列表（无需预排序，
    /// 内部按 `old_start` 升序处理；各编辑的 old range 必须互不重叠）。
    /// 静态区域（未编辑部分）生成 Identity/Shifted 映射；编辑区域无映射。
    #[allow(clippy::excessive_nesting)]
    pub fn from_edits(old_len: usize, edits: &[(usize, usize, usize, usize)]) -> Self {
        let mut sorted: Vec<(usize, usize, usize, usize)> = edits.to_vec();
        sorted.sort_by_key(|e| e.0);
        let mut entries = Vec::new();
        let mut old_pos = 0usize;
        let mut new_pos = 0usize;
        let mut first = true;
        for &(old_start, old_end, _new_start, new_end) in &sorted {
            debug_assert!(old_start >= old_pos && old_end >= old_start);
            if old_start > old_pos {
                let length = old_start - old_pos;
                let kind = if first && old_pos == 0 {
                    OffsetMapKind::Identity
                } else {
                    OffsetMapKind::Shifted
                };
                entries.push(OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(old_pos),
                    new_byte_offset: Utf8ByteOffset::unchecked(new_pos),
                    length,
                    kind,
                });
            }
            first = false;
            old_pos = old_end;
            // 相邻 deleteSurrounding 的 undo 两条 inverse
            // delta 的 new_range 同点退化为零长（如 before/after 紧邻均 point(bs)），
            // 顺序赋值 `new_pos = new_end` 时后处理的端点会覆盖前面更大的端点，尾段
            // 静态区映射偏移。同点零长编辑在最终文本中占据同一插入间隙，取所有端点
            // 最大值才是 point 处的累计位移（p + 总插入字节数）；非零长相邻编辑的
            // new_end 单调递增，max 与赋值等价，不改变既有行为。
            new_pos = new_pos.max(new_end);
        }
        if old_pos < old_len {
            entries.push(OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(old_pos),
                new_byte_offset: Utf8ByteOffset::unchecked(new_pos),
                length: old_len - old_pos,
                kind: OffsetMapKind::Shifted,
            });
        }
        OffsetMap { entries }
    }

    /// Issue #826 评论 10 阻塞 1：组合两份偏移映射 —— `A -> B` 复合 `B -> C` = `A -> C`。
    ///
    /// 只组合两份 map 中**真正有逻辑身份**的交集区间：
    /// - 先在 `self`（A->B）里找到覆盖 `[a_start, a_end)` 的静态条目；
    /// - 再用该条目的 new 区间去 `next`（B->C）里找覆盖的静态条目；
    /// - 两者交集按 old 坐标顺序产出新的静态条目。
    ///
    /// 任一侧在交集上无映射（该段被编辑过）就直接丢弃 —— 这与 `OffsetMap`
    /// 本身的语义一致：被编辑掉的字符没有"同一逻辑身份"。
    ///
    /// 用途：连续多笔编辑时，前沿/Reflow 必须把「burst 最初 base」到
    /// 「当前正文」的映射一直累计下来，不能每笔都拿两份全文重新 `build`
    /// （`build` 只是最长公共前缀+后缀，多 patch 中间的 unchanged island 会丢）。
    #[must_use]
    pub fn compose(&self, next: &OffsetMap) -> OffsetMap {
        let mut composed: Vec<OffsetMapEntry> = Vec::new();
        for left in &self.entries {
            let left_old = left.old_byte_offset.value();
            let left_len = left.length;
            if left_len == 0 {
                continue;
            }
            let left_new_start = left.new_byte_offset.value();
            let left_new_end = left_new_start + left_len;
            for right in &next.entries {
                let right_len = right.length;
                if right_len == 0 {
                    continue;
                }
                // `self` 是 A -> B（left.old 是 A 坐标、left.new 是 B 坐标），
                // `next` 是 B -> C（right.old 是 B 坐标、right.new 是 C 坐标）。
                // 所以交集必须在 **B 坐标**里求。
                let right_b_start = right.old_byte_offset.value();
                let right_b_end = right_b_start + right_len;
                let lo = left_new_start.max(right_b_start);
                let hi = left_new_end.min(right_b_end);
                if lo >= hi {
                    continue;
                }
                let old_start = left_old + (lo - left_new_start);
                let new_start = right.new_byte_offset.value() + (lo - right_b_start);
                let kind = if old_start == new_start {
                    OffsetMapKind::Identity
                } else {
                    OffsetMapKind::Shifted
                };
                // 与上一条相邻且 kind 相同时合并，避免条目无限增长。
                if let Some(last) = composed.last_mut() {
                    let last_end = last.old_byte_offset.value() + last.length;
                    if last_end == old_start && last.kind == kind {
                        last.length += hi - lo;
                        continue;
                    }
                }
                composed.push(OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(old_start),
                    new_byte_offset: Utf8ByteOffset::unchecked(new_start),
                    length: hi - lo,
                    kind,
                });
            }
        }
        composed.sort_by_key(|entry| entry.old_byte_offset.value());
        OffsetMap { entries: composed }
    }

    /// 查找 old byte offset 在 new text 中的对应位置。
    pub fn map_old_to_new(&self, old_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            let entry_old = entry.old_byte_offset.value();
            if old_byte_offset >= entry_old && old_byte_offset < entry_old + entry.length {
                let offset_within = old_byte_offset - entry_old;
                return Some(entry.new_byte_offset.value() + offset_within);
            }
        }
        None
    }

    pub fn map_new_to_old(&self, new_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            let entry_new = entry.new_byte_offset.value();
            if new_byte_offset >= entry_new && new_byte_offset < entry_new + entry.length {
                let offset_within = new_byte_offset - entry_new;
                return Some(entry.old_byte_offset.value() + offset_within);
            }
        }
        None
    }

    /// #606: 映射旧正文中的半开 byte range [old_start, old_end) 到新正文坐标。
    ///
    /// 仅当整个 range 落在同一个映射条目内时返回 `Some`（range 跨越映射/未映射
    /// 区域边界时返回 `None` — 那不指向同一逻辑对象）。range 端点恰为条目末端
    /// （`old_end == entry_old + length`）仍视为完全位于条目内（半开区间语义）。
    pub fn map_old_range_to_new(&self, old_start: usize, old_end: usize) -> Option<(usize, usize)> {
        if old_end < old_start {
            return None;
        }
        let entry = self.entries.iter().find(|entry| {
            let entry_old = entry.old_byte_offset.value();
            old_start >= entry_old && old_start < entry_old + entry.length
        })?;
        let entry_old = entry.old_byte_offset.value();
        if old_end > entry_old + entry.length {
            return None;
        }
        Some((
            entry.new_byte_offset.value() + (old_start - entry_old),
            entry.new_byte_offset.value() + (old_end - entry_old),
        ))
    }

    /// #658: 映射新正文中的半开 byte range [new_start, new_end) 到旧正文坐标。
    ///
    /// 仅当整个 range 落在同一个映射条目内时返回 `Some`（range 跨越映射/未映射
    /// 区域边界时返回 `None` — 那不指向同一逻辑对象）。range 端点恰为条目末端
    /// （`new_end == entry_new + length`）仍视为完全位于条目内（半开区间语义）。
    pub fn map_new_range_to_old(&self, new_start: usize, new_end: usize) -> Option<(usize, usize)> {
        if new_end < new_start {
            return None;
        }
        let entry = self.entries.iter().find(|entry| {
            let entry_new = entry.new_byte_offset.value();
            new_start >= entry_new && new_start < entry_new + entry.length
        })?;
        let entry_new = entry.new_byte_offset.value();
        if new_end > entry_new + entry.length {
            return None;
        }
        Some((
            entry.old_byte_offset.value() + (new_start - entry_new),
            entry.old_byte_offset.value() + (new_end - entry_new),
        ))
    }
}

// ── 辅助函数 ──

fn common_prefix_byte_len(old_text: &str, new_text: &str) -> usize {
    let mut prefix = 0;
    for ((old_index, old_char), (_, new_char)) in
        old_text.char_indices().zip(new_text.char_indices())
    {
        if old_char != new_char {
            break;
        }
        prefix = old_index + old_char.len_utf8();
    }
    prefix
}

fn common_suffix_byte_len(old_text: &str, new_text: &str, prefix: usize) -> usize {
    let old_tail = &old_text[prefix..];
    let new_tail = &new_text[prefix..];
    let mut suffix = 0;
    for ((_, old_char), (_, new_char)) in old_tail
        .char_indices()
        .rev()
        .zip(new_tail.char_indices().rev())
    {
        if old_char != new_char {
            break;
        }
        suffix += old_char.len_utf8();
    }
    suffix
}

#[cfg(test)]
mod compose_tests {
    use super::{OffsetMap, OffsetMapKind};

    /// Issue #826 评论 10 阻塞 1：`compose` 必须保留**多 patch 中间的
    /// unchanged island**。
    ///
    /// `aXbXc -> abc`（删掉两个 X）后 `abc -> ac`（删掉 b）。
    /// 第一步的精确 map 有 `a -> a` / `b -> b` / `c -> c` 三段；
    /// 第二步的 map 有 `a -> a` / `c -> c` 两段。
    /// compose 之后 `b` 已经被真正删掉，不该出现在结果里，
    /// 但 `a` 和 `c` 必须仍然有映射。
    #[test]
    fn compose_keeps_unchanged_islands() {
        // aXbXc -> abc：old_len 5，两处删除 (1,2) 与 (3,4)。
        let first = OffsetMap::from_edits(5, &[(1, 2, 1, 1), (3, 4, 2, 2)]);
        assert_eq!(first.map_old_to_new(0), Some(0), "第一笔的 a 必须有映射");
        assert_eq!(
            first.map_old_to_new(2),
            Some(1),
            "第一笔的 b（unchanged island）必须有映射"
        );

        // abc -> ac：old_len 3，删除 (1,2)。
        let second = OffsetMap::from_single_edit(3, (1, 2), 0);
        let composed = first.compose(&second);

        // b 已被第二笔真正删掉，compose 后不应再有映射。
        assert_eq!(
            composed.map_old_to_new(2),
            None,
            "被第二笔删掉的 b 不能再有映射"
        );
        // a / c 仍然保留。`aXbXc` 里 c 在 old offset 4，两处 X 删除后
        // `abc` 里 c 在 offset 2，再删掉 b 后 `ac` 里 c 在 offset 1。
        assert_eq!(composed.map_old_to_new(0), Some(0), "a 必须仍然有映射");
        assert_eq!(composed.map_old_to_new(4), Some(1), "c 必须仍然有映射");
    }

    /// `compose` 对单次编辑等价于「原 map 复合 identity」。
    #[test]
    fn compose_with_identity_is_identity_on_mapped_regions() {
        // aXbXc(5) -> aXXXc(8)：old [2,3) 被替换成 3 byte。
        let map = OffsetMap::from_single_edit(5, (2, 3), 3);
        // identity 必须建在**中间文本**（8 byte）上，否则不是 B -> B 的恒等映射。
        let identity = OffsetMap::from_single_edit(8, (0, 0), 0);
        let composed = map.compose(&identity);
        for old in 0..5usize {
            let expected = map.map_old_to_new(old);
            assert_eq!(
                composed.map_old_to_new(old),
                expected,
                "old {old} 的映射必须与 compose 前一致"
            );
        }
    }

    /// 相邻且 kind 相同的条目要合并，避免条目数随 burst 长度线性膨胀。
    #[test]
    fn compose_merges_adjacent_same_kind_entries() {
        let first = OffsetMap::from_single_edit(4, (2, 2), 2);
        let second = OffsetMap::from_single_edit(6, (0, 0), 0);
        let composed = first.compose(&second);
        assert!(
            composed.entries.len() <= 2,
            "compose 结果不应爆炸，entries = {:?}",
            composed.entries
        );
        for entry in &composed.entries {
            assert!(entry.kind == OffsetMapKind::Identity || entry.kind == OffsetMapKind::Shifted);
        }
    }

    /// `compose` 的结果必须是 old byte offset 有序的（`map_old_to_new` 依赖它）。
    #[test]
    fn compose_entries_are_sorted_by_old_offset() {
        let first = OffsetMap::from_single_edit(6, (1, 2), 1);
        let second = OffsetMap::from_single_edit(7, (3, 4), 2);
        let composed = first.compose(&second);
        for pair in composed.entries.windows(2) {
            let a_end = pair[0].old_byte_offset.value() + pair[0].length;
            assert!(
                a_end <= pair[1].old_byte_offset.value(),
                "entries 必须按 old offset 有序且不重叠：{:?}",
                composed.entries
            );
        }
    }
}
