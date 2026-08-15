//! # 设置页展示契约（#628 拆分）
//!
//! 定义设置页的 section / item 顺序、控件类型、平台可见性等，
//! 作为三端（Android / Qt / 鸿蒙）设置页的统一契约。
//! 客户端只负责渲染，不允许自行决定设置项顺序或分组。
//!
//! ## 子模块
//!
//! - [`appearance`]：外观 section（order 10）。
//! - [`editor`]：编辑器 section（order 20）。
//! - [`save`]：保存 section（order 30）。
//! - [`sync`]：同步 section（order 40）。
//! - [`ai`]：AI section（order 50）。
//! - [`about`]：关于 section（order 70）。
//!
//! `stats`（order 60）当前是空 section，直接在 [`default_settings_presentation`]
//! 中组装，不单独建空文件（#628）。

pub mod about;
pub mod ai;
pub mod appearance;
pub mod editor;
pub mod save;
pub mod sync;

#[cfg(test)]
mod tests;

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

/// 生成默认的设置页展示契约。
///
/// section 顺序：appearance(10) → editor(20) → save(30) → sync(40) →
/// ai(50) → stats(60, 空) → about(70)。
pub fn default_settings_presentation() -> SettingsPresentation {
    SettingsPresentation {
        sections: vec![
            appearance::build_appearance_section(),
            editor::build_editor_section(),
            save::build_save_section(),
            sync::build_sync_section(),
            ai::build_ai_section(),
            // stats 当前是空 section，直接组装（#628）。
            SettingSectionDef {
                id: "stats".into(),
                title_key: "settings.section.stats".into(),
                order: 60,
                platform_visibility: PlatformVisibility::All,
                items: vec![],
            },
            about::build_about_section(),
        ],
    }
}
