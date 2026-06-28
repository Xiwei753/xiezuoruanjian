use serde::{Deserialize, Serialize};

use super::embed::StarMapEndpoint;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLink {
    pub link_id: String,
    pub source: StarMapEndpoint,
    pub target: crate::starmap::semantic::StarMapDeepTarget,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatch {
    pub source: Option<StarMapEndpoint>,
    pub target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    pub label: Option<Option<String>>,
}
