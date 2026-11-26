pub(crate) use super::layout_snapshot::LineSnapshotId;

pub(crate) use crate::editor::layout::texture_image::{
    publish_cache_metrics, set_active_visual_snapshots, texture_memory_metrics, IntoLineImage,
    LineImage, TextureMemoryMetrics,
};
use qmetaobject::QImage;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Issue #1061 第 2 项：行纹理条目的顺序与共享图元数据。
struct LineTextureEntry {
    texture: Arc<LineImage>,
    /// 插入顺序，越小越老。
    sequence: u64,
}

pub(crate) struct LineTextureStore {
    textures: HashMap<LineSnapshotId, LineTextureEntry>,
    byte_len: usize,
    next_sequence: u64,
    /// 按插入顺序维护的候选淘汰队列；条目的 sequence 与映射不一致时即为
    /// 覆盖/删除后留下的旧记录，跳过即可，不再每次全表找最老条目。
    evict_queue: VecDeque<(LineSnapshotId, u64)>,
}

impl LineTextureStore {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            byte_len: 0,
            next_sequence: 0,
            evict_queue: VecDeque::new(),
        }
    }

    /// 覆盖同一 id 时先扣除旧条目的字节数再加入新图。
    pub fn insert(&mut self, id: LineSnapshotId, texture: Arc<LineImage>) {
        if let Some(previous) = self.textures.remove(&id) {
            self.byte_len = self.byte_len.saturating_sub(previous.texture.bytes());
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.byte_len = self.byte_len.saturating_add(texture.bytes());
        self.evict_queue.push_back((id, sequence));
        self.textures
            .insert(id, LineTextureEntry { texture, sequence });
    }

    pub fn get(&self, id: &LineSnapshotId) -> Option<&QImage> {
        self.textures.get(id).map(|entry| entry.texture.texture())
    }

    pub fn contains(&self, id: &LineSnapshotId) -> bool {
        self.textures.contains_key(id)
    }

    /// 只保留在给定 active ids 集合里的纹理。
    ///
    /// 事务完成/cancel/rebase 后，用当前所有 active transaction 实际还引用的
    /// LineSnapshotId 集合做 retain，不误删相邻事务还在用的纹理。
    pub fn retain(&mut self, active_ids: &[LineSnapshotId]) {
        let active_set: std::collections::HashSet<&LineSnapshotId> = active_ids.iter().collect();
        let mut removed_bytes = 0usize;
        self.textures.retain(|id, entry| {
            let keep = active_set.contains(id);
            if !keep {
                removed_bytes = removed_bytes.saturating_add(entry.texture.bytes());
            }
            keep
        });
        self.byte_len = self.byte_len.saturating_sub(removed_bytes);
        self.evict_queue
            .retain(|(id, _)| self.textures.contains_key(id));
        self.publish_metrics();
    }

    /// 当前缓存的行纹理估算总字节数（含共享条目）。
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// 缓存中仅由缓存自身持有、可安全淘汰的字节数。
    pub fn cache_only_bytes(&self) -> usize {
        self.textures
            .values()
            .filter(|entry| Arc::strong_count(&entry.texture) == 1)
            .fold(0usize, |sum, entry| {
                sum.saturating_add(entry.texture.bytes())
            })
    }

    /// 被快照/动画共享、当前不可淘汰的缓存条目字节数。
    pub fn shared_bytes(&self) -> usize {
        self.byte_len.saturating_sub(self.cache_only_bytes())
    }

    /// 当前缓存的行纹理条数。
    pub fn line_count(&self) -> usize {
        self.textures.len()
    }

    pub fn publish_metrics(&self) {
        let cache_only = self.cache_only_bytes();
        publish_cache_metrics(
            cache_only,
            self.byte_len.saturating_sub(cache_only),
            self.textures.len(),
        );
    }

    /// 超过可淘汰字节预算时按插入顺序淘汰最老的、未被快照共享的条目。
    ///
    /// 被快照共享的图不能在这里删掉，否则会把仍要绘制的行图抽走；它们会在
    /// 快照释放后自动变成 `cache_only_bytes`，下一次扫描再回收。淘汰队列的
    /// 成本随实际扫描到的候选条目增长，不再每淘汰一条就全表找最小值。
    pub fn enforce_byte_budget(&mut self, max_bytes: usize) -> usize {
        let mut cache_only = self.cache_only_bytes();
        if cache_only <= max_bytes {
            self.publish_metrics();
            return 0;
        }
        let candidates = std::mem::take(&mut self.evict_queue);
        let mut iter = candidates.into_iter();
        let mut kept: VecDeque<(LineSnapshotId, u64)> = VecDeque::new();
        let mut evicted = 0usize;
        while cache_only > max_bytes {
            let Some((id, sequence)) = iter.next() else {
                break;
            };
            let Some(entry) = self.textures.get(&id) else {
                continue;
            };
            if entry.sequence != sequence {
                // 同一 id 已被覆盖，旧队列记录失效。
                continue;
            }
            if Arc::strong_count(&entry.texture) > 1 {
                kept.push_back((id, sequence));
                continue;
            }
            let Some(entry) = self.textures.remove(&id) else {
                continue;
            };
            cache_only = cache_only.saturating_sub(entry.texture.bytes());
            self.byte_len = self.byte_len.saturating_sub(entry.texture.bytes());
            evicted += 1;
        }
        kept.extend(iter);
        self.evict_queue = kept;
        self.publish_metrics();
        evicted
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
