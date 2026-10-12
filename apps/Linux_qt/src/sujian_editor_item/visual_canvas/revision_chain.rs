//! Issue #901：presented revision → latest Core revision 的精确 offset map 组合。
//!
//! 维护从**最后已提交帧对应的 revision**（`presented_revision`）到**最新 Core
//! revision**（`latest_revision`）的累计字符身份映射。
//!
//! ## 核心规则
//!
//! - 连续中间编辑没有已提交帧时，只累计映射并覆盖 target，**不**从中途的未显示
//!   状态凭空生成 glyph。
//! - 多 patch 映射要保留中间 unchanged islands —— 用 `OffsetMap::compose` 精确组合，
//!   **不**用 `OffsetMap::build(old_text, new_text)` 的最长前后缀近似代替。
//! - 新帧提交后调用 `reset_to_committed` 把 presented_revision 前进到该帧的
//!   revision，累计映射归零。

use std::collections::HashMap;
use writer_core::editor::{EditorRevision, OffsetMap};

/// 从 `presented_revision` 到 `latest_revision` 的累计 offset map 组合。
///
/// `accumulated_map` 为 `None` 表示 presented_revision == latest_revision
/// （identity，无需映射）。每次 `compose` 用 `OffsetMap::compose` 精确组合，
/// 保留多 patch 中间的 unchanged islands。
///
/// Issue #901 第七轮：同时维护 `intermediate_maps`，为每个中间 revision
/// （`presented < rev < latest`）保留该 revision → latest 的后缀 OffsetMap 组合，
/// 避免把中间 revision 的坐标误用 `accumulated_map`（presented→latest）映射。
#[derive(Clone, Debug)]
pub(crate) struct RevisionChain {
    /// 最后已提交帧对应的 canonical revision。
    presented_revision: EditorRevision,
    /// 最新 Core revision。
    latest_revision: EditorRevision,
    /// 累计的 offset map（presented_revision → latest_revision）。
    ///
    /// `None` 表示 identity（两者相同或尚无编辑）。
    accumulated_map: Option<OffsetMap>,
    /// 中间 revision → latest_revision 的后缀映射链。
    ///
    /// key = 中间 revision（`presented_revision < rev < latest_revision`），
    /// value = 该 revision → latest_revision 的映射：
    /// - `None` 表示 identity（该 revision 与 latest 间只有纯选区/光标操作）。
    /// - `Some(map)` 表示用 `map` 映射。
    ///
    /// 每次 `compose` 时，为新产生的中间 revision 记录从该 revision 到 latest 的映射，
    /// 并把已有映射与新的 `next_offset_map` 精确组合。`reset_to_committed` 时清空。
    /// 这避免旧实现把中间 revision 的坐标误用 `accumulated_map`（presented→latest）
    /// 映射：不同 revision 的 offset 属于不同坐标系，不能混用。
    intermediate_maps: HashMap<EditorRevision, Option<OffsetMap>>,
}

impl RevisionChain {
    /// 创建一条新的 revision chain，初始时 presented == latest == given revision。
    pub(crate) fn new(presented_revision: EditorRevision) -> Self {
        Self {
            presented_revision,
            latest_revision: presented_revision,
            accumulated_map: None,
            intermediate_maps: HashMap::new(),
        }
    }

    /// 返回最后已提交帧对应的 revision。
    pub(crate) fn presented_revision(&self) -> EditorRevision {
        self.presented_revision
    }

    /// 返回最新 Core revision。
    pub(crate) fn latest_revision(&self) -> EditorRevision {
        self.latest_revision
    }

    /// 是否有未提交的编辑（presented != latest）。
    pub(crate) fn has_pending_edits(&self) -> bool {
        self.presented_revision != self.latest_revision
    }

    /// 组合一笔新的 offset map，推进 latest revision。
    ///
    /// `next_offset_map` 是 `base_revision → new_revision` 的映射，其中
    /// `base_revision` 必须等于当前 `latest_revision`。内部用
    /// `OffsetMap::compose` 精确组合，保留中间 unchanged islands。
    ///
    /// 连续中间编辑没有已提交帧时只累计映射，不从中间未显示状态生成 glyph。
    pub(crate) fn compose(&mut self, next_offset_map: &OffsetMap, new_revision: EditorRevision) {
        // Issue #901 第七轮：维护中间 revision → latest 的后缀映射链。
        //
        // `next_offset_map` 是 `latest_revision → new_revision` 的映射。
        // 1. 已有的中间映射都要与 `next_offset_map` 精确组合（latest 前进了）。
        // 2. 旧的 `latest_revision` 若不等于 `presented_revision`，它变成新的中间
        //    revision，其到新 latest 的映射就是 `next_offset_map` 本身。
        // 3. 推进 `accumulated_map` 和 `latest_revision`。
        let next_is_empty = next_offset_map.entries.is_empty();

        // 1. 已有中间映射与 next_offset_map 精确组合（latest 前进了）
        for map_opt in self.intermediate_maps.values_mut() {
            let new_map = if next_is_empty {
                // next 为空：该 revision 的所有字符在 latest 中已不存在
                OffsetMap {
                    entries: Vec::new(),
                }
            } else {
                match map_opt.as_ref() {
                    None => next_offset_map.clone(),
                    Some(map) => map.compose(next_offset_map),
                }
            };
            *map_opt = Some(new_map);
        }

        // 2. 旧的 latest_revision 变成中间 revision（若它不等于 presented_revision）。
        //    其到新 latest 的映射 = next_offset_map（空 map 时 clone 也是空）。
        if self.latest_revision != self.presented_revision {
            self.intermediate_maps
                .insert(self.latest_revision, Some(next_offset_map.clone()));
        }

        // 3. 推进累计映射
        if next_is_empty {
            // 编辑后正文为空或无静态映射：累计映射变为空（所有旧字符都被删除）。
            self.accumulated_map = Some(OffsetMap {
                entries: Vec::new(),
            });
        } else {
            self.accumulated_map = Some(match &self.accumulated_map {
                None => next_offset_map.clone(),
                Some(current) => current.compose(next_offset_map),
            });
        }
        self.latest_revision = new_revision;
    }

    /// 将 presented byte offset 映射到 latest revision 坐标。
    ///
    /// 返回 `None` 表示该 offset 在累计编辑中被删除（无逻辑身份）。
    /// presented == latest 时直接返回原 offset（identity）。
    pub(crate) fn map_presented_to_latest(&self, offset: usize) -> Option<usize> {
        match &self.accumulated_map {
            None => Some(offset),
            Some(map) => map.map_old_to_new(offset),
        }
    }

    /// 将 latest byte offset 映射回 presented revision 坐标。
    ///
    /// 返回 `None` 表示该 offset 是新增字符（在 presented revision 中不存在）。
    pub(crate) fn map_latest_to_presented(&self, offset: usize) -> Option<usize> {
        match &self.accumulated_map {
            None => Some(offset),
            Some(map) => map.map_new_to_old(offset),
        }
    }

    /// Issue #901 问题4：将 presented 中的半开 byte range 映射到 latest revision 坐标。
    ///
    /// 仅当整个 range 落在同一个映射条目内时返回 `Some`（range 跨越映射/未映射
    /// 区域边界时返回 `None` — 那不指向同一逻辑对象）。这比仅凭
    /// `map_presented_to_latest(old_end)` 的起点是 `Some` 更精确：
    /// 半开范围映射条目末端本来就可能返回 `None`，不能据此宣告整条 run 存活。
    ///
    /// presented == latest 时直接返回原 range（identity）。
    pub(crate) fn map_presented_range_to_latest(
        &self,
        old_start: usize,
        old_end: usize,
    ) -> Option<(usize, usize)> {
        match &self.accumulated_map {
            None => Some((old_start, old_end)),
            Some(map) => map.map_old_range_to_new(old_start, old_end),
        }
    }

    /// Issue #901 第五轮阻塞1 / 第六轮阻塞4 / 第七轮：将任意源 revision 的 byte offset
    /// 映射到 latest revision 坐标。
    ///
    /// - 如果 `source_revision == latest_revision`，直接返回原 offset（已经是 latest 坐标）。
    /// - 如果 `source_revision == presented_revision`，用累计映射映射。
    /// - 如果 `presented_revision < source_revision < latest_revision`（中间 revision），
    ///   用 `intermediate_maps` 中为该 revision 保留的「该 revision → latest」后缀映射。
    ///   旧实现误用 `accumulated_map`（presented→latest），把中间 revision 的坐标当作
    ///   presented 坐标映射，导致字符身份错位（如把新插入的 X 错认成旧的 a）。
    /// - 如果 `source_revision < presented_revision` 或未知 revision，返回 `None`
    ///   （旧 session 的贡献，无法精确映射）。
    ///
    /// Issue #901 第七轮反例：presented=`ab`(rev0)，前插 `X` 得 `Xab`(rev1)，再前插 `Y`
    /// 得 `YXab`(rev2)。rev1 中 `X` 的偏移 0 必须用 rev1→rev2 的映射映到 1，而不是用
    /// accumulated_map（rev0→rev2）映到 2（那是 `a` 的位置）。
    pub(crate) fn map_to_latest(
        &self,
        source_revision: EditorRevision,
        offset: usize,
    ) -> Option<usize> {
        if source_revision == self.latest_revision {
            return Some(offset);
        }
        if source_revision == self.presented_revision {
            return self.map_presented_to_latest(offset);
        }
        // Issue #901 第七轮：中间 revision 用 intermediate_maps 中保留的
        // 「该 revision → latest」后缀映射，不用 accumulated_map（presented→latest）。
        if let Some(map_opt) = self.intermediate_maps.get(&source_revision) {
            return match map_opt {
                None => Some(offset), // identity（该 revision 与 latest 间只有选区操作）
                Some(map) => map.map_old_to_new(offset),
            };
        }
        // source_revision < presented_revision 或未知 revision：无法精确映射。
        None
    }

    /// Issue #901 第五轮阻塞1 / 第六轮阻塞4 / 第七轮：将任意源 revision 的半开 byte range
    /// 映射到 latest revision 坐标。
    ///
    /// - 如果 `source_revision == latest_revision`，直接返回原 range。
    /// - 如果 `source_revision == presented_revision`，用累计映射映射。
    /// - 如果 `presented_revision < source_revision < latest_revision`（中间 revision），
    ///   用 `intermediate_maps` 中保留的「该 revision → latest」后缀映射（同
    ///   `map_to_latest` 的中间 revision 处理）。
    /// - 如果 `source_revision < presented_revision` 或未知 revision，返回 `None`
    ///   （旧 session 的贡献，无法精确映射）。
    ///
    /// Issue #901 第七轮：旧实现把中间 revision 的 range 误用 `accumulated_map`
    /// （presented→latest）映射，导致字符身份错位。现在用 intermediate_maps 精确映射。
    pub(crate) fn map_range_to_latest(
        &self,
        source_revision: EditorRevision,
        start: usize,
        end: usize,
    ) -> Option<(usize, usize)> {
        if source_revision == self.latest_revision {
            return Some((start, end));
        }
        if source_revision == self.presented_revision {
            return self.map_presented_range_to_latest(start, end);
        }
        // Issue #901 第七轮：中间 revision 用 intermediate_maps 中保留的后缀映射。
        if let Some(map_opt) = self.intermediate_maps.get(&source_revision) {
            return match map_opt {
                None => Some((start, end)), // identity
                Some(map) => map.map_old_range_to_new(start, end),
            };
        }
        // source_revision < presented_revision 或未知 revision：无法精确映射。
        None
    }

    /// 新帧已提交：把 presented_revision 前进到该帧的 revision，累计映射归零。
    ///
    /// 调用时机：Qt `afterFrameEnd` 确认了一帧，且该帧的 `source_revision`
    /// 等于当前 `latest_revision`（即画面已追上 Core）。
    pub(crate) fn reset_to_committed(&mut self, committed_revision: EditorRevision) {
        self.presented_revision = committed_revision;
        self.latest_revision = committed_revision;
        self.accumulated_map = None;
        // Issue #901 第七轮：清空中间映射链，所有中间 revision 不再有效。
        self.intermediate_maps.clear();
    }

    /// 推进 latest revision 而不改变累计映射。
    ///
    /// 用于纯选区/光标操作（正文不变、`offset_map` 为 `None`）：
    /// revision 前进了但字符身份映射仍然有效，不需要 compose。
    pub(crate) fn advance_revision(&mut self, new_revision: EditorRevision) {
        // Issue #901 第七轮：纯选区/光标操作，正文不变。旧的 latest_revision
        // 若不等于 presented_revision，它变成中间 revision，其到新 latest 的映射
        // 是 identity（None）。已有中间映射不变（正文没变，无需 compose）。
        if self.latest_revision != self.presented_revision
            && self.latest_revision != new_revision
        {
            self.intermediate_maps.insert(self.latest_revision, None);
        }
        self.latest_revision = new_revision;
    }

    /// 返回累计映射的引用（仅供调试 / 测试检查）。
    #[cfg(test)]
    pub(crate) fn accumulated_map(&self) -> Option<&OffsetMap> {
        self.accumulated_map.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_core::editor::OffsetMapEntry;
    use writer_core::editor::strong_types::Utf8ByteOffset;

    fn identity_map(len: usize) -> OffsetMap {
        OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: len,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        }
    }

    fn shifted_map(old_start: usize, new_start: usize, len: usize) -> OffsetMap {
        OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(old_start),
                new_byte_offset: Utf8ByteOffset::unchecked(new_start),
                length: len,
                kind: writer_core::editor::OffsetMapKind::Shifted,
            }],
        }
    }

    #[test]
    fn new_chain_is_identity() {
        let rev = EditorRevision::new(5);
        let chain = RevisionChain::new(rev);
        assert_eq!(chain.presented_revision(), rev);
        assert_eq!(chain.latest_revision(), rev);
        assert!(!chain.has_pending_edits());
        // identity 映射
        assert_eq!(chain.map_presented_to_latest(10), Some(10));
        assert_eq!(chain.map_latest_to_presented(10), Some(10));
    }

    #[test]
    fn compose_single_edit() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // "ab" → "Xb"：删除 a(0..1)，插入 X。offset map: old[1,2) → new[1,2) shifted
        let map = shifted_map(1, 1, 1);
        let rev1 = EditorRevision::new(1);
        chain.compose(&map, rev1);

        assert_eq!(chain.latest_revision(), rev1);
        assert!(chain.has_pending_edits());
        // old offset 1 (b) → new offset 1
        assert_eq!(chain.map_presented_to_latest(1), Some(1));
        // old offset 0 (a) 被删除 → None
        assert_eq!(chain.map_presented_to_latest(0), None);
    }

    #[test]
    fn compose_preserves_unchanged_islands() {
        // 测试多 patch 中间 unchanged island 保留
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // 第一笔："abcde" → "aXcde"：替换 b 为 X
        // old[0,1) identity, old[2,5) shifted to new[2,5)
        let map1 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(0),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Identity,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(2),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 3,
                    kind: writer_core::editor::OffsetMapKind::Identity,
                },
            ],
        };
        chain.compose(&map1, EditorRevision::new(1));

        // 第二笔："aXcde" → "aXcYe"：替换 d 为 Y
        // old[0,3) identity, old[4,5) shifted to new[4,5)
        let map2 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(0),
                    length: 3,
                    kind: writer_core::editor::OffsetMapKind::Identity,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(4),
                    new_byte_offset: Utf8ByteOffset::unchecked(4),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Identity,
                },
            ],
        };
        chain.compose(&map2, EditorRevision::new(2));

        // 中间 unchanged island 'c' (original offset 2) 应该保留映射
        // 经过两笔编辑后，c 仍在 offset 2
        assert_eq!(
            chain.map_presented_to_latest(2),
            Some(2),
            "unchanged island 'c' must be preserved across composed maps"
        );
        // 'a' (original offset 0) 也保留
        assert_eq!(chain.map_presented_to_latest(0), Some(0));
        // 'e' (original offset 4) 也保留
        assert_eq!(chain.map_presented_to_latest(4), Some(4));
    }

    #[test]
    fn reset_to_committed_clears_chain() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        let map = shifted_map(1, 1, 1);
        chain.compose(&map, EditorRevision::new(1));
        assert!(chain.has_pending_edits());

        // 新帧提交，画面追上 Core
        chain.reset_to_committed(EditorRevision::new(1));
        assert!(!chain.has_pending_edits());
        assert_eq!(chain.presented_revision(), EditorRevision::new(1));
        assert_eq!(chain.latest_revision(), EditorRevision::new(1));
        assert!(chain.accumulated_map().is_none());
    }

    #[test]
    fn compose_empty_map_means_all_deleted() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // 全部删除 → 空 map
        let empty = OffsetMap {
            entries: Vec::new(),
        };
        chain.compose(&empty, EditorRevision::new(1));

        // 任何 offset 都映射到 None（被删除）
        assert_eq!(chain.map_presented_to_latest(0), None);
        assert_eq!(chain.map_presented_to_latest(5), None);
    }

    #[test]
    fn identity_map_composes_as_identity() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // 纯选区操作，正文不变 → identity map
        let id = identity_map(10);
        chain.compose(&id, EditorRevision::new(1));

        assert_eq!(chain.map_presented_to_latest(5), Some(5));
        assert_eq!(chain.map_latest_to_presented(7), Some(7));
    }

    /// Issue #901 第七轮：中间 revision 的 offset 必须用该 revision → latest
    /// 的后缀映射，不能用 accumulated_map（presented→latest）。
    ///
    /// 反例：presented=`ab`(rev0)，前插 `X` 得 `Xab`(rev1)，再前插 `Y` 得 `YXab`(rev2)。
    /// rev1 中 `X` 的偏移 0 必须映射到 rev2 的偏移 1（X 的正确位置），
    /// 而不是 accumulated_map（rev0→rev2）的偏移 2（那是 `a` 的位置）。
    #[test]
    fn intermediate_revision_uses_suffix_map_not_accumulated() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // 第一笔："ab" → "Xab"：前插 X。
        // rev0 → rev1: a(0)→1, b(1)→2
        let map1 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(1),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(1),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
            ],
        };
        chain.compose(&map1, EditorRevision::new(1));

        // 第二笔："Xab" → "YXab"：前插 Y。
        // rev1 → rev2: X(0)→1, a(1)→2, b(2)→3
        let map2 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(1),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(1),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(2),
                    new_byte_offset: Utf8ByteOffset::unchecked(3),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
            ],
        };
        chain.compose(&map2, EditorRevision::new(2));

        // rev1 中 X 的偏移 0 必须映射到 rev2 的偏移 1（X 的正确位置）。
        // 旧实现误用 accumulated_map（rev0→rev2）：a(0)→2，把 X 错认成 a。
        assert_eq!(
            chain.map_to_latest(EditorRevision::new(1), 0),
            Some(1),
            "intermediate rev1 的 X(0) 必须映射到 rev2 的 1，不是 accumulated_map 的 2"
        );
        // rev1 中 a 的偏移 1 映射到 rev2 的偏移 2
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 1), Some(2));
        // rev1 中 b 的偏移 2 映射到 rev2 的偏移 3
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 2), Some(3));

        // range 映射也必须用 intermediate_maps
        assert_eq!(
            chain.map_range_to_latest(EditorRevision::new(1), 0, 1),
            Some((1, 2)),
            "intermediate rev1 的 X range [0,1) 必须映射到 rev2 的 [1,2)"
        );
        assert_eq!(
            chain.map_range_to_latest(EditorRevision::new(1), 1, 2),
            Some((2, 3)),
        );

        // presented (rev0) 的映射仍然正确：a(0)→2, b(1)→3
        assert_eq!(chain.map_to_latest(rev0, 0), Some(2));
        assert_eq!(chain.map_to_latest(rev0, 1), Some(3));

        // latest (rev2) 的映射是 identity
        assert_eq!(chain.map_to_latest(EditorRevision::new(2), 1), Some(1));
        assert_eq!(
            chain.map_range_to_latest(EditorRevision::new(2), 1, 2),
            Some((1, 2))
        );
    }

    /// Issue #901 第七轮：三笔连续前插，验证每个中间 revision 都有独立正确的后缀映射。
    #[test]
    fn three_edits_each_intermediate_rev_maps_correctly() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // "b" → "Xb" → "YXb" → "ZYXb"：连续三次前插
        // rev0→rev1: b(0)→1
        let map1 = shifted_map(0, 1, 1);
        chain.compose(&map1, EditorRevision::new(1));
        // rev1→rev2: X(0)→1, b(1)→2
        let map2 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(1),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(1),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
            ],
        };
        chain.compose(&map2, EditorRevision::new(2));
        // rev2→rev3: Y(0)→1, X(1)→2, b(2)→3
        let map3 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(1),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(1),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(2),
                    new_byte_offset: Utf8ByteOffset::unchecked(3),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
            ],
        };
        chain.compose(&map3, EditorRevision::new(3));

        // rev1 中 X(0) → rev3 的 2（YXb 中 X 在偏移 2）
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 0), Some(2));
        // rev1 中 b(1) → rev3 的 3
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 1), Some(3));
        // rev2 中 Y(0) → rev3 的 1
        assert_eq!(chain.map_to_latest(EditorRevision::new(2), 0), Some(1));
        // rev2 中 X(1) → rev3 的 2
        assert_eq!(chain.map_to_latest(EditorRevision::new(2), 1), Some(2));
        // rev2 中 b(2) → rev3 的 3
        assert_eq!(chain.map_to_latest(EditorRevision::new(2), 2), Some(3));
        // rev0 中 b(0) → rev3 的 3
        assert_eq!(chain.map_to_latest(rev0, 0), Some(3));
    }

    /// Issue #901 第七轮：reset_to_committed 清空 intermediate_maps，
    /// 旧中间 revision 不再有效。
    #[test]
    fn reset_clears_intermediate_maps() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);
        chain.compose(&shifted_map(1, 1, 1), EditorRevision::new(1));
        chain.compose(&shifted_map(1, 1, 1), EditorRevision::new(2));
        // 有中间 revision rev1，能映射
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 1), Some(1));

        chain.reset_to_committed(EditorRevision::new(2));
        // reset 后 rev1 不再是有效中间 revision
        assert_eq!(
            chain.map_to_latest(EditorRevision::new(1), 1),
            None,
            "reset 后旧中间 revision 应无法映射"
        );
    }

    /// Issue #901 第七轮：advance_revision（纯选区操作）把旧 latest 记为
    /// identity 中间 revision，其 offset 在新 latest 中不变。
    #[test]
    fn advance_revision_records_identity_intermediate() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);
        // 一笔编辑产生 rev1
        chain.compose(&shifted_map(1, 1, 1), EditorRevision::new(1));
        // 纯选区操作推进到 rev2，正文不变
        chain.advance_revision(EditorRevision::new(2));

        // rev1 是中间 revision，正文没变（advance 是 identity），offset 不变
        assert_eq!(
            chain.map_to_latest(EditorRevision::new(1), 1),
            Some(1),
            "advance_revision 后旧 latest 的 offset 应 identity 映射"
        );
        assert_eq!(
            chain.map_range_to_latest(EditorRevision::new(1), 1, 2),
            Some((1, 2)),
        );
        // rev0（presented）仍用 accumulated_map
        assert_eq!(chain.map_to_latest(rev0, 1), Some(1));
    }

    /// Issue #901 第七轮：中间 revision 的字符被后续编辑删除时返回 None。
    #[test]
    fn intermediate_revision_deleted_chars_return_none() {
        let rev0 = EditorRevision::new(0);
        let mut chain = RevisionChain::new(rev0);

        // "ab" → "Xab"：前插 X（rev0→rev1）
        let map1 = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(0),
                    new_byte_offset: Utf8ByteOffset::unchecked(1),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
                OffsetMapEntry {
                    old_byte_offset: Utf8ByteOffset::unchecked(1),
                    new_byte_offset: Utf8ByteOffset::unchecked(2),
                    length: 1,
                    kind: writer_core::editor::OffsetMapKind::Shifted,
                },
            ],
        };
        chain.compose(&map1, EditorRevision::new(1));

        // "Xab" → "b"：删除 X 和 a，只保留 b（rev1→rev2）
        // rev1 中 b 在偏移 2，rev2 中 b 在偏移 0
        let map2 = OffsetMap {
            entries: vec![OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(2),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 1,
                kind: writer_core::editor::OffsetMapKind::Shifted,
            }],
        };
        chain.compose(&map2, EditorRevision::new(2));

        // rev1 中 X(0) 被删除 → None
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 0), None);
        // rev1 中 a(1) 被删除 → None
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 1), None);
        // rev1 中 b(2) 存活 → rev2 的 0
        assert_eq!(chain.map_to_latest(EditorRevision::new(1), 2), Some(0));
    }
}
