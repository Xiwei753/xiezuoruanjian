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

#[test]
fn test_project_dto_serialization_roundtrip() {
    let dto = ProjectDto {
        id: "proj_1".to_string(),
        title: "Test Project".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
    };
    let json_str = serde_json::to_string(&dto).unwrap();
    let deserialized: ProjectDto = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_project_stats_dto_serialization_roundtrip() {
    let dto = ProjectStatsDto {
        total_word_count: 1000,
        volume_count: 5,
        chapter_count: 50,
    };
    let json_str = serde_json::to_string(&dto).unwrap();
    let deserialized: ProjectStatsDto = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized, dto);
}
