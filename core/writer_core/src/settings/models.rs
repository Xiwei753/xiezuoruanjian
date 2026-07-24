//! # 设置数据模型模块
//!
//! 本模块定义了应用程序设置系统的数据结构，采用树形结构组织设置项，
//! 支持多种类型的设置值。
//!
//! ## 主要功能
//!
//! - **树形设置结构**: 使用 `SettingNode` 构建层级化的设置树
//! - **多类型设置值**: 支持开关、滑块、文本输入、下拉选择等多种设置类型
//! - **版本管理**: 通过 schema_version 支持设置格式的版本演进
//!
//! ## 核心结构
//!
//! - `SettingsTree`: 设置树的根节点，包含版本号和根设置节点
//! - `SettingNode`: 设置节点，支持嵌套子节点，形成树形结构
//! - `SettingValue`: 设置值枚举，支持 Toggle/Slider/TextInput/Dropdown 四种类型
//!
//! ## 依赖关系
//!
//! - `serde`: 序列化/反序列化支持，用于 JSON 配置文件的读写
//!
//! ## 使用场景
//!
//! - 应用程序配置管理
//! - 设置界面的数据绑定
//! - 设置的持久化存储和加载

use serde::{Deserialize, Serialize};

/// 设置值 — 支持四种类型的设置项。
///
/// - `Toggle`：布尔开关（如"启用动画"）
/// - `Slider`：数值滑块，含范围 `[min, max]` 和步长 `step`
/// - `TextInput`：文本输入，含占位提示 `placeholder`
/// - `Dropdown`：下拉选择，`value` 为当前选中项，`options` 为可选项列表
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

/// 设置节点 — 树形设置结构的基本单元。
///
/// `value = Some(...)` 时为叶子节点（可编辑的设置项），
/// `value = None` 时为分组节点（仅包含子节点，如"编辑器设置"分组）。
/// `children` 为空时表示叶子节点没有子项。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingNode {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub value: Option<SettingValue>,
    pub children: Vec<SettingNode>,
}

/// 设置树 — 应用程序设置的根容器。
///
/// `schema_version` 用于设置格式的版本演进：
/// - 版本 1：初始格式
/// - 新增版本时需在加载逻辑中实现向后兼容迁移
/// - 版本号不匹配时平台端应提示用户重置设置
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsTree {
    pub schema_version: u32,
    pub root: SettingNode,
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_value_serialization() {
        let toggle = SettingValue::Toggle { value: true };
        let json = serde_json::to_value(&toggle).unwrap();
        assert_eq!(json["type"], "toggle");
        assert_eq!(json["value"], true);

        let slider = SettingValue::Slider { value: 1.0, min: 0.0, max: 10.0, step: 0.1 };
        let json = serde_json::to_value(&slider).unwrap();
        assert_eq!(json["type"], "slider");
        assert_eq!(json["value"], 1.0);
        assert_eq!(json["min"], 0.0);
        assert_eq!(json["max"], 10.0);
        assert!((json["step"].as_f64().unwrap() - 0.1).abs() < 1e-6);

        let text_input = SettingValue::TextInput { value: "test".to_string(), placeholder: "ph".to_string() };
        let json = serde_json::to_value(&text_input).unwrap();
        assert_eq!(json["type"], "textInput");
        assert_eq!(json["value"], "test");
        assert_eq!(json["placeholder"], "ph");

        let dropdown = SettingValue::Dropdown { value: "A".to_string(), options: vec!["A".to_string(), "B".to_string()] };
        let json = serde_json::to_value(&dropdown).unwrap();
        assert_eq!(json["type"], "dropdown");
        assert_eq!(json["value"], "A");
        assert_eq!(json["options"][0], "A");
        assert_eq!(json["options"][1], "B");
    }

    #[test]
    fn test_setting_node_serialization() {
        let node = SettingNode {
            id: "node1".to_string(),
            title: "Node 1".to_string(),
            description: Some("Desc 1".to_string()),
            value: Some(SettingValue::Toggle { value: false }),
            children: vec![],
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["id"], "node1");
        assert_eq!(json["title"], "Node 1");
        assert_eq!(json["description"], "Desc 1");
        assert_eq!(json["value"]["type"], "toggle");
        assert_eq!(json["value"]["value"], false);
        assert!(json["children"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_settings_tree_serialization() {
        let tree = SettingsTree {
            schema_version: 2,
            root: SettingNode {
                id: "root".to_string(),
                title: "Root".to_string(),
                description: None,
                value: None,
                children: vec![],
            },
        };
        let json = serde_json::to_value(&tree).unwrap();
        assert_eq!(json["schemaVersion"], 2);
        assert_eq!(json["root"]["id"], "root");
        assert_eq!(json["root"]["title"], "Root");
        assert!(json["root"]["description"].is_null());
        assert!(json["root"]["value"].is_null());
        assert!(json["root"]["children"].as_array().unwrap().is_empty());
    }
}
