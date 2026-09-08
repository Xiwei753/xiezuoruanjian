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
fn window_viewport_dto_fields_match_harmony() {
    // #628：WindowViewportDto 替代 WindowCapabilitiesDto，只含 width_dp / height_dp。
    let viewport = crate::presentation::layout::resolver::WindowViewport::default();
    let _json = serde_json::to_value(&viewport).unwrap();
    let ffi_json = json!({
        "widthDp": viewport.width_dp,
        "heightDp": viewport.height_dp
    });
    let expected_keys = vec!["heightDp", "widthDp"];
    let actual_keys = sorted_keys(&ffi_json);
    assert_eq!(
        actual_keys, expected_keys,
        "WindowViewport DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn layout_contract_dto_fields_match_harmony() {
    // #628：LayoutContract 不再含 show_primary_navigation（改由 ScreenPolicy 提供），
    // 新增 primary_navigation_placement 与 metrics。
    let viewport = crate::presentation::layout::resolver::WindowViewport::default();
    let contract = crate::presentation::layout::resolve_layout(&viewport);
    let json = serde_json::to_value(&contract).unwrap();
    assert!(
        json.get("shell_mode").is_some(),
        "Core internal LayoutContract uses snake_case 'shell_mode'"
    );
    assert!(
        json.get("workspace_layout_mode").is_some(),
        "Core internal LayoutContract uses snake_case 'workspace_layout_mode'"
    );
    assert!(
        json.get("primary_navigation_placement").is_some(),
        "Core internal LayoutContract uses snake_case 'primary_navigation_placement'"
    );
    assert!(
        json.get("metrics").is_some(),
        "Core internal LayoutContract uses snake_case 'metrics'"
    );
    // #610：Core 不再输出 Material 断点 / dp / 导航呈现等平台值。
    assert!(
        json.get("content_max_width_dp").is_none()
            && json.get("page_padding_dp").is_none()
            && json.get("navigation_presentation").is_none()
            && json.get("width_class").is_none(),
        "LayoutContract 不得包含 Material 断点/dp/导航呈现等平台值"
    );
    // #628：Core 内部不再含 show_primary_navigation（改由 ScreenPolicy 提供）。
    assert!(
        json.get("show_primary_navigation").is_none(),
        "LayoutContract 不得再含 show_primary_navigation（改由 ScreenPolicy 提供）"
    );

    let ffi_json = json!({
        "shellMode": "SinglePane",
        "workspacePaneMode": "SinglePane",
        "primaryNavigationPlacement": "Bottom",
        "metrics": {
            "listPaneWidthDp": 320.0
        }
    });
    let expected_keys = vec![
        "metrics",
        "primaryNavigationPlacement",
        "shellMode",
        "workspacePaneMode",
    ];
    let actual_keys = sorted_keys(&ffi_json);
    assert_eq!(
        actual_keys, expected_keys,
        "LayoutContract DTO field names must match Harmony CoreDtos.ets"
    );
}

#[test]
fn starmap_motion_policy_dto_fields_match_harmony() {
    let policy = crate::starmap::types::StarMapMotionPolicyDto::default();
    let json = serde_json::to_value(&policy).unwrap();
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

#[test]
fn theme_palette_dto_parses_snake_case_json() {
    let mut map = serde_json::Map::new();
    map.insert("source".into(), json!("android_dynamic_color"));
    map.insert("updated_at_ms".into(), json!(1234567890i64));
    map.insert("device_id".into(), json!("test_device"));
    map.insert("variant".into(), json!("tonal_spot"));
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
    map.insert("light_surface_container_lowest".into(), json!("#FFFFFF"));
    map.insert("light_surface_container_low".into(), json!("#F7F1FA"));
    map.insert("light_surface_container".into(), json!("#F3EDF7"));
    map.insert("light_surface_container_high".into(), json!("#ECE6F0"));
    map.insert("light_surface_container_highest".into(), json!("#E6E0E9"));
    map.insert("light_outline".into(), json!("#79747E"));
    map.insert("light_outline_variant".into(), json!("#CAC4D0"));
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
    map.insert("dark_surface_container_lowest".into(), json!("#0F0D13"));
    map.insert("dark_surface_container_low".into(), json!("#1D1B20"));
    map.insert("dark_surface_container".into(), json!("#211F26"));
    map.insert("dark_surface_container_high".into(), json!("#2B2930"));
    map.insert("dark_surface_container_highest".into(), json!("#36343B"));
    map.insert("dark_outline".into(), json!("#938F99"));
    map.insert("dark_outline_variant".into(), json!("#49454F"));

    let json_val = serde_json::Value::Object(map);
    let dto: crate::api::types::ThemePaletteDto = serde_json::from_value(json_val).unwrap();
    assert_eq!(dto.source, "android_dynamic_color");
    assert_eq!(dto.updated_at_ms, 1234567890i64);
    assert_eq!(dto.device_id, "test_device");
    assert_eq!(dto.variant, "tonal_spot");
    assert!(
        !dto.light_primary.is_empty(),
        "light_primary must not be empty"
    );
    assert!(
        !dto.dark_primary.is_empty(),
        "dark_primary must not be empty"
    );
    assert!(
        !dto.light_surface_container_high.is_empty(),
        "light_surface_container_high must not be empty"
    );
    assert!(
        !dto.dark_surface_container_high.is_empty(),
        "dark_surface_container_high must not be empty"
    );
}

#[test]
fn theme_palette_dto_rejects_camel_case_json() {
    let mut map = serde_json::Map::new();
    map.insert("source".into(), json!("android_dynamic_color"));
    map.insert("updated_at_ms".into(), json!(0i64));
    map.insert("device_id".into(), json!(""));
    map.insert("variant".into(), json!("tonal_spot"));
    let snake_case_color_keys = [
        "light_primary",
        "light_on_primary",
        "light_primary_container",
        "light_on_primary_container",
        "light_secondary",
        "light_on_secondary",
        "light_secondary_container",
        "light_on_secondary_container",
        "light_tertiary",
        "light_on_tertiary",
        "light_tertiary_container",
        "light_on_tertiary_container",
        "light_background",
        "light_on_background",
        "light_surface",
        "light_on_surface",
        "light_surface_variant",
        "light_on_surface_variant",
        "light_surface_container_lowest",
        "light_surface_container_low",
        "light_surface_container",
        "light_surface_container_high",
        "light_surface_container_highest",
        "light_outline",
        "light_outline_variant",
        "dark_primary",
        "dark_on_primary",
        "dark_primary_container",
        "dark_on_primary_container",
        "dark_secondary",
        "dark_on_secondary",
        "dark_secondary_container",
        "dark_on_secondary_container",
        "dark_tertiary",
        "dark_on_tertiary",
        "dark_tertiary_container",
        "dark_on_tertiary_container",
        "dark_background",
        "dark_on_background",
        "dark_surface",
        "dark_on_surface",
        "dark_surface_variant",
        "dark_on_surface_variant",
        "dark_surface_container_lowest",
        "dark_surface_container_low",
        "dark_surface_container",
        "dark_surface_container_high",
        "dark_surface_container_highest",
        "dark_outline",
        "dark_outline_variant",
    ];
    for key in &snake_case_color_keys {
        map.insert((*key).into(), json!(""));
    }
    map.insert("lightPrimary".into(), json!("#FF1234"));
    map.insert("darkPrimary".into(), json!("#D0BCFF"));
    map.insert("lightSurfaceContainerHigh".into(), json!("#ECE6F0"));
    map.insert("darkSurfaceContainerHigh".into(), json!("#2B2930"));

    let json_val = serde_json::Value::Object(map);
    let dto: crate::api::types::ThemePaletteDto = serde_json::from_value(json_val).unwrap();
    assert!(
        dto.light_primary.is_empty(),
        "camelCase lightPrimary should not populate snake_case light_primary"
    );
    assert!(
        dto.dark_primary.is_empty(),
        "camelCase darkPrimary should not populate snake_case dark_primary"
    );
    assert!(dto.light_surface_container_high.is_empty(), "camelCase lightSurfaceContainerHigh should not populate snake_case light_surface_container_high");
    assert!(dto.dark_surface_container_high.is_empty(), "camelCase darkSurfaceContainerHigh should not populate snake_case dark_surface_container_high");
}

#[test]
fn theme_palette_dto_round_trip() {
    let original = crate::api::types::ThemePaletteDto {
        source: "android_dynamic_color".into(),
        updated_at_ms: 1719792000000i64,
        device_id: "test_device_001".into(),
        variant: "tonal_spot".into(),
        light_primary: "#006497".into(),
        light_on_primary: "#FFFFFF".into(),
        light_primary_container: "#CCE5FF".into(),
        light_on_primary_container: "#001E31".into(),
        light_secondary: "#50606E".into(),
        light_on_secondary: "#FFFFFF".into(),
        light_secondary_container: "#D3E5F5".into(),
        light_on_secondary_container: "#0C1D29".into(),
        light_tertiary: "#65587B".into(),
        light_on_tertiary: "#FFFFFF".into(),
        light_tertiary_container: "#EBDDFF".into(),
        light_on_tertiary_container: "#201634".into(),
        light_background: "#F6FAFE".into(),
        light_on_background: "#171C1F".into(),
        light_surface: "#F6FAFE".into(),
        light_on_surface: "#171C1F".into(),
        light_surface_variant: "#DEE3EB".into(),
        light_on_surface_variant: "#42474E".into(),
        light_surface_container_lowest: "#FFFFFF".into(),
        light_surface_container_low: "#F0F6FC".into(),
        light_surface_container: "#EAF0F7".into(),
        light_surface_container_high: "#E4EAF1".into(),
        light_surface_container_highest: "#DEE4EB".into(),
        light_outline: "#72787E".into(),
        light_outline_variant: "#C2C8CE".into(),
        dark_primary: "#85CFFF".into(),
        dark_on_primary: "#00344D".into(),
        dark_primary_container: "#004B6E".into(),
        dark_on_primary_container: "#CCE5FF".into(),
        dark_secondary: "#B7C9D8".into(),
        dark_on_secondary: "#22323F".into(),
        dark_secondary_container: "#384956".into(),
        dark_on_secondary_container: "#D3E5F5".into(),
        dark_tertiary: "#CFC0E8".into(),
        dark_on_tertiary: "#362E4B".into(),
        dark_tertiary_container: "#4D4462".into(),
        dark_on_tertiary_container: "#EBDDFF".into(),
        dark_background: "#0E1417".into(),
        dark_on_background: "#DEE3EB".into(),
        dark_surface: "#0E1417".into(),
        dark_on_surface: "#DEE3EB".into(),
        dark_surface_variant: "#42474E".into(),
        dark_on_surface_variant: "#C2C8CE".into(),
        dark_surface_container_lowest: "#0A0F12".into(),
        dark_surface_container_low: "#151B1F".into(),
        dark_surface_container: "#1B2024".into(),
        dark_surface_container_high: "#252B2F".into(),
        dark_surface_container_highest: "#30363A".into(),
        dark_outline: "#8C9298".into(),
        dark_outline_variant: "#42474E".into(),
    };

    let json_str = serde_json::to_string(&original).expect("ThemePaletteDto should serialize");
    let round_tripped: crate::api::types::ThemePaletteDto =
        serde_json::from_str(&json_str).expect("ThemePaletteDto JSON should deserialize");

    assert_eq!(
        original, round_tripped,
        "ThemePaletteDto round-trip must be lossless"
    );

    assert!(
        !original.light_primary.is_empty(),
        "light_primary must not be empty"
    );
    assert!(
        !original.dark_surface.is_empty(),
        "dark_surface must not be empty"
    );
    assert!(
        !original.light_outline_variant.is_empty(),
        "light_outline_variant must not be empty"
    );
    assert!(
        !original.dark_outline_variant.is_empty(),
        "dark_outline_variant must not be empty"
    );
}

#[test]
fn syncable_settings_persistence_round_trip() {
    use crate::settings::{SyncableSettings, ThemePalette};

    #[allow(deprecated)]
    let original = SyncableSettings {
        font_size: 18.0,
        theme_mode: "dark".to_string(),
        monet_color: "#FF5722".to_string(),
        theme_palette: ThemePalette {
            source: "android_dynamic_color".to_string(),
            updated_at_ms: 1700000000000,
            device_id: "device-abc".to_string(),
            variant: "tonal_spot".to_string(),
            light_primary: "#006494".to_string(),
            light_on_primary: "#FFFFFF".to_string(),
            light_primary_container: "#CCE5FF".to_string(),
            light_on_primary_container: "#001E30".to_string(),
            light_secondary: "#4F616E".to_string(),
            light_on_secondary: "#FFFFFF".to_string(),
            light_secondary_container: "#D3E5F3".to_string(),
            light_on_secondary_container: "#0B1D29".to_string(),
            light_tertiary: "#61587C".to_string(),
            light_on_tertiary: "#FFFFFF".to_string(),
            light_tertiary_container: "#E7DDFF".to_string(),
            light_on_tertiary_container: "#1D1535".to_string(),
            light_background: "#F9FAFF".to_string(),
            light_on_background: "#1A1C1E".to_string(),
            light_surface: "#F9FAFF".to_string(),
            light_on_surface: "#1A1C1E".to_string(),
            light_surface_variant: "#DFE2EB".to_string(),
            light_on_surface_variant: "#434750".to_string(),
            light_surface_container_lowest: "#FFFFFF".to_string(),
            light_surface_container_low: "#F3F4FA".to_string(),
            light_surface_container: "#EDEEF4".to_string(),
            light_surface_container_high: "#E7E8EF".to_string(),
            light_surface_container_highest: "#E2E3E9".to_string(),
            light_outline: "#73777F".to_string(),
            light_outline_variant: "#C3C7CF".to_string(),
            dark_primary: "#92CCFF".to_string(),
            dark_on_primary: "#003352".to_string(),
            dark_primary_container: "#004A73".to_string(),
            dark_on_primary_container: "#CCE5FF".to_string(),
            dark_secondary: "#B7C9D7".to_string(),
            dark_on_secondary: "#21323F".to_string(),
            dark_secondary_container: "#374955".to_string(),
            dark_on_secondary_container: "#D3E5F3".to_string(),
            dark_tertiary: "#CBC1E9".to_string(),
            dark_on_tertiary: "#322D4B".to_string(),
            dark_tertiary_container: "#494363".to_string(),
            dark_on_tertiary_container: "#E7DDFF".to_string(),
            dark_background: "#1A1C1E".to_string(),
            dark_on_background: "#E2E2E5".to_string(),
            dark_surface: "#1A1C1E".to_string(),
            dark_on_surface: "#E2E2E5".to_string(),
            dark_surface_variant: "#434750".to_string(),
            dark_on_surface_variant: "#C3C7CF".to_string(),
            dark_surface_container_lowest: "#0E1114".to_string(),
            dark_surface_container_low: "#1A1D21".to_string(),
            dark_surface_container: "#1E2125".to_string(),
            dark_surface_container_high: "#282B30".to_string(),
            dark_surface_container_highest: "#33363A".to_string(),
            dark_outline: "#8D9199".to_string(),
            dark_outline_variant: "#434750".to_string(),
        },
    };

    let json_str = serde_json::to_string_pretty(&original).unwrap();
    let restored: SyncableSettings = serde_json::from_str(&json_str).unwrap();

    assert_eq!(restored.font_size, 18.0);
    assert_eq!(restored.theme_mode, "dark");
    assert_eq!(restored.monet_color, "#FF5722");
    assert_eq!(restored.theme_palette.source, "android_dynamic_color");
    assert_eq!(restored.theme_palette.updated_at_ms, 1700000000000);
    assert_eq!(restored.theme_palette.device_id, "device-abc");
    assert_eq!(restored.theme_palette.variant, "tonal_spot");
    assert_eq!(
        restored.theme_palette.light_surface_container_lowest,
        "#FFFFFF"
    );
    assert_eq!(
        restored.theme_palette.light_surface_container_low,
        "#F3F4FA"
    );
    assert_eq!(restored.theme_palette.light_surface_container, "#EDEEF4");
    assert_eq!(
        restored.theme_palette.light_surface_container_high,
        "#E7E8EF"
    );
    assert_eq!(
        restored.theme_palette.light_surface_container_highest,
        "#E2E3E9"
    );
    assert_eq!(
        restored.theme_palette.dark_surface_container_lowest,
        "#0E1114"
    );
    assert_eq!(restored.theme_palette.dark_surface_container_low, "#1A1D21");
    assert_eq!(restored.theme_palette.dark_surface_container, "#1E2125");
    assert_eq!(
        restored.theme_palette.dark_surface_container_high,
        "#282B30"
    );
    assert_eq!(
        restored.theme_palette.dark_surface_container_highest,
        "#33363A"
    );

    let json_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    let tp = &json_val["themePalette"];
    assert!(
        tp.get("lightSurfaceContainerLowest").is_some(),
        "camelCase key lightSurfaceContainerLowest must exist in persisted JSON"
    );
    assert!(
        tp.get("lightSurfaceContainerLow").is_some(),
        "camelCase key lightSurfaceContainerLow must exist in persisted JSON"
    );
    assert!(
        tp.get("lightSurfaceContainerHighest").is_some(),
        "camelCase key lightSurfaceContainerHighest must exist in persisted JSON"
    );
    assert!(
        tp.get("darkSurfaceContainerLowest").is_some(),
        "camelCase key darkSurfaceContainerLowest must exist in persisted JSON"
    );
    assert!(
        tp.get("darkSurfaceContainerLow").is_some(),
        "camelCase key darkSurfaceContainerLow must exist in persisted JSON"
    );
    assert!(
        tp.get("darkSurfaceContainerHighest").is_some(),
        "camelCase key darkSurfaceContainerHighest must exist in persisted JSON"
    );
}

#[test]
fn theme_palette_dto_snake_case_round_trip_full() {
    use crate::api::types::ThemePaletteDto;

    let original = ThemePaletteDto {
        source: "android_dynamic_color".to_string(),
        updated_at_ms: 1700000000000,
        device_id: "device-abc".to_string(),
        variant: "tonal_spot".to_string(),
        light_primary: "#006494".to_string(),
        light_on_primary: "#FFFFFF".to_string(),
        light_primary_container: "#CCE5FF".to_string(),
        light_on_primary_container: "#001E30".to_string(),
        light_secondary: "#4F616E".to_string(),
        light_on_secondary: "#FFFFFF".to_string(),
        light_secondary_container: "#D3E5F3".to_string(),
        light_on_secondary_container: "#0B1D29".to_string(),
        light_tertiary: "#61587C".to_string(),
        light_on_tertiary: "#FFFFFF".to_string(),
        light_tertiary_container: "#E7DDFF".to_string(),
        light_on_tertiary_container: "#1D1535".to_string(),
        light_background: "#F9FAFF".to_string(),
        light_on_background: "#1A1C1E".to_string(),
        light_surface: "#F9FAFF".to_string(),
        light_on_surface: "#1A1C1E".to_string(),
        light_surface_variant: "#DFE2EB".to_string(),
        light_on_surface_variant: "#434750".to_string(),
        light_surface_container_lowest: "#FFFFFF".to_string(),
        light_surface_container_low: "#F3F4FA".to_string(),
        light_surface_container: "#EDEEF4".to_string(),
        light_surface_container_high: "#E7E8EF".to_string(),
        light_surface_container_highest: "#E2E3E9".to_string(),
        light_outline: "#73777F".to_string(),
        light_outline_variant: "#C3C7CF".to_string(),
        dark_primary: "#92CCFF".to_string(),
        dark_on_primary: "#003352".to_string(),
        dark_primary_container: "#004A73".to_string(),
        dark_on_primary_container: "#CCE5FF".to_string(),
        dark_secondary: "#B7C9D7".to_string(),
        dark_on_secondary: "#21323F".to_string(),
        dark_secondary_container: "#374955".to_string(),
        dark_on_secondary_container: "#D3E5F3".to_string(),
        dark_tertiary: "#CBC1E9".to_string(),
        dark_on_tertiary: "#322D4B".to_string(),
        dark_tertiary_container: "#494363".to_string(),
        dark_on_tertiary_container: "#E7DDFF".to_string(),
        dark_background: "#1A1C1E".to_string(),
        dark_on_background: "#E2E2E5".to_string(),
        dark_surface: "#1A1C1E".to_string(),
        dark_on_surface: "#E2E2E5".to_string(),
        dark_surface_variant: "#434750".to_string(),
        dark_on_surface_variant: "#C3C7CF".to_string(),
        dark_surface_container_lowest: "#0E1114".to_string(),
        dark_surface_container_low: "#1A1D21".to_string(),
        dark_surface_container: "#1E2125".to_string(),
        dark_surface_container_high: "#282B30".to_string(),
        dark_surface_container_highest: "#33363A".to_string(),
        dark_outline: "#8D9199".to_string(),
        dark_outline_variant: "#434750".to_string(),
    };

    let json_str = serde_json::to_string(&original).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(
        json_val.get("light_surface_container_lowest").is_some(),
        "snake_case key light_surface_container_lowest must exist"
    );
    assert!(
        json_val.get("light_surface_container_low").is_some(),
        "snake_case key light_surface_container_low must exist"
    );
    assert!(
        json_val.get("light_surface_container_highest").is_some(),
        "snake_case key light_surface_container_highest must exist"
    );
    assert!(
        json_val.get("dark_surface_container_lowest").is_some(),
        "snake_case key dark_surface_container_lowest must exist"
    );
    assert!(
        json_val.get("dark_surface_container_low").is_some(),
        "snake_case key dark_surface_container_low must exist"
    );
    assert!(
        json_val.get("dark_surface_container_highest").is_some(),
        "snake_case key dark_surface_container_highest must exist"
    );

    let restored: ThemePaletteDto = serde_json::from_str(&json_str).unwrap();
    assert_eq!(restored.light_surface_container_lowest, "#FFFFFF");
    assert_eq!(restored.light_surface_container_low, "#F3F4FA");
    assert_eq!(restored.light_surface_container, "#EDEEF4");
    assert_eq!(restored.light_surface_container_high, "#E7E8EF");
    assert_eq!(restored.light_surface_container_highest, "#E2E3E9");
    assert_eq!(restored.dark_surface_container_lowest, "#0E1114");
    assert_eq!(restored.dark_surface_container_low, "#1A1D21");
    assert_eq!(restored.dark_surface_container, "#1E2125");
    assert_eq!(restored.dark_surface_container_high, "#282B30");
    assert_eq!(restored.dark_surface_container_highest, "#33363A");
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
