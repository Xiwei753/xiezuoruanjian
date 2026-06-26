//! # 设置页展示契约
//!
//! 定义设置页的 section / item 顺序、控件类型、平台可见性等，
//! 作为三端（Android / Qt / 鸿蒙）设置页的统一契约。
//! 客户端只负责渲染，不允许自行决定设置项顺序或分组。

use serde::{Deserialize, Serialize};

/// 设置项的 UI 控件类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingControlKind {
    Switch,
    Slider,
    Select,
    TextSecret,
    TextPlain,
    Action,
}

/// 设置项的平台可见性
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlatformVisibility {
    All,
    Android,
    Desktop,
    Harmony,
}

/// 设置项定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingItemDef {
    pub id: String,
    pub title_key: String,
    pub description_key: Option<String>,
    pub kind: SettingControlKind,
    pub value_key: String,
    pub order: u32,
    pub platform_visibility: PlatformVisibility,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub step_value: Option<f64>,
    pub select_options: Option<Vec<SelectOption>>,
    pub requires_restart: bool,
    pub is_experimental: bool,
}

/// 下拉选项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub value: String,
    pub label_key: String,
    pub order: u32,
}

/// 设置分组定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingSectionDef {
    pub id: String,
    pub title_key: String,
    pub order: u32,
    pub platform_visibility: PlatformVisibility,
    pub items: Vec<SettingItemDef>,
}

/// 设置页展示契约（整个设置页的完整 schema）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPresentation {
    pub sections: Vec<SettingSectionDef>,
}

/// 生成默认的设置页展示契约
pub fn default_settings_presentation() -> SettingsPresentation {
    use PlatformVisibility::*;

    SettingsPresentation {
        sections: vec![
            // ── appearance (order: 10) ──
            SettingSectionDef {
                id: "appearance".into(),
                title_key: "settings.section.appearance".into(),
                order: 10,
                platform_visibility: All,
                items: vec![
                    SettingItemDef {
                        id: "theme_mode".into(),
                        title_key: "settings.item.theme_mode".into(),
                        description_key: None,
                        kind: SettingControlKind::Select,
                        value_key: "theme_mode".into(),
                        order: 1,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: Some(vec![
                            SelectOption {
                                value: "system".into(),
                                label_key: "settings.theme.system".into(),
                                order: 1,
                            },
                            SelectOption {
                                value: "light".into(),
                                label_key: "settings.theme.light".into(),
                                order: 2,
                            },
                            SelectOption {
                                value: "dark".into(),
                                label_key: "settings.theme.dark".into(),
                                order: 3,
                            },
                        ]),
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "editor_font_size".into(),
                        title_key: "settings.item.editor_font_size".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "editor_font_size".into(),
                        order: 2,
                        platform_visibility: All,
                        min_value: Some(12.0),
                        max_value: Some(72.0), // Desktop 大屏需要更大字号范围，取三端最大值
                        step_value: Some(1.0),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "editor_line_spacing_multiplier".into(),
                        title_key: "settings.item.editor_line_spacing_multiplier".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "editor_line_spacing_multiplier".into(),
                        order: 3,
                        platform_visibility: All,
                        min_value: Some(1.0),
                        max_value: Some(3.0),
                        step_value: Some(0.1),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                ],
            },
            // ── editor (order: 20) ──
            SettingSectionDef {
                id: "editor".into(),
                title_key: "settings.section.editor".into(),
                order: 20,
                platform_visibility: All,
                items: vec![
                    SettingItemDef {
                        id: "auto_indent_enabled".into(),
                        title_key: "settings.item.auto_indent_enabled".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "auto_indent_enabled".into(),
                        order: 1,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "auto_indent_width".into(),
                        title_key: "settings.item.auto_indent_width".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "auto_indent_width".into(),
                        order: 2,
                        platform_visibility: All,
                        min_value: Some(0.0),
                        max_value: Some(8.0),
                        step_value: Some(0.5),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "typing_animation_enabled".into(),
                        title_key: "settings.item.typing_animation_enabled".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "typing_animation_enabled".into(),
                        order: 3,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "typing_animation_duration_ms".into(),
                        title_key: "settings.item.typing_animation_duration_ms".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "typing_animation_duration_ms".into(),
                        order: 4,
                        platform_visibility: All,
                        min_value: Some(0.0),
                        max_value: Some(1000.0),
                        step_value: Some(10.0),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "smooth_cursor_enabled".into(),
                        title_key: "settings.item.smooth_cursor_enabled".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "smooth_cursor_enabled".into(),
                        order: 5,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "smooth_cursor_duration_ms".into(),
                        title_key: "settings.item.smooth_cursor_duration_ms".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "smooth_cursor_duration_ms".into(),
                        order: 6,
                        platform_visibility: All,
                        min_value: Some(0.0),
                        max_value: Some(1000.0),
                        step_value: Some(10.0),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                ],
            },
            // ── save (order: 30) ──
            SettingSectionDef {
                id: "save".into(),
                title_key: "settings.section.save".into(),
                order: 30,
                platform_visibility: All,
                items: vec![
                    SettingItemDef {
                        id: "auto_save_enabled".into(),
                        title_key: "settings.item.auto_save_enabled".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "auto_save_enabled".into(),
                        order: 1,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "auto_save_delay_ms".into(),
                        title_key: "settings.item.auto_save_delay_ms".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "auto_save_delay_ms".into(),
                        order: 2,
                        platform_visibility: All,
                        min_value: Some(1.0),
                        max_value: Some(10.0),
                        step_value: Some(1.0),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                ],
            },
            // ── sync (order: 40) ──
            SettingSectionDef {
                id: "sync".into(),
                title_key: "settings.section.sync".into(),
                order: 40,
                platform_visibility: All,
                items: vec![
                    SettingItemDef {
                        id: "sync_enabled".into(),
                        title_key: "settings.item.sync_enabled".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "sync_enabled".into(),
                        order: 1,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "github_repo".into(),
                        title_key: "settings.item.github_repo".into(),
                        description_key: None,
                        kind: SettingControlKind::TextPlain,
                        value_key: "github_repo".into(),
                        order: 2,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "branch".into(),
                        title_key: "settings.item.branch".into(),
                        description_key: None,
                        kind: SettingControlKind::TextPlain,
                        value_key: "branch".into(),
                        order: 3,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "token".into(),
                        title_key: "settings.item.token".into(),
                        description_key: None,
                        kind: SettingControlKind::TextSecret,
                        value_key: "token".into(),
                        order: 4,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "auto_sync".into(),
                        title_key: "settings.item.auto_sync".into(),
                        description_key: None,
                        kind: SettingControlKind::Switch,
                        value_key: "auto_sync".into(),
                        order: 5,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "sync_interval_seconds".into(),
                        title_key: "settings.item.sync_interval_seconds".into(),
                        description_key: None,
                        kind: SettingControlKind::Slider,
                        value_key: "sync_interval_seconds".into(),
                        order: 6,
                        platform_visibility: All,
                        min_value: Some(60.0),
                        max_value: Some(3600.0),
                        step_value: Some(60.0),
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "sync_dry_run".into(),
                        title_key: "settings.item.sync_dry_run".into(),
                        description_key: None,
                        kind: SettingControlKind::Action,
                        value_key: "sync_dry_run".into(),
                        order: 7,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "sync_test_connection".into(),
                        title_key: "settings.item.sync_test_connection".into(),
                        description_key: None,
                        kind: SettingControlKind::Action,
                        value_key: "sync_test_connection".into(),
                        order: 8,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "sync_now".into(),
                        title_key: "settings.item.sync_now".into(),
                        description_key: None,
                        kind: SettingControlKind::Action,
                        value_key: "sync_now".into(),
                        order: 9,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                ],
            },
            // ── ai (order: 50) ──
            SettingSectionDef {
                id: "ai".into(),
                title_key: "settings.section.ai".into(),
                order: 50,
                platform_visibility: All,
                items: vec![SettingItemDef {
                    id: "ai_enabled".into(),
                    title_key: "settings.item.ai_enabled".into(),
                    description_key: None,
                    kind: SettingControlKind::Switch,
                    value_key: "ai_enabled".into(),
                    order: 1,
                    platform_visibility: All,
                    min_value: None,
                    max_value: None,
                    step_value: None,
                    select_options: None,
                    requires_restart: false,
                    is_experimental: true,
                }],
            },
            // ── stats (order: 60) ── 占位
            SettingSectionDef {
                id: "stats".into(),
                title_key: "settings.section.stats".into(),
                order: 60,
                platform_visibility: All,
                items: vec![],
            },
            // ── about (order: 70) ──
            SettingSectionDef {
                id: "about".into(),
                title_key: "settings.section.about".into(),
                order: 70,
                platform_visibility: All,
                items: vec![
                    SettingItemDef {
                        id: "workspace_path".into(),
                        title_key: "settings.item.workspace_path".into(),
                        description_key: None,
                        kind: SettingControlKind::TextPlain,
                        value_key: "workspace_path".into(),
                        order: 1,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "version".into(),
                        title_key: "settings.item.version".into(),
                        description_key: None,
                        kind: SettingControlKind::TextPlain,
                        value_key: "version".into(),
                        order: 2,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                    SettingItemDef {
                        id: "action_registry".into(),
                        title_key: "settings.item.action_registry".into(),
                        description_key: None,
                        kind: SettingControlKind::Action,
                        value_key: "action_registry".into(),
                        order: 3,
                        platform_visibility: All,
                        min_value: None,
                        max_value: None,
                        step_value: None,
                        select_options: None,
                        requires_restart: false,
                        is_experimental: false,
                    },
                ],
            },
        ],
    }
}

// ────────────────────────────── 测试 ──────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_order() {
        let presentation = default_settings_presentation();
        let section_ids: Vec<&str> = presentation
            .sections
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(
            section_ids,
            vec![
                "appearance",
                "editor",
                "save",
                "sync",
                "ai",
                "stats",
                "about"
            ],
            "section 顺序必须是 appearance→editor→save→sync→ai→stats→about"
        );

        // 同时验证 order 字段单调递增
        let orders: Vec<u32> = presentation.sections.iter().map(|s| s.order).collect();
        assert_eq!(orders, vec![10, 20, 30, 40, 50, 60, 70]);
    }

    #[test]
    fn test_theme_options_order() {
        let presentation = default_settings_presentation();
        let appearance = &presentation.sections[0];
        assert_eq!(appearance.id, "appearance");

        let theme_item = appearance
            .items
            .iter()
            .find(|i| i.id == "theme_mode")
            .expect("theme_mode item must exist");

        let options = theme_item
            .select_options
            .as_ref()
            .expect("theme_mode must have select options");
        let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(
            values,
            vec!["system", "light", "dark"],
            "theme_mode 选项顺序必须是 system→light→dark"
        );
    }

    #[test]
    fn test_serialization() {
        let presentation = default_settings_presentation();

        // 序列化为 JSON
        let json = serde_json::to_string(&presentation).expect("serialization must succeed");

        // 反序列化回来
        let deserialized: SettingsPresentation =
            serde_json::from_str(&json).expect("deserialization must succeed");

        // 验证 roundtrip 一致
        assert_eq!(presentation.sections.len(), deserialized.sections.len());

        // 验证 camelCase 序列化（struct 字段）
        assert!(
            json.contains("\"titleKey\""),
            "struct 字段应序列化为 camelCase"
        );
        assert!(
            json.contains("\"labelKey\""),
            "SelectOption 字段应序列化为 camelCase"
        );
        assert!(
            json.contains("\"platformVisibility\""),
            "struct 字段应序列化为 camelCase"
        );

        // 验证 enum 值保持 PascalCase
        assert!(json.contains("\"Switch\""), "enum 值应保持 PascalCase");
        assert!(json.contains("\"Slider\""), "enum 值应保持 PascalCase");
        assert!(json.contains("\"All\""), "enum 值应保持 PascalCase");
    }

    #[test]
    fn test_all_items_have_valid_ids() {
        let presentation = default_settings_presentation();
        let mut seen_ids = std::collections::HashSet::new();

        for section in &presentation.sections {
            assert!(!section.id.is_empty(), "section id must not be empty");

            for item in &section.items {
                assert!(
                    !item.id.is_empty(),
                    "item id in section '{}' must not be empty",
                    section.id
                );
                assert!(
                    seen_ids.insert(item.id.clone()),
                    "duplicate item id: '{}'",
                    item.id
                );
            }
        }
    }

    #[test]
    fn test_section_item_ids() {
        let presentation = default_settings_presentation();

        // appearance section items
        let appearance = &presentation.sections[0];
        let item_ids: Vec<&str> = appearance.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            item_ids,
            vec![
                "theme_mode",
                "editor_font_size",
                "editor_line_spacing_multiplier"
            ]
        );

        // editor section items
        let editor = &presentation.sections[1];
        let item_ids: Vec<&str> = editor.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            item_ids,
            vec![
                "auto_indent_enabled",
                "auto_indent_width",
                "typing_animation_enabled",
                "typing_animation_duration_ms",
                "smooth_cursor_enabled",
                "smooth_cursor_duration_ms"
            ]
        );

        // save section items
        let save = &presentation.sections[2];
        let item_ids: Vec<&str> = save.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(item_ids, vec!["auto_save_enabled", "auto_save_delay_ms"]);

        // sync section items
        let sync = &presentation.sections[3];
        let item_ids: Vec<&str> = sync.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            item_ids,
            vec![
                "sync_enabled",
                "github_repo",
                "branch",
                "token",
                "auto_sync",
                "sync_interval_seconds",
                "sync_dry_run",
                "sync_test_connection",
                "sync_now"
            ]
        );

        // ai section items
        let ai = &presentation.sections[4];
        let item_ids: Vec<&str> = ai.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(item_ids, vec!["ai_enabled"]);

        // stats section items (empty)
        let stats = &presentation.sections[5];
        assert!(stats.items.is_empty());

        // about section items
        let about = &presentation.sections[6];
        let item_ids: Vec<&str> = about.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            item_ids,
            vec!["workspace_path", "version", "action_registry"]
        );
    }

    #[test]
    fn test_settings_presentation_json_ffi() {
        let presentation = default_settings_presentation();
        let json = serde_json::to_string(&presentation).expect("serialization must succeed");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");

        // 验证 JSON 结构可被客户端消费
        assert!(parsed.get("sections").is_some());
        let sections = parsed.get("sections").unwrap().as_array().unwrap();
        assert_eq!(sections.len(), 7);

        // 验证第一个 section 的结构
        let first = &sections[0];
        assert_eq!(first.get("id").unwrap().as_str().unwrap(), "appearance");
        assert!(first.get("items").unwrap().as_array().unwrap().len() > 0);
    }
}
