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

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
