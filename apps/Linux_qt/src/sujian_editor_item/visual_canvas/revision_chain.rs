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

use writer_core::editor::{EditorRevision, OffsetMap};

/// 从 `presented_revision` 到 `latest_revision` 的累计 offset map 组合。
///
/// `accumulated_map` 为 `None` 表示 presented_revision == latest_revision
/// （identity，无需映射）。每次 `compose` 用 `OffsetMap::compose` 精确组合，
/// 保留多 patch 中间的 unchanged islands。
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
}

impl RevisionChain {
    /// 创建一条新的 revision chain，初始时 presented == latest == given revision。
    pub(crate) fn new(presented_revision: EditorRevision) -> Self {
        Self {
            presented_revision,
            latest_revision: presented_revision,
            accumulated_map: None,
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
        if next_offset_map.entries.is_empty() {
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

    /// 新帧已提交：把 presented_revision 前进到该帧的 revision，累计映射归零。
    ///
    /// 调用时机：Qt `afterFrameEnd` 确认了一帧，且该帧的 `source_revision`
    /// 等于当前 `latest_revision`（即画面已追上 Core）。
    pub(crate) fn reset_to_committed(&mut self, committed_revision: EditorRevision) {
        self.presented_revision = committed_revision;
        self.latest_revision = committed_revision;
        self.accumulated_map = None;
    }

    /// 推进 latest revision 而不改变累计映射。
    ///
    /// 用于纯选区/光标操作（正文不变、`offset_map` 为 `None`）：
    /// revision 前进了但字符身份映射仍然有效，不需要 compose。
    pub(crate) fn advance_revision(&mut self, new_revision: EditorRevision) {
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
}
