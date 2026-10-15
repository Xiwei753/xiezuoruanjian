pub(crate) use super::layout_snapshot::LineSnapshotId;
use super::line_snapshot::LineTextureStore;

pub(crate) struct TextureCache {
    line_store: LineTextureStore,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            line_store: LineTextureStore::new(),
        }
    }

    pub fn insert_line(&mut self, id: LineSnapshotId, texture: qmetaobject::QImage) {
        self.line_store.insert(id, texture);
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
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
