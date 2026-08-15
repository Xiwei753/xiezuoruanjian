use super::project::*;
use crate::project::{Project, ProjectStats, ProjectSummary};
use serde_json::json;

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
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "proj_1",
            "title": "Test Project",
            "createdAt": "2023-01-01T00:00:00Z",
            "updatedAt": "2023-01-02T00:00:00Z"
        })
    );
    let deserialized: ProjectDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_project_stats_dto_serialization_roundtrip() {
    let dto = ProjectStatsDto {
        total_word_count: 1000,
        volume_count: 5,
        chapter_count: 50,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "totalWordCount": 1000,
            "volumeCount": 5,
            "chapterCount": 50
        })
    );
    let deserialized: ProjectStatsDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_project_summary_dto_from_project_summary() {
    // #625 第二段：ProjectSummary → ProjectSummaryDto 字段映射。
    let summary = ProjectSummary {
        id: "proj-1".to_string(),
        title: "Test Project".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        total_word_count: 10000,
        volume_count: 5,
        chapter_count: 20,
    };
    let dto = ProjectSummaryDto::from(summary);
    assert_eq!(dto.id, "proj-1");
    assert_eq!(dto.title, "Test Project");
    assert_eq!(dto.created_at, "2023-01-01T00:00:00Z");
    assert_eq!(dto.updated_at, "2023-01-02T00:00:00Z");
    assert_eq!(dto.total_word_count, 10000);
    assert_eq!(dto.volume_count, 5);
    assert_eq!(dto.chapter_count, 20);
}

#[test]
fn test_project_summary_dto_serialization_roundtrip() {
    // #625 第二段：ProjectSummaryDto camelCase 序列化契约。
    let dto = ProjectSummaryDto {
        id: "proj_1".to_string(),
        title: "Test Project".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        total_word_count: 1000,
        volume_count: 5,
        chapter_count: 50,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "proj_1",
            "title": "Test Project",
            "createdAt": "2023-01-01T00:00:00Z",
            "updatedAt": "2023-01-02T00:00:00Z",
            "totalWordCount": 1000,
            "volumeCount": 5,
            "chapterCount": 50
        })
    );
    let deserialized: ProjectSummaryDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}
