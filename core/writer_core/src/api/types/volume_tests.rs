use super::volume::*;
use crate::chapter::Chapter;
use crate::volume::Volume;

#[test]
fn test_volume_dto_from_volume() {
    let volume = Volume {
        id: "vol-1".to_string(),
        title: "Test Volume".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        order: 1,
    };
    let dto = VolumeDto::from(volume);
    assert_eq!(dto.id, "vol-1");
    assert_eq!(dto.title, "Test Volume");
    assert_eq!(dto.created_at, "2023-01-01T00:00:00Z");
    assert_eq!(dto.updated_at, "2023-01-02T00:00:00Z");
    assert_eq!(dto.order, 1);
}

#[test]
fn test_chapter_meta_dto_from_chapter() {
    let chapter = Chapter {
        id: "ch-1".to_string(),
        title: "Test Chapter".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        order: 2,
        word_count: 500,
        hash: "abc123hash".to_string(),
        note: Some("Test note".to_string()),
    };
    let dto = ChapterMetaDto::from(chapter);
    assert_eq!(dto.id, "ch-1");
    assert_eq!(dto.title, "Test Chapter");
    assert_eq!(dto.created_at, "2023-01-01T00:00:00Z");
    assert_eq!(dto.updated_at, "2023-01-02T00:00:00Z");
    assert_eq!(dto.order, 2);
    assert_eq!(dto.word_count, 500);
    assert_eq!(dto.hash, "abc123hash");
    assert_eq!(dto.note, Some("Test note".to_string()));
}

#[test]
fn test_volume_dto_serialization_roundtrip() {
    let dto = VolumeDto {
        id: "vol_1".to_string(),
        title: "Test Volume".to_string(),
        created_at: "2023-01-01T00:00:00Z".to_string(),
        updated_at: "2023-01-02T00:00:00Z".to_string(),
        order: 1,
    };
    let json_val = serde_json::to_value(&dto).unwrap();
    assert_eq!(json_val["id"], "vol_1");
    assert_eq!(json_val["title"], "Test Volume");
    assert_eq!(json_val["order"], 1);
    let json_str = serde_json::to_string(&dto).unwrap();
    let deserialized: VolumeDto = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized, dto);
}
