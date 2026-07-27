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
