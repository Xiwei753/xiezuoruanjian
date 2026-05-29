//! # 星图数据类型定义模块
//!
//! 本模块定义了星图功能的核心数据结构，包括节点类型、边类型、图形结构、
//! 布局信息以及用于更新操作的补丁结构。
//!
//! ## 主要类型
//!
//! ### 节点类型 (`StarMapNodeKind`)
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
//! - `Chapter`：章节
//! - `Custom`：自定义类型
//!
//! ### 边类型 (`StarMapEdgeKind`)
//! - `Contains`：包含关系
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
//! ### 布局类型 (`StarMapLayoutKind`)
//! - `Freeform`：自由布局
//! - `AutoRadial`：自动放射状布局
//! - `Custom`：自定义布局
//!
//! ## 数据结构
//! - `StarMapGraph`：星图图形数据，包含节点和边的集合
//! - `StarMapNode`：星图节点，表示故事元素
//! - `StarMapEdge`：星图边，表示元素之间的关系
//! - `StarMapLayout`：布局信息，包含节点位置
//! - `StarMapLayoutNode`：单个节点的布局信息
//! - `StarMapNodePatch`：节点更新补丁
//! - `StarMapEdgePatch`：边更新补丁
//!
//! ## 依赖关系
//! - `serde`：JSON序列化/反序列化支持
//!
//! ## 使用场景
//! - 定义星图的数据模型
//! - 支持JSON格式的数据交换
//! - 为前端提供类型安全的接口

use crate::starmap::semantic::{
    StarMapAnchor, StarMapDisplayPolicy, StarMapNodeContent, StarMapOpenBehavior, StarMapPortal,
    StarMapProvenance,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapNodeKind {
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
    Chapter,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapEdgeKind {
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
pub struct StarMapGraph {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNode>,
    pub edges: Vec<StarMapEdge>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNode {
    pub id: String,
    pub title: String,
    pub kind: StarMapNodeKind,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content: StarMapNodeContent,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchor>,
    #[serde(default)]
    pub portal: Option<StarMapPortal>,
    #[serde(default)]
    pub display_policy: StarMapDisplayPolicy,
    #[serde(default)]
    pub open_behavior: StarMapOpenBehavior,
    #[serde(default)]
    pub provenance: StarMapProvenance,

    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapLayoutKind {
    Freeform,
    AutoRadial,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayout {
    pub kind: StarMapLayoutKind,
    pub nodes: Vec<StarMapLayoutNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutNode {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,

    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub depth: f32,
    #[serde(default)]
    pub focus_weight: f32,
    #[serde(default)]
    pub orbit_group: Option<String>,
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatch {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKind>,
    pub payload: Option<Option<serde_json::Value>>,
    pub tags: Option<Vec<String>>,
    pub content: Option<StarMapNodeContent>,
    pub anchors: Option<Vec<StarMapAnchor>>,
    pub portal: Option<Option<StarMapPortal>>,
    pub display_policy: Option<StarMapDisplayPolicy>,
    pub open_behavior: Option<StarMapOpenBehavior>,
    pub provenance: Option<StarMapProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatch {
    pub kind: Option<StarMapEdgeKind>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<serde_json::Value>>,
}

impl Default for StarMapGraph {
    fn default() -> Self {
        Self {
            schema_version: 1,
            id: String::new(),
            starmap_id: String::new(),
            title: String::new(),
            nodes: vec![],
            edges: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
}

impl Default for StarMapLayout {
    fn default() -> Self {
        Self {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![],
        }
    }
}
