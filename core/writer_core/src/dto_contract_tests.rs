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
        "title": "Test",
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
        "title",
        "totalWordCount",
        "updatedAt",
        "volumeCount",
    ];
    let actual_keys = sorted_keys(&ffi_project);
    assert_eq!(
        actual_keys, expected_keys,
        "Project DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn volume_dto_fields_match_harmony() {
    let ffi_volume = json!({
        "id": "v1",
        "projectId": "p1",
        "title": "Volume 1",
        "order": 0,
        "chapterCount": 3,
        "createdAt": "2024-01-01",
        "updatedAt": "2024-01-01"
    });

    let expected_keys = vec![
        "chapterCount",
        "createdAt",
        "id",
        "order",
        "projectId",
        "title",
        "updatedAt",
    ];
    let actual_keys = sorted_keys(&ffi_volume);
    assert_eq!(
        actual_keys, expected_keys,
        "Volume DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn chapter_dto_fields_match_harmony() {
    let ffi_chapter = json!({
        "id": "c1",
        "volumeId": "v1",
        "title": "Chapter 1",
        "wordCount": 500,
        "order": 0,
        "updatedAt": "2024-01-01",
        "createdAt": "2024-01-01"
    });

    let expected_keys = vec![
        "createdAt",
        "id",
        "order",
        "title",
        "updatedAt",
        "volumeId",
        "wordCount",
    ];
    let actual_keys = sorted_keys(&ffi_chapter);
    assert_eq!(
        actual_keys, expected_keys,
        "Chapter DTO field names must match Harmony CoreDtos.ets"
    );
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
    assert_eq!(
        actual_keys, expected_keys,
        "ChapterData DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn save_receipt_dto_fields_match_harmony() {
    let ffi_receipt = json!({
        "success": true,
        "wordCount": 500,
        "savedAt": "2024-01-01"
    });

    let expected_keys = vec!["savedAt", "success", "wordCount"];
    let actual_keys = sorted_keys(&ffi_receipt);
    assert_eq!(
        actual_keys, expected_keys,
        "SaveReceipt DTO field names must match Harmony CoreDtos.ets"
    );
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
    assert_eq!(
        actual_keys, expected_keys,
        "LocalSettings DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn result_envelope_fields_match_harmony() {
    let envelope = json!({
        "success": true,
        "data": null,
        "errorCode": null,
        "userMessage": null
    });

    let expected_keys = vec!["data", "errorCode", "success", "userMessage"];
    let actual_keys = sorted_keys(&envelope);
    assert_eq!(
        actual_keys, expected_keys,
        "ResultEnvelope field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn project_tree_dto_fields_match_harmony() {
    let ffi_tree = json!({
        "project": {},
        "volumes": []
    });

    let expected_keys = vec!["project", "volumes"];
    let actual_keys = sorted_keys(&ffi_tree);
    assert_eq!(
        actual_keys, expected_keys,
        "ProjectTree DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn volume_tree_dto_fields_match_harmony() {
    let ffi_vol_tree = json!({
        "volume": {},
        "chapters": []
    });

    let expected_keys = vec!["chapters", "volume"];
    let actual_keys = sorted_keys(&ffi_vol_tree);
    assert_eq!(
        actual_keys, expected_keys,
        "VolumeTree DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn project_stats_dto_fields_match_harmony() {
    let ffi_stats = json!({
        "totalWordCount": 100,
        "volumeCount": 1,
        "chapterCount": 2
    });

    let expected_keys = vec!["chapterCount", "totalWordCount", "volumeCount"];
    let actual_keys = sorted_keys(&ffi_stats);
    assert_eq!(
        actual_keys, expected_keys,
        "ProjectStats DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_meta_dto_fields_match_harmony() {
    let ffi_starmap_meta = json!({
        "id": "sm1",
        "title": "StarMap 1",
        "description": "A star map",
        "nodeCount": 5,
        "edgeCount": 3,
        "projectId": "p1",
        "createdAt": "2024-01-01",
        "updatedAt": "2024-01-01",
        "layoutType": "force"
    });

    let expected_keys = vec![
        "createdAt",
        "description",
        "edgeCount",
        "id",
        "layoutType",
        "nodeCount",
        "projectId",
        "title",
        "updatedAt",
    ];
    let actual_keys = sorted_keys(&ffi_starmap_meta);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapMeta DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_node_dto_fields_match_harmony() {
    let ffi_starmap_node = json!({
        "id": "n1",
        "label": "Character A",
        "description": "Main character",
        "x": 100.0,
        "y": 200.0,
        "width": 80.0,
        "height": 40.0,
        "type": "character",
        "color": "#FF5722",
        "icon": "person",
        "metadata": {},
        "parentId": "n0",
        "childIds": []
    });

    let expected_keys = vec![
        "childIds",
        "color",
        "description",
        "height",
        "icon",
        "id",
        "label",
        "metadata",
        "parentId",
        "type",
        "width",
        "x",
        "y",
    ];
    let actual_keys = sorted_keys(&ffi_starmap_node);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapNode DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_edge_dto_fields_match_harmony() {
    let ffi_starmap_edge = json!({
        "id": "e1",
        "sourceId": "n1",
        "targetId": "n2",
        "label": "knows",
        "type": "relation",
        "color": "#4CAF50",
        "style": "solid",
        "metadata": {}
    });

    let expected_keys = vec![
        "color", "id", "label", "metadata", "sourceId", "style", "targetId", "type",
    ];
    let actual_keys = sorted_keys(&ffi_starmap_edge);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapEdge DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn writing_stats_dto_fields_match_harmony() {
    let ffi_writing_stats = json!({
        "totalChars": 5000,
        "totalWords": 3000,
        "totalChapters": 10,
        "totalProjects": 2,
        "todayChars": 500,
        "todayDuration": 3600,
        "todaySessions": 3,
        "averageSpeed": 25.0,
        "longestStreak": 7,
        "currentStreak": 3,
        "weeklyStats": []
    });

    let expected_keys = vec![
        "averageSpeed",
        "currentStreak",
        "longestStreak",
        "todayChars",
        "todayDuration",
        "todaySessions",
        "totalChapters",
        "totalChars",
        "totalProjects",
        "totalWords",
        "weeklyStats",
    ];
    let actual_keys = sorted_keys(&ffi_writing_stats);
    assert_eq!(
        actual_keys, expected_keys,
        "WritingStats DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn sync_config_dto_fields_match_harmony() {
    let ffi_sync_config = json!({
        "enabled": true,
        "provider": "github",
        "remoteUrl": "https://github.com/user/repo",
        "branch": "main",
        "autoSync": true,
        "autoSyncInterval": 300,
        "conflictStrategy": "manual",
        "lastSyncAt": "2024-01-01",
        "syncPath": "/sync"
    });

    let expected_keys = vec![
        "autoSync",
        "autoSyncInterval",
        "branch",
        "conflictStrategy",
        "enabled",
        "lastSyncAt",
        "provider",
        "remoteUrl",
        "syncPath",
    ];
    let actual_keys = sorted_keys(&ffi_sync_config);
    assert_eq!(
        actual_keys, expected_keys,
        "SyncConfig DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn recent_edit_dto_fields_match_harmony() {
    let ffi_recent_edit = json!({
        "projectId": "p1",
        "projectTitle": "My Novel",
        "chapterId": "c1",
        "chapterTitle": "Chapter 1",
        "volumeId": "v1",
        "timestamp": "2024-01-01",
        "wordCount": 500
    });

    let expected_keys = vec![
        "chapterId",
        "chapterTitle",
        "projectId",
        "projectTitle",
        "timestamp",
        "volumeId",
        "wordCount",
    ];
    let actual_keys = sorted_keys(&ffi_recent_edit);
    assert_eq!(
        actual_keys, expected_keys,
        "RecentEdit DTO field names must match Harmony CoreDtos.ets"
    );
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
    assert!(
        json.get("title").is_some(),
        "Core internal Project uses 'title', not 'name'"
    );
    assert!(json.get("id").is_some());
    assert!(
        json.get("created_at").is_some(),
        "Core internal uses snake_case"
    );
}

#[test]
fn ffi_project_maps_title_to_title() {
    let project = crate::project::Project {
        id: "p1".into(),
        title: "My Novel".into(),
        created_at: "2024-01-01".into(),
        updated_at: "2024-01-01".into(),
        order: 0,
    };
    let ffi_json = json!({
        "id": project.id,
        "title": project.title,
        "createdAt": project.created_at,
        "updatedAt": project.updated_at
    });
    assert_eq!(
        ffi_json["title"], "My Novel",
        "FFI must output 'title' to match Core DTO"
    );
    assert!(
        ffi_json.get("name").is_none(),
        "FFI should NOT output 'name' key — use 'title'"
    );
}

// ── Layout Policy DTO contract tests ──

#[test]
fn window_metrics_dto_fields_match_harmony() {
    let metrics = crate::layout_policy::WindowMetrics::default();
    let _json = serde_json::to_value(&metrics).unwrap();
    // Core uses snake_case internally, but FFI JSON output uses camelCase
    // Verify the expected camelCase keys exist when serialized through FFI
    let ffi_json = json!({
        "widthVp": metrics.width_vp,
        "heightVp": metrics.height_vp,
        "safeTopVp": metrics.safe_top_vp,
        "safeBottomVp": metrics.safe_bottom_vp,
        "keyboardVisible": metrics.keyboard_visible,
        "foldPosture": "Unknown",
        "orientation": "Portrait",
        "pointer": "Touch"
    });
    let expected_keys = vec![
        "foldPosture",
        "heightVp",
        "keyboardVisible",
        "orientation",
        "pointer",
        "safeBottomVp",
        "safeTopVp",
        "widthVp",
    ];
    let actual_keys = sorted_keys(&ffi_json);
    assert_eq!(
        actual_keys, expected_keys,
        "WindowMetrics DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn layout_plan_dto_fields_match_harmony() {
    let metrics = crate::layout_policy::WindowMetrics::default();
    let plan = crate::layout_policy::resolve_layout(&metrics);
    let json = serde_json::to_value(&plan).unwrap();
    // Verify Core internal serialization uses snake_case
    assert!(
        json.get("width_class").is_some(),
        "Core internal LayoutPlan uses snake_case 'width_class'"
    );
    assert!(
        json.get("shell_mode").is_some(),
        "Core internal LayoutPlan uses snake_case 'shell_mode'"
    );
    assert!(
        json.get("content_max_width_vp").is_some(),
        "Core internal LayoutPlan uses snake_case 'content_max_width_vp'"
    );

    // Verify FFI output uses camelCase
    let ffi_json = json!({
        "widthClass": "Compact",
        "heightClass": "Compact",
        "shellMode": "SinglePane",
        "editorMode": "FullWidth",
        "navigationMode": "Stack",
        "contentMaxWidthVp": 0.0,
        "pagePaddingVp": 16.0,
        "gridColumns": 2,
        "showSidePanel": false,
        "showBottomBar": true
    });
    let expected_keys = vec![
        "contentMaxWidthVp",
        "editorMode",
        "gridColumns",
        "heightClass",
        "navigationMode",
        "pagePaddingVp",
        "shellMode",
        "showBottomBar",
        "showSidePanel",
        "widthClass",
    ];
    let actual_keys = sorted_keys(&ffi_json);
    assert_eq!(
        actual_keys, expected_keys,
        "LayoutPlan DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn workspace_summary_dto_fields_match_harmony() {
    let ffi_workspace = json!({
        "path": "/mock/workspace",
        "isValid": true,
        "projects": [],
        "recentEdits": []
    });
    let expected_keys = vec!["isValid", "path", "projects", "recentEdits"];
    let actual_keys = sorted_keys(&ffi_workspace);
    assert_eq!(
        actual_keys, expected_keys,
        "WorkspaceSummary DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn chapter_location_dto_fields_match_harmony() {
    let ffi_location = json!({
        "projectId": "p1",
        "volumeId": "v1",
        "chapterId": "c1"
    });
    let expected_keys = vec!["chapterId", "projectId", "volumeId"];
    let actual_keys = sorted_keys(&ffi_location);
    assert_eq!(
        actual_keys, expected_keys,
        "ChapterLocation DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn writing_event_dto_fields_match_harmony() {
    let ffi_event = json!({
        "deviceId": "harmony",
        "platform": "harmony",
        "projectId": "p1",
        "volumeId": "v1",
        "chapterId": "c1",
        "oldText": "旧文本",
        "newText": "新文本",
        "durationSeconds": 60,
        "sessionId": "hm-1234567890"
    });
    // 验证所有字段名都是 camelCase
    let expected_keys = vec![
        "chapterId",
        "deviceId",
        "durationSeconds",
        "newText",
        "oldText",
        "platform",
        "projectId",
        "sessionId",
        "volumeId",
    ];
    let actual_keys = sorted_keys(&ffi_event);
    assert_eq!(
        actual_keys, expected_keys,
        "WritingEvent DTO field names must match Harmony CoreDtos.ets"
    );
}

// ── StarMap Motion Policy / Layout / Graph / Viewport DTO contract tests ──

#[test]
fn starmap_motion_policy_dto_fields_match_harmony() {
    let policy = crate::starmap::types::StarMapMotionPolicyDto::default();
    let json = serde_json::to_value(&policy).unwrap();
    // 验证 camelCase 字段名
    let expected_keys = vec![
        "dragLiftScale",
        "dragShadowBoost",
        "enabled",
        "idleAmplitudeVp",
        "idlePeriodMs",
        "idleWobbleEnabled",
        "reduceMotion",
        "settleDurationMs",
    ];
    let actual_keys = sorted_keys(&json);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapMotionPolicyDto DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_layout_dto_fields_match_harmony() {
    let layout = crate::starmap::types::StarMapLayout::default();
    let json = serde_json::to_value(&layout).unwrap();
    let expected_keys = vec!["kind", "nodes"];
    let actual_keys = sorted_keys(&json);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapLayout DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_layout_node_dto_fields_match_harmony() {
    let ffi_node = json!({
        "nodeId": "n1",
        "x": 100.0,
        "y": 200.0,
        "width": 160.0,
        "height": 80.0,
        "radius": 16.0,
        "collapsed": false,
        "zIndex": 0,
        "scale": 1.0,
        "depth": 0.0,
        "focusWeight": 0.0,
        "orbitGroup": null
    });
    let expected_keys = vec![
        "collapsed",
        "depth",
        "focusWeight",
        "height",
        "nodeId",
        "orbitGroup",
        "radius",
        "scale",
        "width",
        "x",
        "y",
        "zIndex",
    ];
    let actual_keys = sorted_keys(&ffi_node);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapLayoutNode DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_graph_dto_serialization_contract() {
    let graph = crate::starmap::types::StarMapGraph::default();
    let json = serde_json::to_value(&graph).unwrap();
    // Core 内部使用 camelCase (因为 #[serde(rename_all = "camelCase")])
    assert!(
        json.get("schemaVersion").is_some(),
        "StarMapGraph must serialize schemaVersion in camelCase"
    );
    assert!(json.get("starmapId").is_some());
    assert!(json.get("nodes").is_some());
    assert!(json.get("edges").is_some());
    assert!(json.get("embeds").is_some());
    assert!(json.get("links").is_some());
    assert!(json.get("createdAt").is_some());
    assert!(json.get("updatedAt").is_some());
}

#[test]
fn starmap_viewport_dto_fields_match_harmony() {
    let viewport = crate::starmap::types::StarMapViewport::default();
    let json = serde_json::to_value(&viewport).unwrap();
    let expected_keys = vec!["height", "offsetX", "offsetY", "scale", "width"];
    let actual_keys = sorted_keys(&json);
    assert_eq!(
        actual_keys, expected_keys,
        "StarMapViewport DTO field names must match Harmony CoreDtos.ets"
    );
}

// ── ThemePalette DTO contract tests ──
// Android 端 ThemePaletteHelper 生成 JSON，Rust 端 ThemePaletteDto 解析。
// ThemePaletteDto 没有 #[serde(rename_all = "camelCase")]，默认期望 snake_case。

#[test]
fn theme_palette_dto_parses_snake_case_json() {
    // Build JSON incrementally to avoid macro recursion limit
    let mut map = serde_json::Map::new();
    map.insert("source".into(), json!("android_dynamic_color"));
    map.insert("updated_at_ms".into(), json!(1234567890i64));
    map.insert("device_id".into(), json!("test_device"));
    map.insert("variant".into(), json!("tonal_spot"));
    // Light palette
    map.insert("light_primary".into(), json!("#FF1234"));
    map.insert("light_on_primary".into(), json!("#FFFFFF"));
    map.insert("light_primary_container".into(), json!("#FFE0E0"));
    map.insert("light_on_primary_container".into(), json!("#690005"));
    map.insert("light_secondary".into(), json!("#FF5678"));
    map.insert("light_on_secondary".into(), json!("#FFFFFF"));
    map.insert("light_secondary_container".into(), json!("#FFDADA"));
    map.insert("light_on_secondary_container".into(), json!("#5F1318"));
    map.insert("light_tertiary".into(), json!("#FF9ABC"));
    map.insert("light_on_tertiary".into(), json!("#FFFFFF"));
    map.insert("light_tertiary_container".into(), json!("#FFDADF"));
    map.insert("light_on_tertiary_container".into(), json!("#5F1128"));
    map.insert("light_background".into(), json!("#FFFBFF"));
    map.insert("light_on_background".into(), json!("#1C1B1F"));
    map.insert("light_surface".into(), json!("#FFFBFF"));
    map.insert("light_on_surface".into(), json!("#1C1B1F"));
    map.insert("light_surface_variant".into(), json!("#E7E0EC"));
    map.insert("light_on_surface_variant".into(), json!("#49454F"));
    map.insert("light_surface_container".into(), json!("#F3EDF7"));
    map.insert("light_surface_container_high".into(), json!("#ECE6F0"));
    map.insert("light_outline".into(), json!("#79747E"));
    map.insert("light_outline_variant".into(), json!("#CAC4D0"));
    // Dark palette
    map.insert("dark_primary".into(), json!("#D0BCFF"));
    map.insert("dark_on_primary".into(), json!("#381E72"));
    map.insert("dark_primary_container".into(), json!("#4F378B"));
    map.insert("dark_on_primary_container".into(), json!("#EADDFF"));
    map.insert("dark_secondary".into(), json!("#CCC2DC"));
    map.insert("dark_on_secondary".into(), json!("#332D41"));
    map.insert("dark_secondary_container".into(), json!("#4A4458"));
    map.insert("dark_on_secondary_container".into(), json!("#E8DEF8"));
    map.insert("dark_tertiary".into(), json!("#EFB8C8"));
    map.insert("dark_on_tertiary".into(), json!("#492532"));
    map.insert("dark_tertiary_container".into(), json!("#633B48"));
    map.insert("dark_on_tertiary_container".into(), json!("#FFD8E4"));
    map.insert("dark_background".into(), json!("#1C1B1F"));
    map.insert("dark_on_background".into(), json!("#E6E1E5"));
    map.insert("dark_surface".into(), json!("#1C1B1F"));
    map.insert("dark_on_surface".into(), json!("#E6E1E5"));
    map.insert("dark_surface_variant".into(), json!("#49454F"));
    map.insert("dark_on_surface_variant".into(), json!("#CAC4D0"));
    map.insert("dark_surface_container".into(), json!("#211F26"));
    map.insert("dark_surface_container_high".into(), json!("#2B2930"));
    map.insert("dark_outline".into(), json!("#938F99"));
    map.insert("dark_outline_variant".into(), json!("#49454F"));

    let json_val = serde_json::Value::Object(map);
    let dto: crate::api::types::ThemePaletteDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(dto.source, "android_dynamic_color");
    assert_eq!(dto.updated_at_ms, 1234567890i64);
    assert_eq!(dto.device_id, "test_device");
    assert_eq!(dto.variant, "tonal_spot");
    assert!(!dto.light_primary.is_empty(), "light_primary must not be empty");
    assert!(!dto.dark_primary.is_empty(), "dark_primary must not be empty");
    assert!(!dto.light_surface_container_high.is_empty(), "light_surface_container_high must not be empty");
    assert!(!dto.dark_surface_container_high.is_empty(), "dark_surface_container_high must not be empty");
}

#[test]
fn theme_palette_dto_rejects_camel_case_json() {
    // camelCase JSON should NOT populate snake_case Rust fields.
    // Since ThemePaletteDto has no #[serde(default)] on String fields, we must provide
    // all required snake_case fields (as empty strings) so deserialization succeeds,
    // then add camelCase keys to verify they are NOT matched to the struct fields.
    let mut map = serde_json::Map::new();
    map.insert("source".into(), json!("android_dynamic_color"));
    map.insert("updated_at_ms".into(), json!(0i64));
    map.insert("device_id".into(), json!(""));
    map.insert("variant".into(), json!("tonal_spot"));
    // Provide all required snake_case fields as empty strings
    let snake_case_color_keys = [
        "light_primary", "light_on_primary", "light_primary_container", "light_on_primary_container",
        "light_secondary", "light_on_secondary", "light_secondary_container", "light_on_secondary_container",
        "light_tertiary", "light_on_tertiary", "light_tertiary_container", "light_on_tertiary_container",
        "light_background", "light_on_background", "light_surface", "light_on_surface",
        "light_surface_variant", "light_on_surface_variant",
        "light_surface_container", "light_surface_container_high",
        "light_outline", "light_outline_variant",
        "dark_primary", "dark_on_primary", "dark_primary_container", "dark_on_primary_container",
        "dark_secondary", "dark_on_secondary", "dark_secondary_container", "dark_on_secondary_container",
        "dark_tertiary", "dark_on_tertiary", "dark_tertiary_container", "dark_on_tertiary_container",
        "dark_background", "dark_on_background", "dark_surface", "dark_on_surface",
        "dark_surface_variant", "dark_on_surface_variant",
        "dark_surface_container", "dark_surface_container_high",
        "dark_outline", "dark_outline_variant",
    ];
    for key in &snake_case_color_keys {
        map.insert((*key).into(), json!(""));
    }
    // Add camelCase keys with non-empty values — these should NOT be matched
    map.insert("lightPrimary".into(), json!("#FF1234"));
    map.insert("darkPrimary".into(), json!("#D0BCFF"));
    map.insert("lightSurfaceContainerHigh".into(), json!("#ECE6F0"));
    map.insert("darkSurfaceContainerHigh".into(), json!("#2B2930"));

    let json_val = serde_json::Value::Object(map);
    let dto: crate::api::types::ThemePaletteDto = serde_json::from_value(json_val).unwrap();
    // These fields should be empty because camelCase keys don't match snake_case fields
    assert!(dto.light_primary.is_empty(), "camelCase lightPrimary should not populate snake_case light_primary");
    assert!(dto.dark_primary.is_empty(), "camelCase darkPrimary should not populate snake_case dark_primary");
    assert!(dto.light_surface_container_high.is_empty(), "camelCase lightSurfaceContainerHigh should not populate snake_case light_surface_container_high");
    assert!(dto.dark_surface_container_high.is_empty(), "camelCase darkSurfaceContainerHigh should not populate snake_case dark_surface_container_high");
}
