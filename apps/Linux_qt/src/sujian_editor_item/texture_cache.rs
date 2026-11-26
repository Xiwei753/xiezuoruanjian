pub(crate) use super::layout_snapshot::LineSnapshotId;
use super::line_snapshot::{IntoLineImage, LineTextureStore, TextureMemoryMetrics};

pub(crate) struct TextureCache {
    line_store: LineTextureStore,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            line_store: LineTextureStore::new(),
        }
    }

    pub fn insert_line<T: IntoLineImage>(&mut self, id: LineSnapshotId, texture: T) {
        self.line_store.insert(id, texture.into_line_image());
    }

    pub fn get_line(&self, id: &LineSnapshotId) -> Option<&qmetaobject::QImage> {
        self.line_store.get(id)
    }

    pub fn contains_line(&self, id: &LineSnapshotId) -> bool {
        self.line_store.contains(id)
    }

    /// Issue #904：按当前所有仍被引用的 LineSnapshotId 管理局部纹理生命周期。
    /// 只保留在 active ids 里的纹理，删除不在的。局部动画源纹理由
    /// `TextMotion::source_snapshot_ids()` 与 QSG 帧回执共同纳入 active ids。
    pub fn retain_active_snapshot_ids(&mut self, active_snapshot_ids: &[LineSnapshotId]) {
        self.line_store.retain(active_snapshot_ids);
    }

    /// Issue #1061 第 2 项：当前缓存的行纹理估算总字节数。
    pub fn byte_len(&self) -> usize {
        self.line_store.byte_len()
    }

    /// Issue #1061 复审：仅缓存持有、可安全淘汰的字节数。
    pub fn cache_only_bytes(&self) -> usize {
        self.line_store.cache_only_bytes()
    }

    /// Issue #1061 复审：仍被快照共享的缓存字节数。
    pub fn shared_bytes(&self) -> usize {
        self.line_store.shared_bytes()
    }

    /// Issue #1061 第 2 项：当前缓存的行纹理条数。
    pub fn line_count(&self) -> usize {
        self.line_store.line_count()
    }

    pub fn metrics(&self) -> TextureMemoryMetrics {
        TextureMemoryMetrics {
            cache_only_bytes: self.cache_only_bytes(),
            shared_bytes: self.shared_bytes(),
            live_bytes: super::line_snapshot::texture_memory_metrics().live_bytes,
            lines: self.line_count(),
            active_visual_snapshots: super::line_snapshot::texture_memory_metrics()
                .active_visual_snapshots,
        }
    }

    pub fn publish_metrics(&self) {
        self.line_store.publish_metrics();
    }

    /// Issue #1061 第 2 项：超过字节预算时按插入顺序淘汰最老的行纹理。
    ///
    /// 返回淘汰条数；被淘汰的图仍可从布局快照重新插入。
    pub fn enforce_byte_budget(&mut self, max_bytes: usize) -> usize {
        self.line_store.enforce_byte_budget(max_bytes)
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
