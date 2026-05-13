use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SettingValue {
    Toggle {
        value: bool,
    },
    Slider {
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    TextInput {
        value: String,
        placeholder: String,
    },
    Dropdown {
        value: String,
        options: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingNode {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub value: Option<SettingValue>,
    pub children: Vec<SettingNode>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsTree {
    pub schema_version: u32,
    pub root: SettingNode,
}
