#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentEditDto {
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub timestamp: String,
}

impl From<crate::recent_edits::RecentEdit> for RecentEditDto {
    fn from(r: crate::recent_edits::RecentEdit) -> Self {
        Self {
            project_id: r.project_id,
            volume_id: r.volume_id,
            chapter_id: r.chapter_id,
            timestamp: r.timestamp,
        }
    }
}
