pub(crate) use super::layout_snapshot::LineSnapshotId;

use qmetaobject::QImage;
use std::collections::HashMap;

/// Issue #1061 第 2 项：行纹理条目的顺序与字节元数据。
struct LineTextureEntry {
    texture: QImage,
    /// ARGB32_Premultiplied 估算字节数（宽 × 高 × 4）。
    bytes: usize,
    /// 插入顺序，越小越老。
    sequence: u64,
}

/// 按 ARGB32_Premultiplied 估算一张行纹理的字节数。
fn estimate_texture_bytes(texture: &QImage) -> usize {
    let size = texture.size();
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(4)
}

pub(crate) struct LineTextureStore {
    textures: HashMap<LineSnapshotId, LineTextureEntry>,
    byte_len: usize,
    next_sequence: u64,
}

impl LineTextureStore {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            byte_len: 0,
            next_sequence: 0,
        }
    }

    /// Issue #1061 第 2 项：覆盖同一 id 时先扣除旧条目的字节数再加入新图。
    pub fn insert(&mut self, id: LineSnapshotId, texture: QImage) {
        let bytes = estimate_texture_bytes(&texture);
        if let Some(previous) = self.textures.remove(&id) {
            self.byte_len = self.byte_len.saturating_sub(previous.bytes);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.byte_len = self.byte_len.saturating_add(bytes);
        self.textures.insert(
            id,
            LineTextureEntry {
                texture,
                bytes,
                sequence,
            },
        );
    }

    pub fn get(&self, id: &LineSnapshotId) -> Option<&QImage> {
        self.textures.get(id).map(|entry| &entry.texture)
    }

    pub fn contains(&self, id: &LineSnapshotId) -> bool {
        self.textures.contains_key(id)
    }

    /// Issue #736 评论 5786231506: 只保留在给定 active ids 集合里的纹理。
    /// 事务完成/cancel/rebase 后，用当前所有 active transaction 实际还引用的
    /// LineSnapshotId 集合做 retain，不再让 remove_for_transaction 无条件删共享
    /// snapshot，避免误删相邻事务还在用的纹理。
    ///
    /// Issue #1061 第 2 项：删除条目时同步从字节统计中扣减。
    pub fn retain(&mut self, active_ids: &[LineSnapshotId]) {
        let active_set: std::collections::HashSet<&LineSnapshotId> = active_ids.iter().collect();
        let mut removed_bytes = 0usize;
        self.textures.retain(|id, entry| {
            let keep = active_set.contains(id);
            if !keep {
                removed_bytes = removed_bytes.saturating_add(entry.bytes);
            }
            keep
        });
        self.byte_len = self.byte_len.saturating_sub(removed_bytes);
    }

    /// Issue #1061 第 2 项：当前缓存的行纹理估算总字节数。
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Issue #1061 第 2 项：当前缓存的行纹理条数。
    pub fn line_count(&self) -> usize {
        self.textures.len()
    }

    /// Issue #1061 第 2 项：超过字节预算时按插入顺序淘汰最老的快照纹理。
    ///
    /// 被删掉的图仍保留在各自 `EditorLayoutSnapshot.line_snapshots[].image` 中，
    /// 下一次 `prepare_local_text_textures` 会从快照重新插入；因此这里允许淘汰，
    /// 但不伪造空图。每轮只找 `sequence` 最小的一条，不做全量排序。
    ///
    /// 返回淘汰条数。
    pub fn enforce_byte_budget(&mut self, max_bytes: usize) -> usize {
        let mut evicted = 0usize;
        while self.byte_len > max_bytes {
            let Some(oldest_id) = self
                .textures
                .iter()
                .min_by_key(|(_, entry)| entry.sequence)
                .map(|(id, _)| *id)
            else {
                break;
            };
            let Some(entry) = self.textures.remove(&oldest_id) else {
                break;
            };
            self.byte_len = self.byte_len.saturating_sub(entry.bytes);
            evicted += 1;
        }
        evicted
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
