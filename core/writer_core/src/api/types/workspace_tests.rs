use super::workspace::*;
use crate::recent_edits::RecentEdit;
use serde_json::json;

#[test]
fn test_recent_edit_dto_conversion() {
    let edit = RecentEdit {
        project_id: "proj_123".to_string(),
        volume_id: "vol_456".to_string(),
        chapter_id: "chap_789".to_string(),
        timestamp: "2023-10-01T12:00:00Z".to_string(),
    };
    let dto: RecentEditDto = edit.clone().into();
    assert_eq!(dto.project_id, edit.project_id);
    assert_eq!(dto.volume_id, edit.volume_id);
    assert_eq!(dto.chapter_id, edit.chapter_id);
    assert_eq!(dto.timestamp, edit.timestamp);
}

#[test]
fn test_recent_edit_dto_serialization_roundtrip() {
    let dto = RecentEditDto {
        project_id: "p1".to_string(),
        volume_id: "v1".to_string(),
        chapter_id: "c1".to_string(),
        timestamp: "2023-10-10".to_string(),
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "projectId": "p1",
            "volumeId": "v1",
            "chapterId": "c1",
            "timestamp": "2023-10-10"
        })
    );
    let deserialized: RecentEditDto = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(dto, deserialized);
    let as_object = json.as_object().unwrap();
    assert_eq!(
        as_object.len(),
        4,
        "RecentEditDto must have exactly 4 JSON keys"
    );
}
