use std::collections::HashMap;

use super::transaction_key::VisualTransactionKey;
use super::line_snapshot::LineSnapshotId;
use qmetaobject::QImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TexturePhase {
    Insert,
    DeleteOld,
    OldReflow,
    NewReflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextureCacheKey {
    pub transaction_key: VisualTransactionKey,
    pub phase: TexturePhase,
    pub run_identity: i32,
}

impl TextureCacheKey {
    pub fn new(transaction_key: VisualTransactionKey, phase: TexturePhase, run_identity: i32) -> Self {
        Self { transaction_key, phase, run_identity }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineTextureCacheKey {
    pub line_snapshot_id: LineSnapshotId,
    pub phase: TexturePhase,
    pub run_identity: i32,
}

impl LineTextureCacheKey {
    pub fn new(line_snapshot_id: LineSnapshotId, phase: TexturePhase, run_identity: i32) -> Self {
        Self { line_snapshot_id, phase, run_identity }
    }
}

pub(crate) struct TextureCache {
    textures: HashMap<TextureCacheKey, QImage>,
    line_textures: HashMap<LineTextureCacheKey, QImage>,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            line_textures: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: TextureCacheKey, texture: QImage) {
        self.textures.insert(key, texture);
    }

    pub fn insert_line(&mut self, key: LineTextureCacheKey, texture: QImage) {
        self.line_textures.insert(key, texture);
    }

    pub fn insert_batch(&mut self, keys: Vec<TextureCacheKey>, textures: Vec<QImage>) {
        for (k, t) in keys.into_iter().zip(textures.into_iter()) {
            self.textures.insert(k, t);
        }
    }

    pub fn get(&self, key: &TextureCacheKey) -> Option<&QImage> {
        self.textures.get(key)
    }

    pub fn get_line(&self, key: &LineTextureCacheKey) -> Option<&QImage> {
        self.line_textures.get(key)
    }

    pub fn remove_for_transaction(&mut self, transaction_key: &VisualTransactionKey) {
        self.textures.retain(|k, _| k.transaction_key != *transaction_key);
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        self.line_textures.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty() && self.line_textures.is_empty()
    }
}
