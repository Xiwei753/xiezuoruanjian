//! # 设置页 save section（#628 拆分）

use super::{PlatformVisibility, SettingItemDef, SettingSectionDef};

/// 构建 save section（order: 30）。
pub(super) fn build_save_section() -> SettingSectionDef {
    SettingSectionDef {
        id: "save".into(),
        title_key: "settings.section.save".into(),
        order: 30,
        platform_visibility: PlatformVisibility::All,
        items: vec![
            SettingItemDef {
                id: "auto_save_enabled".into(),
                title_key: "settings.item.auto_save_enabled".into(),
                description_key: None,
                kind: super::SettingControlKind::Switch,
                value_key: "auto_save_enabled".into(),
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
                id: "auto_save_delay_ms".into(),
                title_key: "settings.item.auto_save_delay_ms".into(),
                description_key: None,
                kind: super::SettingControlKind::Slider,
                value_key: "auto_save_delay_ms".into(),
                order: 2,
                platform_visibility: PlatformVisibility::All,
                min_value: Some(1.0),
                max_value: Some(10.0),
                step_value: Some(1.0),
                select_options: None,
                requires_restart: false,
                is_experimental: false,
            },
        ],
    }
}
