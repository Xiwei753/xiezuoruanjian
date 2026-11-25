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
/// `schema_version` 用于格式识别；当前固定为 3。
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
    pub x: f64,
    pub y: f64,
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

/// 画布上的节点读模型（NodeView）。
///
/// Issue #1060：本类型不再拥有任何持久化数据。共享内容（标题、正文、锚点、
/// portal、样式、来源）的唯一所有者是 [`super::StarMapNodeDefinition`]，
/// 画布侧的唯一所有者是 [`super::StarMapNodeInstance`]。运行时把两者投影成
/// 本结构供解析器、边、校验和 DTO 使用。
///
/// - `id`：画布实例 ID（不是素材 ID）
/// - `asset_id`：共享素材 ID
/// - `updated_at`：**实例 revision**，只随位置/实例元数据变化递增
/// - `definition_revision`：素材本体的 revision，只随共享内容变化
///
/// 两者互不覆盖，也不再取 `max` 合成单一写入版本。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNode {
    pub id: String,
    /// 共享素材 ID。空字符串表示旧数据中"素材 ID 等于实例 ID"的兼容视图。
    #[serde(default)]
    pub asset_id: String,
    pub title: String,
    pub kind: StarMapNodeKind,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub content: StarMapNodeContent,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchor>,
    #[serde(default)]
    pub portal: Option<StarMapPortal>,
    /// 节点在星图文档坐标系下的位置。schema 4 起为必填 authored data。
    /// 旧格式由 migration 补 position；新格式缺 position 即视为损坏，
    /// 反序列化直接失败，不静默补 (0,0)。
    pub position: StarMapPoint,
    #[serde(default)]
    pub style: StarMapNodeStyle,
    #[serde(default)]
    pub provenance: StarMapProvenance,

    /// 画布实例创建时间。
    pub created_at: u64,
    /// 画布实例 revision（位置与实例元数据），不合并素材本体 revision。
    pub updated_at: u64,
    /// 共享素材本体 revision；旧数据缺省为 0。
    #[serde(default)]
    pub definition_revision: u64,
}

impl StarMapNode {
    pub fn instance_revision(&self) -> u64 {
        self.updated_at
    }

    pub fn definition_revision(&self) -> u64 {
        self.definition_revision
    }

    /// 当前节点引用的共享素材 ID；旧数据回退到实例 ID。
    pub fn resolved_asset_id(&self) -> &str {
        if self.asset_id.is_empty() {
            &self.id
        } else {
            &self.asset_id
        }
    }
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

/// 节点编辑命令：把共享内容修改与实例移动分开表达。
///
/// Issue #1060：`position` 属于画布实例，其余字段属于共享素材本体。
/// Core 会把本命令拆成 [`NodeDefinitionPatch`] 与 [`NodeInstancePatch`]，
/// 分别用各自的 expected revision 提交；只移动时不会触发整图校验。
///
/// `None` 表示"不修改"，`Some(None)` 表示"清空可选字段"（如 payload、portal）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatch {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKind>,
    pub payload: Option<Option<serde_json::Value>>,
    pub content: Option<StarMapNodeContent>,
    pub anchors: Option<Vec<StarMapAnchor>>,
    pub portal: Option<Option<StarMapPortal>>,
    pub position: Option<StarMapPoint>,
    pub style: Option<StarMapNodeStyle>,
    pub provenance: Option<StarMapProvenance>,
}

/// 共享素材本体补丁：写 `StarMapNodeDefinition`，不触碰实例位置与实例 revision。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinitionPatch {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKind>,
    pub payload: Option<Option<serde_json::Value>>,
    pub content: Option<StarMapNodeContent>,
    pub anchors: Option<Vec<StarMapAnchor>>,
    pub portal: Option<Option<StarMapPortal>>,
    pub style: Option<StarMapNodeStyle>,
    pub provenance: Option<StarMapProvenance>,
}

impl NodeDefinitionPatch {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.kind.is_none()
            && self.payload.is_none()
            && self.content.is_none()
            && self.anchors.is_none()
            && self.portal.is_none()
            && self.style.is_none()
            && self.provenance.is_none()
    }

    /// 只把共享内容字段从编辑命令里取出来；`position` 留在实例补丁。
    pub fn from_command(patch: &StarMapNodePatch) -> Self {
        Self {
            title: patch.title.clone(),
            kind: patch.kind.clone(),
            payload: patch.payload.clone(),
            content: patch.content.clone(),
            anchors: patch.anchors.clone(),
            portal: patch.portal.clone(),
            style: patch.style.clone(),
            provenance: patch.provenance.clone(),
        }
    }
}

/// 画布实例补丁：只写 `StarMapNodeInstance`，不触碰共享素材本体。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInstancePatch {
    pub position: Option<StarMapPoint>,
}

impl NodeInstancePatch {
    pub fn is_empty(&self) -> bool {
        self.position.is_none()
    }

    pub fn from_command(patch: &StarMapNodePatch) -> Self {
        Self {
            position: patch.position.clone(),
        }
    }
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
