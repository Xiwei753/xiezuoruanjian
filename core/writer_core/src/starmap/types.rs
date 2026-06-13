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

// ---------------------------------------------------------------------------
// StarMapDocument — 独立文档对象
// ---------------------------------------------------------------------------
// StarMap 不再是嵌套树中的子节点，而是独立的文档对象。
// 大星图里出现小星图，是因为 Embed（嵌入）了另一个独立星图的实例，
// 而不是因为"拥有"它。StarMapDocument 是该独立文档的顶层容器。

/// StarMap 独立文档对象。
///
/// 每个 StarMapDocument 都是独立的文档实体，拥有自己的 meta、graph、layout。
/// 它不依赖父星图才能存在。`parent_starmap_id` 仅作为遗留兼容字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDocument {
    pub starmap_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Legacy: 仅作为遗留兼容组织字段，不表示拥有关系。
    #[serde(default)]
    pub parent_starmap_id: Option<String>,
    #[serde(default)]
    pub is_main_for_project: bool,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default)]
    pub graph: StarMapGraph,
    #[serde(default)]
    pub layout: StarMapLayout,
    #[serde(default)]
    pub viewport: StarMapViewport,
    /// 子星图放置信息：记录当前文档中嵌入的子星图实例的放置细节。
    #[serde(default)]
    pub child_map_placements: Vec<StarMapChildMapPlacement>,
    /// 超链接：跳转引用，不产生 Edge。
    #[serde(default)]
    pub hyperlinks: Vec<StarMapHyperlink>,
    pub created_at: u64,
    pub updated_at: u64,
}

fn default_accent_color() -> String {
    "#7B8CDE".to_string()
}

impl Default for StarMapDocument {
    fn default() -> Self {
        Self {
            starmap_id: String::new(),
            title: String::new(),
            description: String::new(),
            project_id: None,
            parent_starmap_id: None,
            is_main_for_project: false,
            accent_color: default_accent_color(),
            graph: StarMapGraph::default(),
            layout: StarMapLayout::default(),
            viewport: StarMapViewport::default(),
            child_map_placements: vec![],
            hyperlinks: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// StarMapEndpointPath — 端点路径
// ---------------------------------------------------------------------------
// 用于精确定位跨层级的端点。与 StarMapDeepTarget 不同，
// EndpointPath 专注于"从当前文档出发，经过哪些层级到达目标端点"，
// 是 Edge 端点引用的标准化路径表达。

/// 端点路径：描述从当前 StarMap 出发，经过一系列层级到达目标端点的路径。
///
/// 路径由 `segments` 组成，每个 segment 描述一次层级穿越（进入子星图空间），
/// 最终的 `endpoint` 描述路径终点的具体端点类型（节点/锚点/星图/深目标）。
/// 节点是原子，不允许作为路径段；节点只能出现在 `endpoint` 中。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointPath {
    /// 从当前 StarMap 出发的路径段序列。
    #[serde(default)]
    pub segments: Vec<StarMapEndpointPathSegment>,
    /// 路径终点的端点。
    pub endpoint: StarMapEdgeEndpoint,
}

impl StarMapEndpointPath {
    /// 检测路径中是否存在循环。
    ///
    /// 如果同一条路径中多次出现相同的 starmap_id（通过 EnterChildMap 段），
    /// 则判定为循环。
    pub fn has_cycle(&self) -> bool {
        let mut visited = std::collections::HashSet::new();
        for seg in &self.segments {
            let StarMapEndpointPathSegment::EnterChildMap { starmap_id } = seg;
            if !visited.insert(starmap_id.clone()) {
                return true;
            }
        }
        false
    }
}

/// 端点路径段：描述一次层级穿越。
///
/// 路径中间层只允许进入子星图空间，节点是原子，不能作为路径段"进入"。
/// 节点只能作为路径终点的 `endpoint`（`StarMapEdgeEndpoint::Node` / `Anchor`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEndpointPathSegment {
    /// 进入子星图空间。`starmap_id` 是目标子星图的 ID。
    EnterChildMap { starmap_id: String },
}

// ---------------------------------------------------------------------------
// StarMapChildMapPlacement — 子星图放置信息
// ---------------------------------------------------------------------------
// 记录当前文档中嵌入的子星图实例的放置细节。
// 与 StarMapEmbed 不同，ChildMapPlacement 专注于"放置"语义，
// 即子星图在父星图画布上的位置和显示策略，不涉及引用/跳转。

/// 子星图放置信息：记录当前文档中嵌入的子星图实例的放置细节。
///
/// child StarMap 不是 Node。子星图是独立的文档对象，通过 placement
/// 记录其在父星图画布上的位置，而不是作为节点存在于 graph 中。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapChildMapPlacement {
    /// 嵌入实例 ID（与 StarMapEmbed.instance_id 对应）。
    pub instance_id: String,
    /// 目标子星图 ID。
    pub target_starmap_id: String,
    /// 放置位置。
    pub placement: StarMapEmbedPlacement,
    /// 目标视口。
    #[serde(default)]
    pub target_viewport: StarMapEmbedViewport,
    /// 显示策略。
    #[serde(default)]
    pub display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
    /// 打开行为。
    #[serde(default)]
    pub open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
}

// ---------------------------------------------------------------------------
// StarMapHyperlink — 超链接
// ---------------------------------------------------------------------------
// 超链接是跳转引用，不产生 Edge。
// 与 StarMapLink 不同，Hyperlink 是文档级的跳转引用，
// 不参与 graph 的边计算，不产生 Edge。

/// 超链接：跳转引用，不产生 Edge。
///
/// Hyperlink 是文档级的跳转引用，用于从当前文档的某个端点跳转到
/// 另一个目标（外部 URI、其他星图等）。它不参与 graph 的边计算，
/// 不会在 graph.edges 中产生对应的 Edge 对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlink {
    /// 超链接 ID。
    pub hyperlink_id: String,
    /// 源端点路径。
    pub source: StarMapEndpointPath,
    /// 目标 URI 或描述。
    pub target_uri: String,
    /// 可选标签。
    pub label: Option<String>,
    /// 可选的目标星图 ID（如果跳转目标是另一个星图）。
    #[serde(default)]
    pub target_starmap_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
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
    /// Legacy: 旧式源节点 ID。新代码应使用 `from_endpoint_path`。
    #[serde(default)]
    pub from: Option<String>,
    /// Legacy: 旧式目标节点 ID。新代码应使用 `to_endpoint_path`。
    #[serde(default)]
    pub to: Option<String>,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
    /// Legacy: 旧式源端深目标。新代码应使用 `from_endpoint_path`。
    #[serde(default)]
    pub from_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    /// Legacy: 旧式目标端深目标。新代码应使用 `to_endpoint_path`。
    #[serde(default)]
    pub to_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    /// Legacy: 旧式源端端点。新代码应使用 `from_endpoint_path`。
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpoint>,
    /// Legacy: 旧式目标端端点。新代码应使用 `to_endpoint_path`。
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpoint>,
    /// 新式：源端端点路径，支持跨层级精确定位。
    #[serde(default)]
    pub from_endpoint_path: Option<StarMapEndpointPath>,
    /// 新式：目标端端点路径，支持跨层级精确定位。
    #[serde(default)]
    pub to_endpoint_path: Option<StarMapEndpointPath>,
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
    /// Legacy
    pub from_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    /// Legacy
    pub to_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    /// Legacy
    pub from_endpoint: Option<Option<StarMapEdgeEndpoint>>,
    /// Legacy
    pub to_endpoint: Option<Option<StarMapEdgeEndpoint>>,
    /// 新式：源端端点路径
    pub from_endpoint_path: Option<Option<StarMapEndpointPath>>,
    /// 新式：目标端端点路径
    pub to_endpoint_path: Option<Option<StarMapEndpointPath>>,
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
                raw.source_node_id
                    .as_ref()
                    .map(|node_id| StarMapEndpoint::Anchor {
                        node_id: node_id.clone(),
                        anchor_id,
                    })
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // EndpointPath 多层 roundtrip
    // -----------------------------------------------------------------------
    #[test]
    fn test_endpoint_path_multi_layer_roundtrip() {
        // 构造一个多层 EndpointPath: 进入子星图 A -> 进入子星图 B -> 终点是 Anchor
        let path = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child_b".to_string(),
                },
            ],
            endpoint: StarMapEdgeEndpoint::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
            },
        };

        // Serialize to JSON
        let json = serde_json::to_string(&path).unwrap();
        // Deserialize back
        let deserialized: StarMapEndpointPath = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized, path);
        assert_eq!(deserialized.segments.len(), 2);
        assert_eq!(
            deserialized.segments[0],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child_a".to_string(),
            }
        );
        assert_eq!(
            deserialized.segments[1],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child_b".to_string(),
            }
        );
        assert_eq!(
            deserialized.endpoint,
            StarMapEdgeEndpoint::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // EndpointPath 循环检测
    // -----------------------------------------------------------------------
    #[test]
    fn test_endpoint_path_cycle_detection() {
        // 无循环的路径
        let no_cycle = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_b".to_string(),
                },
            ],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n1".to_string(),
            },
        };
        assert!(!no_cycle.has_cycle());

        // 有循环的路径：sm_a 出现两次
        let with_cycle = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_b".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(), // 循环！
                },
            ],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n2".to_string(),
            },
        };
        assert!(with_cycle.has_cycle());

        // 空路径无循环
        let empty = StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::Starmap,
        };
        assert!(!empty.has_cycle());

        // 单段路径无循环
        let single = StarMapEndpointPath {
            segments: vec![StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_a".to_string(),
            }],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n1".to_string(),
            },
        };
        assert!(!single.has_cycle());
    }

    // -----------------------------------------------------------------------
    // child StarMap 不是 Node
    // -----------------------------------------------------------------------
    #[test]
    fn test_child_starmap_is_not_node() {
        // StarMapChildMapPlacement 是独立的放置信息，不是 StarMapNode
        let placement = StarMapChildMapPlacement {
            instance_id: "embed_1".to_string(),
            target_starmap_id: "sm_child".to_string(),
            placement: StarMapEmbedPlacement::default(),
            target_viewport: StarMapEmbedViewport::default(),
            display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
            open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
        };

        // 验证 child map placement 不在 graph.nodes 中
        // 它是文档级字段，不是节点
        let doc = StarMapDocument {
            starmap_id: "sm_parent".to_string(),
            title: "Parent".to_string(),
            child_map_placements: vec![placement.clone()],
            ..Default::default()
        };

        // graph.nodes 为空，但 child_map_placements 有内容
        assert!(doc.graph.nodes.is_empty());
        assert_eq!(doc.child_map_placements.len(), 1);
        assert_eq!(
            doc.child_map_placements[0].target_starmap_id,
            "sm_child"
        );

        // StarMapChildMapPlacement 序列化/反序列化 roundtrip
        let json = serde_json::to_string(&placement).unwrap();
        let deserialized: StarMapChildMapPlacement = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.instance_id, "embed_1");
        assert_eq!(deserialized.target_starmap_id, "sm_child");
    }

    // -----------------------------------------------------------------------
    // Hyperlink 不产生 Edge
    // -----------------------------------------------------------------------
    #[test]
    fn test_hyperlink_does_not_produce_edge() {
        let hyperlink = StarMapHyperlink {
            hyperlink_id: "hl_1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Node {
                    node_id: "n1".to_string(),
                },
            },
            target_uri: "https://example.com".to_string(),
            label: Some("Example".to_string()),
            target_starmap_id: Some("sm_other".to_string()),
            created_at: 0,
            updated_at: 0,
        };

        // 创建一个文档，包含 hyperlink 但不包含对应的 edge
        let doc = StarMapDocument {
            starmap_id: "sm_1".to_string(),
            title: "Test".to_string(),
            hyperlinks: vec![hyperlink.clone()],
            ..Default::default()
        };

        // graph.edges 为空，hyperlinks 有内容
        // 这证明 hyperlink 不产生 edge
        assert!(doc.graph.edges.is_empty());
        assert_eq!(doc.hyperlinks.len(), 1);
        assert_eq!(doc.hyperlinks[0].hyperlink_id, "hl_1");

        // Hyperlink 序列化/反序列化 roundtrip
        let json = serde_json::to_string(&hyperlink).unwrap();
        let deserialized: StarMapHyperlink = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.hyperlink_id, "hl_1");
        assert_eq!(deserialized.target_uri, "https://example.com");
        assert_eq!(deserialized.target_starmap_id, Some("sm_other".to_string()));
    }

    // -----------------------------------------------------------------------
    // containment 不产生 Edge
    // -----------------------------------------------------------------------
    #[test]
    fn test_containment_does_not_produce_edge() {
        // StarMapChildMapPlacement 表示子星图的包含/放置关系，
        // 但不产生 graph.edges 中的 Contains 边。
        // containment 是通过文档结构（child_map_placements）表达的，
        // 而不是通过 Edge 表达的。

        let doc = StarMapDocument {
            starmap_id: "sm_parent".to_string(),
            title: "Parent Map".to_string(),
            child_map_placements: vec![
                StarMapChildMapPlacement {
                    instance_id: "embed_1".to_string(),
                    target_starmap_id: "sm_child_1".to_string(),
                    placement: StarMapEmbedPlacement::default(),
                    target_viewport: StarMapEmbedViewport::default(),
                    display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
                    open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
                },
                StarMapChildMapPlacement {
                    instance_id: "embed_2".to_string(),
                    target_starmap_id: "sm_child_2".to_string(),
                    placement: StarMapEmbedPlacement::default(),
                    target_viewport: StarMapEmbedViewport::default(),
                    display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
                    open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
                },
            ],
            ..Default::default()
        };

        // 包含两个子星图，但 graph.edges 为空
        // containment 不产生 Edge
        assert!(doc.graph.edges.is_empty());
        assert_eq!(doc.child_map_placements.len(), 2);

        // 验证没有 Contains 类型的边
        let contains_edges: Vec<_> = doc
            .graph
            .edges
            .iter()
            .filter(|e| e.kind == StarMapEdgeKind::Contains)
            .collect();
        assert!(contains_edges.is_empty());
    }

    // -----------------------------------------------------------------------
    // StarMapDocument 序列化 roundtrip
    // -----------------------------------------------------------------------
    #[test]
    fn test_starmap_document_roundtrip() {
        let doc = StarMapDocument {
            starmap_id: "sm_1".to_string(),
            title: "Test Document".to_string(),
            description: "A test".to_string(),
            project_id: Some("proj_1".to_string()),
            parent_starmap_id: None,
            is_main_for_project: true,
            accent_color: "#FF0000".to_string(),
            graph: StarMapGraph::default(),
            layout: StarMapLayout::default(),
            viewport: StarMapViewport::default(),
            child_map_placements: vec![],
            hyperlinks: vec![],
            created_at: 1000,
            updated_at: 2000,
        };

        let json = serde_json::to_string(&doc).unwrap();
        let deserialized: StarMapDocument = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.starmap_id, "sm_1");
        assert_eq!(deserialized.title, "Test Document");
        assert_eq!(deserialized.project_id, Some("proj_1".to_string()));
        assert!(deserialized.is_main_for_project);
        assert_eq!(deserialized.accent_color, "#FF0000");
    }

    // -----------------------------------------------------------------------
    // StarMapEdge endpoint_path 字段 roundtrip
    // -----------------------------------------------------------------------
    #[test]
    fn test_edge_endpoint_path_roundtrip() {
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: None,
            to: None,
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: Some(StarMapEndpointPath {
                segments: vec![StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child".to_string(),
                }],
                endpoint: StarMapEdgeEndpoint::Node {
                    node_id: "n1".to_string(),
                },
            }),
            to_endpoint_path: Some(StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Starmap,
            }),
            created_at: 0,
            updated_at: 0,
        };

        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: StarMapEdge = serde_json::from_str(&json).unwrap();

        assert!(deserialized.from.is_none());
        assert!(deserialized.to.is_none());
        assert!(deserialized.from_endpoint_path.is_some());
        assert!(deserialized.to_endpoint_path.is_some());

        let from_path = deserialized.from_endpoint_path.unwrap();
        assert_eq!(from_path.segments.len(), 1);
        assert_eq!(
            from_path.segments[0],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child".to_string(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // StarMapEdge legacy 字段向后兼容
    // -----------------------------------------------------------------------
    #[test]
    fn test_edge_legacy_fields_backward_compatible() {
        // 旧格式 JSON（只有 from/to）应该仍然能反序列化
        let old_json = r#"{
            "id": "e_old",
            "from": "n1",
            "to": "n2",
            "kind": "relatedTo",
            "label": null,
            "payload": null,
            "createdAt": 0,
            "updatedAt": 0
        }"#;

        let edge: StarMapEdge = serde_json::from_str(old_json).unwrap();
        assert_eq!(edge.from, Some("n1".to_string()));
        assert_eq!(edge.to, Some("n2".to_string()));
        assert!(edge.from_endpoint_path.is_none());
        assert!(edge.to_endpoint_path.is_none());
    }
}
