//! # 思维导图数据类型定义模块
//!
//! 本模块定义了思维导图功能的核心数据结构，包括节点类型、边类型、
//! 图形结构以及相关的数据模型。
//!
//! ## 主要类型
//!
//! ### 节点类型 (`MindMapNodeKind`)
//! - `Project`：项目节点，表示整个写作项目
//! - `Volume`：卷节点，表示项目中的卷
//! - `Chapter`：章节节点，表示卷中的章节
//! - `TextAnchor`：文本锚点节点，关联到章节中的特定文本
//! - `Character`：角色
//! - `Event`：事件
//! - `Location`：地点
//! - `Item`：物品
//! - `Concept`：概念
//! - `Theme`：主题
//! - `Note`：笔记
//! - `Organization`：组织
//! - `Timeline`：时间线
//! - `Plot`：情节
//! - `Foreshadowing`：伏笔
//! - `Custom`：自定义类型
//!
//! ### 边类型 (`MindMapEdgeKind`)
//! - `Contains`：包含关系（如项目包含卷，卷包含章节）
//! - `References`：引用关系
//! - `AppearsIn`：出现在
//! - `Causes`：因果关系
//! - `RelatedTo`：相关
//! - `LocatedAt`：位于
//! - `CharacterRelation`：角色关系
//! - `Timeline`：时间关系
//! - `Foreshadows`：伏笔关系
//! - `Resolves`：解决关系
//! - `DependsOn`：依赖关系
//! - `ConflictsWith`：冲突关系
//! - `Custom`：自定义关系
//!
//! ## 数据结构
//! - `MindMapGraph`：思维导图图形数据，包含节点、边、锚点和链接
//! - `MindMapGraphNode`：思维导图节点
//! - `MindMapGraphEdge`：思维导图边
//!
//! ## 依赖关系
//! - `serde`：JSON序列化/反序列化支持
//! - `crate::mind_map::anchor`：锚点和链接类型
//!
//! ## 使用场景
//! - 定义思维导图的数据模型
//! - 支持JSON格式的数据交换
//! - 为前端提供类型安全的接口
//! - 支持多种故事元素类型和关系类型

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MindMapNodeKind {
    Project,
    Volume,
    Chapter,
    TextAnchor,
    Character,
    Event,
    Location,
    Item,
    Concept,
    Theme,
    Note,
    Organization,
    Timeline,
    Plot,
    Foreshadowing,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MindMapEdgeKind {
    Contains,
    References,
    AppearsIn,
    Causes,
    RelatedTo,
    LocatedAt,
    CharacterRelation,
    Timeline,
    Foreshadows,
    Resolves,
    DependsOn,
    ConflictsWith,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraph {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub nodes: Vec<MindMapGraphNode>,
    pub edges: Vec<MindMapGraphEdge>,
    pub anchors: Vec<crate::mind_map::anchor::MindMapAnchor>,
    pub links: Vec<crate::mind_map::anchor::MindMapLink>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphNode {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKind,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: MindMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub created_at: u64,
    pub updated_at: u64,
}
