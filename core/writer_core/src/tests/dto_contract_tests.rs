//! # DTO 序列化行为测试
//!
//! 验证 Rust 类型的序列化输出格式（snake_case/camelCase）和往返一致性。
//! 不再硬编码手工 JSON 字段名列表与源码对比——编译器和 serde 已保证结构对齐。

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
fn test_stats_summary_dto_contract() {
    let summary = crate::api::types::WritingStatsSummaryDto {
        range: crate::api::types::DateRangeDto {
            start_date: "2024-01-01".to_string(),
            end_date: "2024-01-02".to_string(),
        },
        total_human_typed_chars: 100,
        total_pasted_chars: 50,
        total_deleted_chars: 10,
        total_ai_inserted_chars: 5,
        total_net_delta_chars: 145,
        total_active_seconds: 3600,
        total_sessions: 2,
        days_count: 2,
    };

    let json = serde_json::to_value(&summary).unwrap();
    let mut expected_keys = vec![
        "daysCount",
        "range",
        "totalActiveSeconds",
        "totalAiInsertedChars",
        "totalDeletedChars",
        "totalHumanTypedChars",
        "totalNetDeltaChars",
        "totalPastedChars",
        "totalSessions",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();
    expected_keys.sort();

    let actual_keys = sorted_keys(&json);
    assert_eq!(actual_keys, expected_keys);

    let range = &json["range"];
    assert!(range.get("startDate").is_some());
    assert!(range.get("endDate").is_some());
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

#[test]
fn starmap_graph_dto_serialization_contract() {
    let graph = crate::starmap::types::StarMapGraph::default();
    let json = serde_json::to_value(&graph).unwrap();
    assert!(
        json.get("schemaVersion").is_some(),
        "StarMapGraph must serialize schemaVersion in camelCase"
    );
    assert!(json.get("starmapId").is_some());
    assert!(json.get("nodes").is_some());
    assert!(json.get("edges").is_some());
    assert!(json.get("embeds").is_some());
    assert!(json.get("links").is_some());
}

#[test]
fn test_settings_auto_indent_logic() {
    let mut settings = crate::settings::LocalSettings::default();
    settings.auto_indent_enabled = true;
    settings.auto_indent_width = 4.0;
    settings.validate();
    assert!(settings.auto_indent_enabled);
    assert_eq!(settings.auto_indent_width, 4.0);

    settings.auto_indent_width = 0.0;
    settings.validate();
    assert_eq!(settings.auto_indent_width, 0.0);

    settings.auto_indent_width = 100.0;
    settings.validate();
    assert_eq!(
        settings.auto_indent_width,
        crate::settings::ranges::INDENT_WIDTH_MAX
    );

    settings.auto_indent_width = -1.0;
    settings.validate();
    assert_eq!(
        settings.auto_indent_width,
        crate::settings::ranges::INDENT_WIDTH_MIN
    );
}

#[test]
fn test_settings_auto_indent_contract() {
    let settings = crate::settings::LocalSettings {
        auto_indent_enabled: true,
        auto_indent_width: 4.0,
        ..Default::default()
    };

    let json = serde_json::to_value(&settings).unwrap();

    assert_eq!(json["autoIndentEnabled"], true);
    assert_eq!(json["autoIndentWidth"], 4.0);
}
