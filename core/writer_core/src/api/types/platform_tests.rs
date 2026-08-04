use super::platform::*;
use serde_json::json;

#[test]
fn test_platform_dto_serialization_roundtrip() {
    let dto = PlatformDto::Android;
    let serialized = serde_json::to_value(&dto).unwrap();
    assert_eq!(serialized, json!("Android"));
    let deserialized: PlatformDto = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_platform_init_dto_serialization_roundtrip() {
    let dto = PlatformInitDto {
        platform: PlatformDto::Harmony,
        app_data_dir: "/data".to_string(),
        cache_dir: "/cache".to_string(),
        log_dir: "/log".to_string(),
        no_backup_dir: Some("/no_backup".to_string()),
        device_id: "dev-1".to_string(),
        app_version: "1.0.0".to_string(),
        locale: "zh-CN".to_string(),
        timezone: "Asia/Shanghai".to_string(),
        is_connected: true,
        is_metered: false,
        proxy_host: None,
        proxy_port: None,
    };
    let json = serde_json::to_value(&dto).unwrap();
    let deserialized: PlatformInitDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_window_metrics_dto_serialization_roundtrip() {
    let dto = WindowMetricsDto {
        width_dp: 1920.0,
        height_dp: 1080.0,
        safe_top_dp: 24.0,
        safe_bottom_dp: 48.0,
        keyboard_visible: false,
        fold_feature: FoldFeatureInfoDto {
            state: FoldStateDto::Flat,
            orientation: FoldOrientationDto::Vertical,
            occlusion: FoldOcclusionDto::None,
            is_separating: false,
            bounds_left_vp: 0.0,
            bounds_top_vp: 0.0,
            bounds_right_vp: 0.0,
            bounds_bottom_vp: 0.0,
        },
        orientation: OrientationDto::Landscape,
        pointer: PointerKindDto::Mouse,
    };
    let json = serde_json::to_value(&dto).unwrap();
    let deserialized: WindowMetricsDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}

#[test]
fn test_layout_plan_dto_serialization_roundtrip() {
    let dto = LayoutPlanDto {
        width_class: WidthClassDto::Expanded,
        height_class: HeightClassDto::Medium,
        shell_mode: ShellModeDto::ThreePane,
        editor_mode: EditorModeDto::CenteredPaper,
        navigation_mode: NavigationModeDto::ListDetail,
        navigation_presentation: NavigationPresentationDto::PermanentDrawer,
        workspace_pane_mode: WorkspacePaneModeDto::ThreePane,
        visible_pane_roles: VisiblePaneRolesDto {
            show_project_list: true,
            show_chapter_tree: true,
            show_editor: true,
            show_supporting: false,
        },
        content_max_width_dp: 1200.0,
        page_padding_dp: 24.0,
        grid_columns: 12,
        show_bottom_bar: false,
        list_pane_width: PaneWidthConstraintDto {
            min_dp: 200.0,
            preferred_dp: 300.0,
            max_dp: 400.0,
        },
        editor_content_max_width_dp: 800.0,
        primary_pane_min_dp: 400.0,
        primary_pane_preferred_dp: 800.0,
        primary_pane_max_dp: 1200.0,
        supporting_pane_mode: None,
        avoid_regions: vec![AvoidRegionDto {
            left_dp: 0.0,
            top_dp: 24.0,
            right_dp: 0.0,
            bottom_dp: 48.0,
            kind: AvoidRegionKindDto::WindowInset,
        }],
    };
    let json = serde_json::to_value(&dto).unwrap();
    let deserialized: LayoutPlanDto = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, dto);
}
