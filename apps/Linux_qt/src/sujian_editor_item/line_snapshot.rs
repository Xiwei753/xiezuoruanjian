pub(crate) use super::layout_snapshot::LineSnapshotId;
pub(crate) use super::layout_revision::LayoutRevision;

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

    pub fn remove_revision(&mut self, revision: LayoutRevision) {
        self.textures.retain(|id, _| id.layout_revision != revision.0);
    }

    pub fn remove(&mut self, id: &LineSnapshotId) {
        self.textures.remove(id);
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
