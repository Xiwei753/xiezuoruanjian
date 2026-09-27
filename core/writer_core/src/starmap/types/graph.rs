use serde::{Deserialize, Serialize};

use crate::starmap::semantic::{
    StarMapAnchor, StarMapNodeContent, StarMapPortal, StarMapProvenance,
};
use crate::starmap::types::reference::StarMapTargetPath;

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

/// 当前支持的星图 graph schema 版本。
///
/// `Default` 和 `to_starmap_graph()` 固定写出此版本。当前不做导出/导入 API；
/// 以后真做导入导出时，直接把同一套 Meta + Graph 包起来，不再另造一套模型。
///
/// 版本 3：node/embed 的 position/style 进入 authored object，删除 layout/viewport/
/// displayPolicy/openBehavior 等显示层字段。Graph 结构发生破坏性变化，
/// 从 2 升到 3 以区分 #772 的旧 Graph 和 #781 的新 Graph。
pub const CURRENT_GRAPH_SCHEMA_VERSION: u32 = 3;

/// 星图图数据：节点、边、嵌入、链接的完整集合。
///
/// 这是星图 authored object 的完整集合：节点的 `position`/`style`、
/// 嵌入的 `position` 都在对象自己身上，序列化 Graph 即可完整还原星图本身，
/// 不依赖 layout/viewport/display policy。持久化为 `graph.json`。
/// `schema_version` 用于格式识别；当前固定为 2。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraph {
    pub schema_version: u32,
    pub starmap_id: String,
    pub nodes: Vec<StarMapNode>,
    pub edges: Vec<StarMapEdge>,
    #[serde(default)]
    pub embeds: Vec<super::StarMapEmbed>,
    #[serde(default)]
    pub links: Vec<super::StarMapLink>,
    #[serde(default)]
    pub hyperlinks: Vec<super::StarMapHyperlink>,
}

impl Default for StarMapGraph {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
            starmap_id: String::new(),
            nodes: vec![],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            hyperlinks: vec![],
        }
    }
}

/// 节点在星图文档坐标系下的位置。
///
/// 这是节点的底层数据字段。节点移动就是更新 `position`，
/// 不再另外创建 layout record。各平台如何映射到像素属于显示层，
/// Core 的 position 只是星图文档坐标，不定义成屏幕像素单位。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPoint {
    pub x: f32,
    pub y: f32,
}

impl Default for StarMapPoint {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// 节点样式：纯数据层的外观属性，不含任何交互/渲染策略。
///
/// `fill_color` 为可选的 CSS 颜色字符串（如 `#7B8CDE`），
/// `None` 表示使用平台默认主题色。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeStyle {
    #[serde(default)]
    pub fill_color: Option<String>,
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
    /// 节点在星图文档坐标系下的位置。旧 schema 无此字段，
    /// 反序列化时默认-补全为 (0.0, 0.0)。
    #[serde(default)]
    pub position: StarMapPoint,
    #[serde(default)]
    pub style: StarMapNodeStyle,
    #[serde(default)]
    pub provenance: StarMapProvenance,

    pub created_at: u64,
    pub updated_at: u64,
}

/// 星图边。
///
/// 边的端点使用统一的 `StarMapTargetPath` 引用模型，
/// 支持跨星图层级引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdge {
    pub id: String,
    pub from: StarMapTargetPath,
    pub to: StarMapTargetPath,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
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
    pub position: Option<StarMapPoint>,
    pub style: Option<StarMapNodeStyle>,
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
    pub from: Option<StarMapTargetPath>,
    pub to: Option<StarMapTargetPath>,
}
