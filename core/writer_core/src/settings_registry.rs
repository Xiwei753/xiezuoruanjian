//! # 设置注册表模块 (Settings Registry)
//!
//! 本模块实现了应用程序的设置注册和管理系统，用于统一管理所有可配置的设置项。
//!
//! ## 主要功能
//!
//! - **设置项定义**: 定义每个设置项的元数据（ID、标题、描述、类型等）
//! - **分类管理**: 按类别（编辑器、同步、AI 等）和子类别组织设置
//! - **值类型支持**: 支持布尔、字符串、数字、枚举等多种值类型
//! - **范围控制**: 定义数值型设置的最小值、最大值和步长
//! - **同步标记**: 标记哪些设置需要跨设备同步
//! - **敏感标记**: 标记敏感设置（如 API 密钥），防止意外泄露
//! - **UI 建议**: 提供 UI 控件建议（slider、switch、password 等）
//!
//! ## 设置类别
//!
//! - `Editor`: 编辑器相关设置
//! - `Sync`: 同步相关设置
//! - `DeepSeekAi`: AI 相关设置
//! - `ThemeAppearance`: 主题外观设置
//! - 其他...
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化
//!
//! ## 使用场景
//!
//! - 设置界面的动态生成
//! - 设置项的验证和持久化
//! - 跨设备设置同步
//! - AI 助手调用设置

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingCategory {
    Editor,
    AnimationPerformance,
    AutoSave,
    Sync,
    DeepSeekAi,
    ToolCalls,
    AutoCorrection,
    GraphStarMap,
    ThemeAppearance,
    PrivacySecurity,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingSubCategory {
    General,
    Appearance,
    Behavior,
    Advanced,
    Network,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingValueType {
    Boolean,
    String,
    Number,
    Enum(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingValue {
    Boolean(bool),
    String(String),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingItem {
    pub id: String,
    /// i18n key for the setting title (e.g. "settings.item.editor_font_size")
    #[serde(alias = "title")]
    pub title_key: String,
    /// Fallback display text when i18n is unavailable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_fallback: Option<String>,
    /// i18n key for the setting description (e.g. "settings.item.editor_font_size.desc")
    #[serde(alias = "description")]
    pub description_key: String,
    /// Fallback display text when i18n is unavailable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_fallback: Option<String>,
    /// i18n key for the setting category (e.g. "settings.category.editor")
    #[serde(alias = "category")]
    pub category_key: String,
    /// Fallback display text when i18n is unavailable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_fallback: Option<String>,
    pub sub_category: SettingSubCategory,
    pub value_type: SettingValueType,
    pub default_value: SettingValue,
    pub current_value: Option<SettingValue>,
    pub is_syncable: bool,
    pub is_sensitive: bool,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub step_value: Option<f64>,
    pub ui_control_suggestion: Option<String>,
    pub requires_restart: bool,
    pub is_experimental: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsRegistry {
    pub items: Vec<SettingItem>,
}

impl SettingsRegistry {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn default_registry() -> Self {
        #[allow(unused_mut)]
        let mut items = vec![
            SettingItem {
                id: "editor.font_size".to_string(),
                title_key: "settings.item.editor_font_size".to_string(),
                title_fallback: Some("字体大小".to_string()),
                description_key: "settings.item.editor_font_size.desc".to_string(),
                description_fallback: Some("编辑器正文的字体大小".to_string()),
                category_key: "settings.category.editor".to_string(),
                category_fallback: Some("编辑器".to_string()),
                sub_category: SettingSubCategory::Appearance,
                value_type: SettingValueType::Number,
                default_value: SettingValue::Number(16.0),
                current_value: None,
                is_syncable: true,
                is_sensitive: false,
                min_value: Some(10.0),
                max_value: Some(72.0),
                step_value: Some(1.0),
                ui_control_suggestion: Some("slider".to_string()),
                requires_restart: false,
                is_experimental: false,
            },
            SettingItem {
                id: "sync.git.token".to_string(),
                title_key: "settings.item.token".to_string(),
                title_fallback: Some("Git 同步 Token".to_string()),
                description_key: "settings.item.token.desc".to_string(),
                description_fallback: Some("用于 Git 同步的凭证".to_string()),
                category_key: "settings.category.sync".to_string(),
                category_fallback: Some("同步".to_string()),
                sub_category: SettingSubCategory::Network,
                value_type: SettingValueType::String,
                default_value: SettingValue::String("".to_string()),
                current_value: None,
                is_syncable: false,
                is_sensitive: true,
                min_value: None,
                max_value: None,
                step_value: None,
                ui_control_suggestion: Some("password".to_string()),
                requires_restart: false,
                is_experimental: false,
            },
        ];

        #[cfg(feature = "ai")]
        {
            items.insert(
                1,
                SettingItem {
                    id: "ai.deepseek.api_key".to_string(),
                    title_key: "settings.item.ai_api_key".to_string(),
                    title_fallback: Some("DeepSeek API 密钥".to_string()),
                    description_key: "settings.item.ai_api_key.desc".to_string(),
                    description_fallback: Some("用于调用 DeepSeek 服务的凭证".to_string()),
                    category_key: "settings.category.deepseek_ai".to_string(),
                    category_fallback: Some("DeepSeek AI".to_string()),
                    sub_category: SettingSubCategory::Network,
                    value_type: SettingValueType::String,
                    default_value: SettingValue::String("".to_string()),
                    current_value: None,
                    is_syncable: false,
                    is_sensitive: true,
                    min_value: None,
                    max_value: None,
                    step_value: None,
                    ui_control_suggestion: Some("password".to_string()),
                    requires_restart: false,
                    is_experimental: false,
                },
            );
        }

        Self { items }
    }
}

impl Default for SettingsRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_registry_default() {
        let registry = SettingsRegistry::default_registry();
        assert!(!registry.items.is_empty());

        let font_size_item = registry.items.iter().find(|i| i.id == "editor.font_size").unwrap();
        assert!(!font_size_item.title_key.is_empty());
        assert!(!font_size_item.description_key.is_empty());
        assert!(font_size_item.title_fallback.is_some());
        assert!(font_size_item.description_fallback.is_some());
        assert!(!font_size_item.category_key.is_empty());
        assert!(font_size_item.category_fallback.is_some());
    }

    #[test]
    fn test_setting_item_serialization_skip_fallback() {
        let item = SettingItem {
            id: "test".to_string(),
            title_key: "settings.item.test".to_string(),
            title_fallback: None,
            description_key: "settings.item.test.desc".to_string(),
            description_fallback: None,
            category_key: "settings.category.editor".to_string(),
            category_fallback: None,
            sub_category: SettingSubCategory::General,
            value_type: SettingValueType::Boolean,
            default_value: SettingValue::Boolean(true),
            current_value: None,
            is_syncable: false,
            is_sensitive: false,
            min_value: None,
            max_value: None,
            step_value: None,
            ui_control_suggestion: None,
            requires_restart: false,
            is_experimental: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("title_fallback"));
        assert!(!json.contains("description_fallback"));
        assert!(!json.contains("category_fallback"));
    }

    #[test]
    fn test_setting_item_deserialize_old_field_names() {
        let old_json = r#"{
            "id": "test",
            "title": "settings.item.test",
            "description": "settings.item.test.desc",
            "category": "Editor",
            "sub_category": "General",
            "value_type": "Boolean",
            "default_value": {"Boolean": true},
            "is_syncable": false,
            "is_sensitive": false,
            "requires_restart": false,
            "is_experimental": false
        }"#;
        let item: SettingItem = serde_json::from_str(old_json).unwrap();
        assert_eq!(item.title_key, "settings.item.test");
        assert_eq!(item.description_key, "settings.item.test.desc");
        assert_eq!(item.category_key, "Editor");
    }
}
