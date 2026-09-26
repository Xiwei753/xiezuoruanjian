use serde::{Deserialize, Serialize};

use crate::starmap::types::reference::StarMapTargetPath;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLink {
    pub link_id: String,
    pub source: StarMapTargetPath,
    pub target: StarMapTargetPath,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatch {
    pub source: Option<StarMapTargetPath>,
    pub target: Option<StarMapTargetPath>,
    pub label: Option<Option<String>>,
}
