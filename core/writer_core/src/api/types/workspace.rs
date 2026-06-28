use super::*;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RecentEditDto {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

impl From<crate::workspace::RecentEdit> for RecentEditDto {
    fn from(r: crate::workspace::RecentEdit) -> Self {
        Self {
            project_id: r.project_id,
            volume_id: r.volume_id,
            chapter_id: r.chapter_id,
            timestamp: r.timestamp,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct WorkspaceSummaryDto {
    pub path: String,
    pub is_valid: bool,
    pub projects: Vec<ProjectDto>,
    pub recent_edits: Vec<RecentEditDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiagnosticsDto {
    pub has_workspace: bool,
    pub workspace_path: String,
    pub core_initialized: bool,
    pub path_exists: bool,
    pub is_dir: bool,
    pub manifest_path: String,
    pub manifest_exists: bool,
    pub projects_path: String,
    pub projects_dir_exists: bool,
    pub app_meta_exists: bool,
    pub writable: bool,
    pub writable_error: String,
    pub validate_workspace: bool,
    pub tree_count: u64,
    pub last_workspace_path: String,
    pub create_project_available: bool,
}
