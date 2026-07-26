use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    ChapterBody,
    ChapterTitle,
    ChapterNote,
    ProjectTitle,
    VolumeTitle,
    StarmapTitle,
    StarmapNode,
    StarmapEdgeLabel,
    StarmapHyperlink,
    Setting,
    All,
}

impl SearchScope {
    pub fn all_scopes() -> &'static [SearchScope] {
        &[
            SearchScope::ChapterBody,
            SearchScope::ChapterTitle,
            SearchScope::ChapterNote,
            SearchScope::ProjectTitle,
            SearchScope::VolumeTitle,
            SearchScope::StarmapTitle,
            SearchScope::StarmapNode,
            SearchScope::StarmapEdgeLabel,
            SearchScope::StarmapHyperlink,
            SearchScope::Setting,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub title: String,
    pub path: String,
    pub summary: String,
    pub match_ranges: Vec<(usize, usize)>,
    pub score: f64,
    pub scope: SearchScope,
    pub target: SearchTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTarget {
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub starmap_id: Option<String>,
    pub node_id: Option<String>,
    pub setting_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    pub scope: SearchScope,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            scope: SearchScope::All,
            limit: 50,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStatus {
    pub total_entries: usize,
    pub scope_counts: Vec<(SearchScope, usize)>,
    pub last_rebuild_at: u64,
    pub is_rebuilding: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexUpdate {
    pub object_id: String,
    pub scope: SearchScope,
    pub title: String,
    pub body: String,
    pub target: SearchTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub object_id: String,
    pub scope: SearchScope,
    pub title: String,
    pub body: String,
    pub target: SearchTarget,
}
