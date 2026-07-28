use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::relation_index::{EdgeRelationIndex, EmbedHostIndex, LinkRelationIndex, HyperlinkRelationIndex};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMeta {
    pub schema_version: String,
    pub starmap_id: String,
    pub title: String,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub embed_instance_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub hyperlink_ids: Vec<String>,
    #[serde(default)]
    pub edge_relation_index: Vec<EdgeRelationIndex>,
    #[serde(default)]
    pub embed_host_index: Vec<EmbedHostIndex>,
    #[serde(default)]
    pub link_relation_index: Vec<LinkRelationIndex>,
    #[serde(default)]
    pub hyperlink_relation_index: Vec<HyperlinkRelationIndex>,
    #[serde(default)]
    pub node_kind_counts: HashMap<String, u32>,
    pub package_revision: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub deleted_since_last_sync: DeletedSinceLastSync,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedSinceLastSync {
    #[serde(default)]
    pub entries: Vec<DeletionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionEntry {
    pub object_type: String,
    pub object_id: String,
    pub deleted_at_revision: u64,
}

impl Default for DeletedSinceLastSync {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl DeletedSinceLastSync {
    pub fn add_entry(&mut self, object_type: &str, object_id: &str, revision: u64) {
        self.entries.push(DeletionEntry {
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            deleted_at_revision: revision,
        });
    }

    pub fn remove_entry(&mut self, object_type: &str, object_id: &str) {
        self.entries.retain(|e| !(e.object_type == object_type && e.object_id == object_id));
    }

    pub fn entries_since(&self, since_revision: u64) -> impl Iterator<Item = &DeletionEntry> {
        self.entries.iter().filter(move |e| e.deleted_at_revision > since_revision)
    }

    pub fn compact(&mut self, keep_since_revision: u64) {
        self.entries.retain(|e| e.deleted_at_revision >= keep_since_revision);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LegacyGraphMeta {
    pub(super) schema_version: u32,
    pub(super) id: String,
    pub(super) starmap_id: String,
    pub(super) title: String,
    pub(super) created_at: u64,
    pub(super) updated_at: u64,
}
