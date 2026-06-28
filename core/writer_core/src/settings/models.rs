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
