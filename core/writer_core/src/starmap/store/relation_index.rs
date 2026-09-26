use serde::{Deserialize, Serialize};

use crate::starmap::types::reference::StarMapTargetPath;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRelationIndex {
    pub edge_id: String,
    pub from: StarMapTargetPath,
    pub to: StarMapTargetPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedHostIndex {
    pub instance_id: String,
    pub host_path: StarMapTargetPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRelationIndex {
    pub link_id: String,
    pub source_node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperlinkRelationIndex {
    pub hyperlink_id: String,
    pub source_node_id: String,
}

pub(super) fn target_path_node_id(path: &StarMapTargetPath) -> Option<&str> {
    match &path.target {
        crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id),
        crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id),
        _ => None,
    }
}

pub(super) fn extract_eri_node_refs(eri: &EdgeRelationIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if let Some(id) = target_path_node_id(&eri.from) {
        refs.push(id);
    }
    if let Some(id) = target_path_node_id(&eri.to) {
        refs.push(id);
    }
    refs
}

pub(super) fn extract_ehi_node_refs(ehi: &EmbedHostIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if let Some(id) = target_path_node_id(&ehi.host_path) {
        refs.push(id);
    }
    refs
}
