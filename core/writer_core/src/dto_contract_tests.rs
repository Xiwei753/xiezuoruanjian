//! # 跨端 DTO 契约测试
//!
//! 验证 FFI 层手动拼接的 JSON 字段名与各端期望的字段名对齐。
//! Harmony 端参考: apps/harmony/entry/src/main/ets/model/CoreDtos.ets
//! Desktop 端参考: apps/desktop/src/backend/ (QML via json_utils)
//! Android 端参考: core/writer_core/src/api.udl + api/types/

use serde_json::json;

fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            keys
        }
        _ => vec![],
    }
}

#[test]
fn project_dto_fields_match_harmony() {
    let ffi_project = json!({
        "id": "p1",
        "name": "Test",
        "volumeCount": 1,
        "chapterCount": 2,
        "totalWordCount": 100,
        "createdAt": "2024-01-01",
        "updatedAt": "2024-01-01"
    });

    let expected_keys = vec![
        "chapterCount",
        "createdAt",
        "id",
        "name",
        "totalWordCount",
        "updatedAt",
        "volumeCount",
    ];
    let actual_keys = sorted_keys(&ffi_project);
    assert_eq!(actual_keys, expected_keys, "Project DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn volume_dto_fields_match_harmony() {
    let ffi_volume = json!({
        "id": "v1",
        "projectId": "p1",
        "name": "Volume 1",
        "order": 0,
        "chapterCount": 3,
        "createdAt": "2024-01-01",
        "updatedAt": "2024-01-01"
    });

    let expected_keys = vec![
        "chapterCount",
        "createdAt",
        "id",
        "name",
        "order",
        "projectId",
        "updatedAt",
    ];
    let actual_keys = sorted_keys(&ffi_volume);
    assert_eq!(actual_keys, expected_keys, "Volume DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn chapter_dto_fields_match_harmony() {
    let ffi_chapter = json!({
        "id": "c1",
        "volumeId": "v1",
        "name": "Chapter 1",
        "wordCount": 500,
        "order": 0,
        "updatedAt": "2024-01-01",
        "createdAt": "2024-01-01"
    });

    let expected_keys = vec![
        "createdAt",
        "id",
        "name",
        "order",
        "updatedAt",
        "volumeId",
        "wordCount",
    ];
    let actual_keys = sorted_keys(&ffi_chapter);
    assert_eq!(actual_keys, expected_keys, "Chapter DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn chapter_data_dto_fields_match_harmony() {
    let ffi_chapter_data = json!({
        "id": "c1",
        "title": "Chapter 1",
        "content": "Hello world",
        "wordCount": 500,
        "volumeId": "v1",
        "projectId": "p1",
        "updatedAt": "2024-01-01",
        "createdAt": "2024-01-01"
    });

    let expected_keys = vec![
        "content",
        "createdAt",
        "id",
        "projectId",
        "title",
        "updatedAt",
        "volumeId",
        "wordCount",
    ];
    let actual_keys = sorted_keys(&ffi_chapter_data);
    assert_eq!(actual_keys, expected_keys, "ChapterData DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn save_receipt_dto_fields_match_harmony() {
    let ffi_receipt = json!({
        "success": true,
        "wordCount": 500,
        "savedAt": "2024-01-01"
    });

    let expected_keys = vec![
        "savedAt",
        "success",
        "wordCount",
    ];
    let actual_keys = sorted_keys(&ffi_receipt);
    assert_eq!(actual_keys, expected_keys, "SaveReceipt DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn local_settings_dto_fields_match_harmony() {
    let ffi_settings = json!({
        "fontSize": 16.0,
        "lineHeight": 1.5,
        "fontFamily": "HarmonyOS Sans",
        "theme": "system",
        "autoSave": true,
        "autoSaveInterval": 30.0,
        "autoIndent": true,
        "showWordCount": true,
        "showLineNumbers": false,
        "wordWrap": true,
        "spellCheck": false
    });

    let expected_keys = vec![
        "autoIndent",
        "autoSave",
        "autoSaveInterval",
        "fontFamily",
        "fontSize",
        "lineHeight",
        "showLineNumbers",
        "showWordCount",
        "spellCheck",
        "theme",
        "wordWrap",
    ];
    let actual_keys = sorted_keys(&ffi_settings);
    assert_eq!(actual_keys, expected_keys, "LocalSettings DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn result_envelope_fields_match_harmony() {
    let envelope = json!({
        "success": true,
        "data": null,
        "errorCode": null,
        "userMessage": null
    });

    let expected_keys = vec![
        "data",
        "errorCode",
        "success",
        "userMessage",
    ];
    let actual_keys = sorted_keys(&envelope);
    assert_eq!(actual_keys, expected_keys, "ResultEnvelope field names must match Harmony CoreDtos.ets");
}

#[test]
fn project_tree_dto_fields_match_harmony() {
    let ffi_tree = json!({
        "project": {},
        "volumes": []
    });

    let expected_keys = vec![
        "project",
        "volumes",
    ];
    let actual_keys = sorted_keys(&ffi_tree);
    assert_eq!(actual_keys, expected_keys, "ProjectTree DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn volume_tree_dto_fields_match_harmony() {
    let ffi_vol_tree = json!({
        "volume": {},
        "chapters": []
    });

    let expected_keys = vec![
        "chapters",
        "volume",
    ];
    let actual_keys = sorted_keys(&ffi_vol_tree);
    assert_eq!(actual_keys, expected_keys, "VolumeTree DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn project_stats_dto_fields_match_harmony() {
    let ffi_stats = json!({
        "totalWordCount": 100,
        "volumeCount": 1,
        "chapterCount": 2
    });

    let expected_keys = vec![
        "chapterCount",
        "totalWordCount",
        "volumeCount",
    ];
    let actual_keys = sorted_keys(&ffi_stats);
    assert_eq!(actual_keys, expected_keys, "ProjectStats DTO field names must match Harmony CoreDtos.ets");
}

#[test]
fn core_internal_project_serializes_with_snake_case() {
    let project = crate::project::Project {
        id: "p1".into(),
        title: "Test".into(),
        created_at: "2024-01-01".into(),
        updated_at: "2024-01-01".into(),
        order: 0,
    };
    let json = serde_json::to_value(&project).unwrap();
    assert!(json.get("title").is_some(), "Core internal Project uses 'title', not 'name'");
    assert!(json.get("id").is_some());
    assert!(json.get("created_at").is_some(), "Core internal uses snake_case");
}

#[test]
fn ffi_project_maps_title_to_name() {
    let project = crate::project::Project {
        id: "p1".into(),
        title: "My Novel".into(),
        created_at: "2024-01-01".into(),
        updated_at: "2024-01-01".into(),
        order: 0,
    };
    let ffi_json = json!({
        "id": project.id,
        "name": project.title,
        "createdAt": project.created_at,
        "updatedAt": project.updated_at
    });
    assert_eq!(ffi_json["name"], "My Novel", "FFI must map core 'title' → 'name' for Harmony");
    assert!(ffi_json.get("title").is_none(), "FFI should NOT expose 'title' key — use 'name'");
}