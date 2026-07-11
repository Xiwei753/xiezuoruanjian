use std::collections::HashMap;

use super::transaction_key::VisualTransactionKey;
use super::insert_animation::GlyphFrameData;
use crate::sujian_editor_item::editor_animation_debug_log;
use qmetaobject::QImage;

pub(crate) struct TextureCache {
    textures: HashMap<VisualTransactionKey, Vec<QImage>>,
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: VisualTransactionKey, textures: Vec<QImage>) {
        self.textures.insert(key, textures);
    }

    pub fn get(&self, key: &VisualTransactionKey) -> Option<&Vec<QImage>> {
        self.textures.get(key)
    }

    pub fn remove(&mut self, key: &VisualTransactionKey) -> Option<Vec<QImage>> {
        self.textures.remove(key)
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }

    pub fn keys(&self) -> Vec<VisualTransactionKey> {
        self.textures.keys().copied().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}
