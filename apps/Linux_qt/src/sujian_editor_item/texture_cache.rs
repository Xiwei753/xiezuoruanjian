use super::line_snapshot::LineTextureStore;
pub(crate) use super::layout_snapshot::LineSnapshotId;

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

    pub fn remove_for_transaction(&mut self, _snapshot_ids: &[LineSnapshotId]) {
        for id in _snapshot_ids {
            self.line_store.remove(id);
        }
    }

    pub fn clear(&mut self) {
        self.line_store.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.line_store.is_empty()
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}
