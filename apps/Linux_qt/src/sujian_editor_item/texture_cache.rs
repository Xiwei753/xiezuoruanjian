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

    /// Issue #736 评论 5786231506: 不再推荐使用 `remove_for_transaction`。
    /// 它会无条件删除传入的 snapshot 纹理，可能误删相邻事务还在用的共享 snapshot。
    /// 改用 [`retain_active_snapshot_ids`]，按当前所有 active transaction 实际还
    /// 引用的 LineSnapshotId 集合做 retain，只释放已经没有任何 active transaction
    /// 引用的纹理。保留本方法仅为向后兼容，新代码不应再调用。
    #[deprecated(note = "Issue #736 评论 5786231506: 改用 retain_active_snapshot_ids")]
    pub fn remove_for_transaction(&mut self, _snapshot_ids: &[LineSnapshotId]) {
        for id in _snapshot_ids {
            self.line_store.remove(id);
        }
    }

    /// Issue #736 评论 5786231506: 按当前所有 active transaction 实际还引用的
    /// LineSnapshotId 来管理纹理生命周期。只保留在 active ids 里的纹理，
    /// 删除不在的。不再让 remove_for_transaction 无条件删共享 snapshot。
    pub fn retain_active_snapshot_ids(&mut self, active_snapshot_ids: &[LineSnapshotId]) {
        self.line_store.retain(active_snapshot_ids);
    }

    pub fn clear(&mut self) {
        self.line_store.clear();
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
