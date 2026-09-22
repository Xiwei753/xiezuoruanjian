pub(crate) use super::layout_snapshot::LineSnapshotId;

use qmetaobject::QImage;
use std::collections::HashMap;

pub(crate) struct LineTextureStore {
    textures: HashMap<LineSnapshotId, QImage>,
}

impl LineTextureStore {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: LineSnapshotId, texture: QImage) {
        self.textures.insert(id, texture);
    }

    pub fn get(&self, id: &LineSnapshotId) -> Option<&QImage> {
        self.textures.get(id)
    }

    pub fn contains(&self, id: &LineSnapshotId) -> bool {
        self.textures.contains_key(id)
    }

    pub fn remove(&mut self, id: &LineSnapshotId) {
        self.textures.remove(id);
    }

    /// Issue #736 评论 5786231506: 只保留在给定 active ids 集合里的纹理。
    /// 事务完成/cancel/rebase 后，用当前所有 active transaction 实际还引用的
    /// LineSnapshotId 集合做 retain，不再让 remove_for_transaction 无条件删共享
    /// snapshot，避免误删相邻事务还在用的纹理。
    pub fn retain(&mut self, active_ids: &[LineSnapshotId]) {
        let active_set: std::collections::HashSet<&LineSnapshotId> = active_ids.iter().collect();
        self.textures.retain(|id, _| active_set.contains(id));
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
