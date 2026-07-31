#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::project::Project> for ProjectDto {
    fn from(p: crate::project::Project) -> Self {
        Self {
            id: p.id,
            title: p.title,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatsDto {
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

impl From<crate::project::ProjectStats> for ProjectStatsDto {
    fn from(s: crate::project::ProjectStats) -> Self {
        Self {
            total_word_count: s.total_word_count,
            volume_count: s.volume_count,
            chapter_count: s.chapter_count,
        }
    }
}
