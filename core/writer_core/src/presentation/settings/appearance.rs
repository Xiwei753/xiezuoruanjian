//! # 设置页 appearance section（#628 拆分）

use super::{PlatformVisibility, SettingItemDef, SettingSectionDef};

/// 构建 appearance section（order: 10）。
pub(super) fn build_appearance_section() -> SettingSectionDef {
    SettingSectionDef {
        id: "appearance".into(),
        title_key: "settings.section.appearance".into(),
        order: 10,
        platform_visibility: PlatformVisibility::All,
        items: vec![
            SettingItemDef {
                id: "theme_mode".into(),
                title_key: "settings.item.theme_mode".into(),
                description_key: None,
                kind: super::SettingControlKind::Select,
                value_key: "theme_mode".into(),
                order: 1,
                platform_visibility: PlatformVisibility::All,
                min_value: None,
                max_value: None,
                step_value: None,
                select_options: Some(vec![
                    super::SelectOption {
                        value: "system".into(),
                        label_key: "settings.theme.system".into(),
                        order: 1,
                    },
                    super::SelectOption {
                        value: "light".into(),
                        label_key: "settings.theme.light".into(),
                        order: 2,
                    },
                    super::SelectOption {
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
                kind: super::SettingControlKind::Slider,
                value_key: "editor_font_size".into(),
                order: 2,
                platform_visibility: PlatformVisibility::All,
                min_value: Some(12.0),
                max_value: Some(72.0), // Linux_qt 大屏需要更大字号范围，取三端最大值
                step_value: Some(1.0),
                select_options: None,
                requires_restart: false,
                is_experimental: false,
            },
            SettingItemDef {
                id: "editor_line_spacing_multiplier".into(),
                title_key: "settings.item.editor_line_spacing_multiplier".into(),
                description_key: None,
                kind: super::SettingControlKind::Slider,
                value_key: "editor_line_spacing_multiplier".into(),
                order: 3,
                platform_visibility: PlatformVisibility::All,
                min_value: Some(1.0),
                max_value: Some(3.0),
                step_value: Some(0.1),
                select_options: None,
                requires_restart: false,
                is_experimental: false,
            },
        ],
    }
}
