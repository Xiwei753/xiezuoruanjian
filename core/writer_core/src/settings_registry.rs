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
