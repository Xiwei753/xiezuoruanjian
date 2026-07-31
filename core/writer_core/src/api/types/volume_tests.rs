use super::volume::*;
use crate::chapter::Chapter;
use crate::volume::Volume;
use serde_json::json;

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
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "vol_1",
            "title": "Test Volume",
            "createdAt": "2023-01-01T00:00:00Z",
            "updatedAt": "2023-01-02T00:00:00Z",
            "order": 1
        })
    );
    let deserialized: VolumeDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_chapter_meta_dto_json_key_contract() {
    let dto = ChapterMetaDto {
        id: "ch1".to_string(),
        title: "Ch".to_string(),
        created_at: "2023-01-01".to_string(),
        updated_at: "2023-01-02".to_string(),
        order: 3,
        word_count: 100,
        hash: "h1".to_string(),
        note: None,
    };
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json,
        json!({
            "id": "ch1",
            "title": "Ch",
            "createdAt": "2023-01-01",
            "updatedAt": "2023-01-02",
            "order": 3,
            "wordCount": 100,
            "hash": "h1",
            "note": null
        })
    );
    let deserialized: ChapterMetaDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}
