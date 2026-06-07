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
    #[serde(default)]
    pub embeds: Vec<StarMapEmbed>,
    #[serde(default)]
    pub links: Vec<StarMapLink>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEdgeEndpoint {
    Node {
        node_id: String,
    },
    Anchor {
        node_id: String,
        anchor_id: String,
    },
    Starmap,
    DeepTarget {
        target: crate::starmap::semantic::StarMapDeepTarget,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdge {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub from_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    #[serde(default)]
    pub to_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpoint>,
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpoint>,
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
    pub from_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    pub to_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    pub from_endpoint: Option<Option<StarMapEdgeEndpoint>>,
    pub to_endpoint: Option<Option<StarMapEdgeEndpoint>>,
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
            embeds: vec![],
            links: vec![],
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapViewport {
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
}

impl Default for StarMapViewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub z_index: i32,
    pub collapsed: bool,
}

impl Default for StarMapEmbedPlacement {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 200.0,
            scale: 1.0,
            z_index: 0,
            collapsed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedViewport {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for StarMapEmbedViewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
    pub open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
    pub placement: StarMapEmbedPlacement,
    pub target_viewport: StarMapEmbedViewport,
    pub source_node_id: Option<String>,
    pub host_endpoint: Option<StarMapEndpoint>,
    pub provenance: crate::starmap::semantic::StarMapProvenance,
    pub created_at: u64,
    pub updated_at: u64,
}

impl<'de> Deserialize<'de> for StarMapEmbed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            instance_id: String,
            target_starmap_id: String,
            label: Option<String>,
            #[serde(default)]
            display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
            #[serde(default)]
            open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
            placement: Option<StarMapEmbedPlacement>,
            target_viewport: Option<StarMapEmbedViewport>,
            viewport: Option<StarMapViewport>, // Legacy
            source_node_id: Option<String>,
            host_endpoint: Option<StarMapEndpoint>,
            host_anchor: Option<String>, // Legacy
            #[serde(default)]
            provenance: crate::starmap::semantic::StarMapProvenance,
            created_at: u64,
            updated_at: u64,
        }

        let raw = Raw::deserialize(deserializer)?;
        let mut placement = raw.placement.unwrap_or_default();
        let mut target_viewport = raw.target_viewport.unwrap_or_default();

        if let Some(vp) = raw.viewport {
            placement.width = vp.width;
            placement.height = vp.height;
            target_viewport.scale = vp.scale;
            target_viewport.offset_x = vp.offset_x;
            target_viewport.offset_y = vp.offset_y;
        }

        let host_endpoint = raw.host_endpoint.or_else(|| {
            raw.host_anchor.and_then(|anchor_id| {
                if let Some(node_id) = &raw.source_node_id {
                    Some(StarMapEndpoint::Anchor {
                        node_id: node_id.clone(),
                        anchor_id,
                    })
                } else {
                    None
                }
            })
        });

        Ok(StarMapEmbed {
            instance_id: raw.instance_id,
            target_starmap_id: raw.target_starmap_id,
            label: raw.label,
            display_policy: raw.display_policy,
            open_behavior: raw.open_behavior,
            placement,
            target_viewport,
            source_node_id: raw.source_node_id,
            host_endpoint,
            provenance: raw.provenance,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEndpoint {
    Node { node_id: String },
    Anchor { node_id: String, anchor_id: String },
    Starmap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLink {
    pub link_id: String,
    pub source: StarMapEndpoint,
    pub target: crate::starmap::semantic::StarMapDeepTarget,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatch {
    pub label: Option<Option<String>>,
    pub display_policy: Option<crate::starmap::semantic::StarMapDisplayPolicy>,
    pub open_behavior: Option<crate::starmap::semantic::StarMapOpenBehavior>,
    pub viewport: Option<Option<StarMapViewport>>,
    pub placement: Option<Option<StarMapEmbedPlacement>>,
    pub target_viewport: Option<Option<StarMapEmbedViewport>>,
    pub source_node_id: Option<Option<String>>,
    pub host_anchor: Option<Option<String>>,
    pub host_endpoint: Option<Option<StarMapEndpoint>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatch {
    pub source: Option<StarMapEndpoint>,
    pub target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    pub label: Option<Option<String>>,
}
