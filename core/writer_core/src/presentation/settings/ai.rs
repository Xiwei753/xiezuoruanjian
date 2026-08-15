//! # 设置页 ai section（#628 拆分）

use super::{PlatformVisibility, SettingItemDef, SettingSectionDef};

/// 构建 ai section（order: 50）。
pub(super) fn build_ai_section() -> SettingSectionDef {
    SettingSectionDef {
        id: "ai".into(),
        title_key: "settings.section.ai".into(),
        order: 50,
        platform_visibility: PlatformVisibility::All,
        items: vec![SettingItemDef {
            id: "ai_enabled".into(),
            title_key: "settings.item.ai_enabled".into(),
            description_key: None,
            kind: super::SettingControlKind::Switch,
            value_key: "ai_enabled".into(),
            order: 1,
            platform_visibility: PlatformVisibility::All,
            min_value: None,
            max_value: None,
            step_value: None,
            select_options: None,
            requires_restart: false,
            is_experimental: true,
        }],
    }
}
