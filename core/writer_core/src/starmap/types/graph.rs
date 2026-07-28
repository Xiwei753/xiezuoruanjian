use serde::{Deserialize, Serialize};

use crate::starmap::semantic::{
    StarMapAnchor, StarMapDisplayPolicy, StarMapNodeContent, StarMapOpenBehavior, StarMapPortal,
    StarMapProvenance,
};

use super::StarMapEndpointPath;

/// 边端点类型（结构化引用，替代 legacy `from`/`to` 字符串）。
///
/// - `Node`：直接引用当前星图中的节点
/// - `Anchor`：引用节点内的锚点
/// - `Starmap`：指向整个星图（无边端点）
/// - `DeepTarget`：跨星图层级引用，由 `resolve_deep_target` 在验证时解析
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

/// 星图图数据：节点、边、嵌入、链接的完整集合。
///
/// 持久化为 `graph.json`（单文件模式）或 `graph.json` + 子目录（包存储模式）。
/// `schema_version` 用于未来格式迁移；当前固定为 1。
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
    pub embeds: Vec<super::StarMapEmbed>,
    #[serde(default)]
    pub links: Vec<super::StarMapLink>,
    #[serde(default)]
    pub hyperlinks: Vec<super::StarMapHyperlink>,
    pub created_at: u64,
    pub updated_at: u64,
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
            hyperlinks: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
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

/// 星图边。
///
/// ## 端点引用演进（向后兼容）
///
/// 边的端点引用有三代字段，优先级从高到低：
/// 1. `from_endpoint_path` / `to_endpoint_path`：跨星图层级路径（v3，最优先）
/// 2. `from_endpoint` / `to_endpoint`：结构化端点（v2）
/// 3. `from` / `to`：legacy 节点 ID 字符串（v1，最低优先级）
///
/// 所有 legacy 字段使用 `#[serde(default)]` 保持向前兼容——
/// 旧格式 JSON 缺少新字段时自动填充为 `None`，新格式 JSON 缺少旧字段时同理。
/// 验证和渲染时按优先级选择可用的端点引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdge {
    pub id: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub from_endpoint_path: Option<StarMapEndpointPath>,
    #[serde(default)]
    pub to_endpoint_path: Option<StarMapEndpointPath>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// 节点局部更新补丁。
///
/// `None` 表示"不修改"，`Some(None)` 表示"清空可选字段"（如 payload、portal）。
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

/// 边局部更新补丁。
///
/// `None` 表示"不修改该字段"，`Some(None)` 表示"清空该可选字段"。
/// 这种双层 Option 模式允许区分"不改动"和"置空"两种语义。
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
    pub from_endpoint_path: Option<Option<StarMapEndpointPath>>,
    pub to_endpoint_path: Option<Option<StarMapEndpointPath>>,
}
