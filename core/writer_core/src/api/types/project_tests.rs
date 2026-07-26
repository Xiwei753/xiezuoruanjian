use super::project::*;
use crate::project::{Project, ProjectStats};

#[test]
fn test_project_dto_from_project() {
    let project = Project {
        id: "proj-1".to_string(),
        title: "Test Project".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        order: 1,
    };
    let dto = ProjectDto::from(project);
    assert_eq!(dto.id, "proj-1");
    assert_eq!(dto.title, "Test Project");
    assert_eq!(dto.created_at, "2023-01-01T00:00:00Z");
    assert_eq!(dto.updated_at, "2023-01-02T00:00:00Z");
}

#[test]
fn test_project_stats_dto_from_project_stats() {
    let stats = ProjectStats {
        total_word_count: 10000,
        volume_count: 5,
        chapter_count: 20,
    };
    let dto = ProjectStatsDto::from(stats);
    assert_eq!(dto.total_word_count, 10000);
    assert_eq!(dto.volume_count, 5);
    assert_eq!(dto.chapter_count, 20);
}
