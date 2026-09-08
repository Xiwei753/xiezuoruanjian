//! # 设置页 about section（#628 拆分）

use super::{PlatformVisibility, SettingItemDef, SettingSectionDef};

/// 构建 about section（order: 70）。
pub(super) fn build_about_section() -> SettingSectionDef {
    SettingSectionDef {
        id: "about".into(),
        title_key: "settings.section.about".into(),
        order: 70,
        platform_visibility: PlatformVisibility::All,
        items: vec![
            SettingItemDef {
                id: "data_root_path".into(),
                title_key: "settings.item.data_root_path".into(),
                description_key: None,
                kind: super::SettingControlKind::TextPlain,
                value_key: "data_root_path".into(),
                order: 1,
                platform_visibility: PlatformVisibility::All,
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
                kind: super::SettingControlKind::TextPlain,
                value_key: "version".into(),
                order: 2,
                platform_visibility: PlatformVisibility::All,
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
                kind: super::SettingControlKind::Action,
                value_key: "action_registry".into(),
                order: 3,
                platform_visibility: PlatformVisibility::All,
                min_value: None,
                max_value: None,
                step_value: None,
                select_options: None,
                requires_restart: false,
                is_experimental: false,
            },
        ],
    }
}
