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

/// 项目摘要 DTO — 元数据 + 统计一次性返回（#625 第二段）。
///
/// 作品卡片显示字数需要在列表时一次拿到所有项目的 summary，
/// 避免端侧逐卡跨 FFI 调 `get_project_stats`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub total_word_count: u32,
    pub volume_count: u32,
    pub chapter_count: u32,
}

impl From<crate::project::ProjectSummary> for ProjectSummaryDto {
    fn from(s: crate::project::ProjectSummary) -> Self {
        Self {
            id: s.id,
            title: s.title,
            created_at: s.created_at,
            updated_at: s.updated_at,
            total_word_count: s.total_word_count,
            volume_count: s.volume_count,
            chapter_count: s.chapter_count,
        }
    }
}
