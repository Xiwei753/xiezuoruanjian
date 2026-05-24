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
    GraphMindMap,
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
    pub title: String,
    pub description: String,
    pub category: SettingCategory,
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
                title: "字体大小".to_string(),
                description: "编辑器正文的字体大小".to_string(),
                category: SettingCategory::Editor,
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
                title: "Git 同步 Token".to_string(),
                description: "用于 Git 同步的凭证".to_string(),
                category: SettingCategory::Sync,
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
                    title: "DeepSeek API 密钥".to_string(),
                    description: "用于调用 DeepSeek 服务的凭证".to_string(),
                    category: SettingCategory::DeepSeekAi,
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
    }
}
