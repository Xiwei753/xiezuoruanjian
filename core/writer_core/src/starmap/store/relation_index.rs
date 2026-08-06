use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRelationIndex {
    pub edge_id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub from_endpoint: Option<crate::starmap::types::StarMapEdgeEndpoint>,
    #[serde(default)]
    pub to_endpoint: Option<crate::starmap::types::StarMapEdgeEndpoint>,
    #[serde(default)]
    pub from_endpoint_path: Option<crate::starmap::types::StarMapEndpointPath>,
    #[serde(default)]
    pub to_endpoint_path: Option<crate::starmap::types::StarMapEndpointPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedHostIndex {
    pub instance_id: String,
    pub host_node_id: String,
    #[serde(default)]
    pub host_endpoint: Option<crate::starmap::types::StarMapEndpoint>,
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

pub(super) fn endpoint_node_id(endpoint: &crate::starmap::types::StarMapEndpoint) -> Option<&str> {
    match endpoint {
        crate::starmap::types::StarMapEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEndpoint::Starmap => None,
    }
}

pub(super) fn endpoint_path_node_id(
    path: &crate::starmap::types::StarMapEndpointPath,
) -> Option<&str> {
    match &path.endpoint {
        crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Starmap => None,
        crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { .. } => None,
    }
}

pub(super) fn edge_endpoint_node_id(
    ep: &crate::starmap::types::StarMapEdgeEndpoint,
) -> Option<&str> {
    match ep {
        crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Starmap => None,
        crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { .. } => None,
    }
}

pub(super) fn extract_eri_node_refs(eri: &EdgeRelationIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if !eri.from.is_empty() {
        refs.push(eri.from.as_str());
    }
    if !eri.to.is_empty() {
        refs.push(eri.to.as_str());
    }
    if let Some(ref ep) = eri.from_endpoint {
        if let Some(id) = edge_endpoint_node_id(ep) {
            refs.push(id);
        }
    }
    if let Some(ref ep) = eri.to_endpoint {
        if let Some(id) = edge_endpoint_node_id(ep) {
            refs.push(id);
        }
    }
    if let Some(ref path) = eri.from_endpoint_path {
        if let Some(id) = endpoint_path_node_id(path) {
            refs.push(id);
        }
    }
    if let Some(ref path) = eri.to_endpoint_path {
        if let Some(id) = endpoint_path_node_id(path) {
            refs.push(id);
        }
    }
    refs
}

pub(super) fn extract_ehi_node_refs(ehi: &EmbedHostIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if !ehi.host_node_id.is_empty() {
        refs.push(ehi.host_node_id.as_str());
    }
    if let Some(ref ep) = ehi.host_endpoint {
        if let Some(id) = endpoint_node_id(ep) {
            refs.push(id);
        }
    }
    refs
}
