use super::*;
// LEGACY MindMap DTOs — retained for migration compatibility only. Use StarMap DTOs for new features.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotDto {
    pub project_id: String,
    pub layout_kind: String,
    pub nodes: Vec<MindMapSnapshotNodeDto>,
    pub edges: Vec<MindMapSnapshotEdgeDto>,
    pub bounds: MindMapBoundsDto,
    pub generated_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotNodeDto {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKindDto,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub anchor_count: u32,
    pub broken_link: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotEdgeDto {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapBoundsDto {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl From<crate::mind_map::MindMapSnapshot> for MindMapSnapshotDto {
    fn from(s: crate::mind_map::MindMapSnapshot) -> Self {
        Self {
            project_id: s.project_id,
            layout_kind: s.layout_kind,
            nodes: s.nodes.into_iter().map(Into::into).collect(),
            edges: s.edges.into_iter().map(Into::into).collect(),
            bounds: s.bounds.into(),
            generated_at: s.generated_at,
        }
    }
}

impl From<crate::mind_map::MindMapSnapshotNode> for MindMapSnapshotNodeDto {
    fn from(n: crate::mind_map::MindMapSnapshotNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            anchor_count: n.anchor_count as u32,
            broken_link: n.broken_link,
            tags: n.tags,
        }
    }
}

impl From<crate::mind_map::MindMapSnapshotEdge> for MindMapSnapshotEdgeDto {
    fn from(e: crate::mind_map::MindMapSnapshotEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind,
            label: e.label,
        }
    }
}

impl From<crate::mind_map::MindMapBounds> for MindMapBoundsDto {
    fn from(b: crate::mind_map::MindMapBounds) -> Self {
        Self {
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphDto {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub nodes: Vec<MindMapGraphNodeDto>,
    pub edges: Vec<MindMapGraphEdgeDto>,
    pub anchors: Vec<MindMapAnchorDto>,
    pub links: Vec<MindMapLinkDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraph> for MindMapGraphDto {
    fn from(g: crate::mind_map::MindMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id,
            project_id: g.project_id,
            title: g.title,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            anchors: g.anchors.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

impl From<MindMapGraphDto> for crate::mind_map::MindMapGraph {
    fn from(d: MindMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            id: d.id,
            project_id: d.project_id,
            title: d.title,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            anchors: d.anchors.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphNodeDto {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKindDto,
    pub payload: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraphNode> for MindMapGraphNodeDto {
    fn from(n: crate::mind_map::MindMapGraphNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            payload: n
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            tags: n.tags,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

impl From<MindMapGraphNodeDto> for crate::mind_map::MindMapGraphNode {
    fn from(d: MindMapGraphNodeDto) -> Self {
        Self {
            id: d.id,
            title: d.title,
            kind: d.kind.into(),
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            tags: d.tags,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphEdgeDto {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: MindMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapGraphEdge> for MindMapGraphEdgeDto {
    fn from(e: crate::mind_map::MindMapGraphEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
            payload: e
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<MindMapGraphEdgeDto> for crate::mind_map::MindMapGraphEdge {
    fn from(d: MindMapGraphEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from,
            to: d.to,
            kind: d.kind.into(),
            label: d.label,
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapAnchorDto {
    pub id: String,
    pub project_id: String,
    pub chapter_id: String,
    pub start_offset: u32,
    pub end_offset: u32,
    pub selected_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub checksum: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapAnchor> for MindMapAnchorDto {
    fn from(a: crate::mind_map::MindMapAnchor) -> Self {
        Self {
            id: a.id,
            project_id: a.project_id,
            chapter_id: a.chapter_id,
            start_offset: a.start_offset as u32,
            end_offset: a.end_offset as u32,
            selected_text: a.selected_text,
            prefix_text: a.prefix_text,
            suffix_text: a.suffix_text,
            checksum: a.checksum,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

impl From<MindMapAnchorDto> for crate::mind_map::MindMapAnchor {
    fn from(d: MindMapAnchorDto) -> Self {
        Self {
            id: d.id,
            project_id: d.project_id,
            chapter_id: d.chapter_id,
            start_offset: d.start_offset as usize,
            end_offset: d.end_offset as usize,
            selected_text: d.selected_text,
            prefix_text: d.prefix_text,
            suffix_text: d.suffix_text,
            checksum: d.checksum,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLinkDto {
    pub id: String,
    pub node_id: String,
    pub anchor_id: String,
    pub kind: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::MindMapLink> for MindMapLinkDto {
    fn from(l: crate::mind_map::MindMapLink) -> Self {
        Self {
            id: l.id,
            node_id: l.node_id,
            anchor_id: l.anchor_id,
            kind: l.kind,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

impl From<MindMapLinkDto> for crate::mind_map::MindMapLink {
    fn from(d: MindMapLinkDto) -> Self {
        Self {
            id: d.id,
            node_id: d.node_id,
            anchor_id: d.anchor_id,
            kind: d.kind,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutDto {
    pub kind: MindMapLayoutKindDto,
    pub nodes: Vec<MindMapLayoutNodeDto>,
}

impl From<crate::mind_map::MindMapLayout> for MindMapLayoutDto {
    fn from(l: crate::mind_map::MindMapLayout) -> Self {
        Self {
            kind: l.kind.into(),
            nodes: l.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<MindMapLayoutDto> for crate::mind_map::MindMapLayout {
    fn from(d: MindMapLayoutDto) -> Self {
        Self {
            kind: d.kind.into(),
            nodes: d.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutNodeDto {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
}

impl From<crate::mind_map::MindMapLayoutNode> for MindMapLayoutNodeDto {
    fn from(n: crate::mind_map::MindMapLayoutNode) -> Self {
        Self {
            node_id: n.node_id,
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            z_index: n.z_index,
        }
    }
}

impl From<MindMapLayoutNodeDto> for crate::mind_map::MindMapLayoutNode {
    fn from(d: MindMapLayoutNodeDto) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            radius: d.radius,
            collapsed: d.collapsed,
            z_index: d.z_index,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphMetadataDto {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::mind_map::edit::MindMapGraphMetadata> for MindMapGraphMetadataDto {
    fn from(m: crate::mind_map::edit::MindMapGraphMetadata) -> Self {
        Self {
            id: m.id,
            title: m.title,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphsListDto {
    pub default_graph_id: Option<String>,
    pub graphs: Vec<MindMapGraphMetadataDto>,
}

impl From<crate::mind_map::edit::MindMapGraphsList> for MindMapGraphsListDto {
    fn from(l: crate::mind_map::edit::MindMapGraphsList) -> Self {
        Self {
            default_graph_id: l.default_graph_id,
            graphs: l.graphs.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapNodePatchDto {
    pub title: Option<String>,
    pub kind: Option<MindMapNodeKindDto>,
    pub payload: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}

impl From<MindMapNodePatchDto> for crate::mind_map::edit::MindMapGraphNodePatch {
    fn from(d: MindMapNodePatchDto) -> Self {
        Self {
            title: d.title,
            kind: d.kind.map(Into::into),
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
            tags: d.tags,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapEdgePatchDto {
    pub kind: Option<MindMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
}

impl From<MindMapEdgePatchDto> for crate::mind_map::edit::MindMapGraphEdgePatch {
    fn from(d: MindMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
        }
    }
}


// MindMap enums
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapNodeKindDto {
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
    Custom,
}

impl From<crate::mind_map::MindMapNodeKind> for MindMapNodeKindDto {
    fn from(k: crate::mind_map::MindMapNodeKind) -> Self {
        match k {
            crate::mind_map::MindMapNodeKind::Project => Self::Project,
            crate::mind_map::MindMapNodeKind::Volume => Self::Volume,
            crate::mind_map::MindMapNodeKind::Chapter => Self::Chapter,
            crate::mind_map::MindMapNodeKind::TextAnchor => Self::TextAnchor,
            crate::mind_map::MindMapNodeKind::Character => Self::Character,
            crate::mind_map::MindMapNodeKind::Event => Self::Event,
            crate::mind_map::MindMapNodeKind::Location => Self::Location,
            crate::mind_map::MindMapNodeKind::Item => Self::Item,
            crate::mind_map::MindMapNodeKind::Concept => Self::Concept,
            crate::mind_map::MindMapNodeKind::Theme => Self::Theme,
            crate::mind_map::MindMapNodeKind::Note => Self::Note,
            crate::mind_map::MindMapNodeKind::Organization => Self::Organization,
            crate::mind_map::MindMapNodeKind::Timeline => Self::Timeline,
            crate::mind_map::MindMapNodeKind::Plot => Self::Plot,
            crate::mind_map::MindMapNodeKind::Foreshadowing => Self::Foreshadowing,
            crate::mind_map::MindMapNodeKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapNodeKindDto> for crate::mind_map::MindMapNodeKind {
    fn from(dto: MindMapNodeKindDto) -> Self {
        match dto {
            MindMapNodeKindDto::Project => Self::Project,
            MindMapNodeKindDto::Volume => Self::Volume,
            MindMapNodeKindDto::Chapter => Self::Chapter,
            MindMapNodeKindDto::TextAnchor => Self::TextAnchor,
            MindMapNodeKindDto::Character => Self::Character,
            MindMapNodeKindDto::Event => Self::Event,
            MindMapNodeKindDto::Location => Self::Location,
            MindMapNodeKindDto::Item => Self::Item,
            MindMapNodeKindDto::Concept => Self::Concept,
            MindMapNodeKindDto::Theme => Self::Theme,
            MindMapNodeKindDto::Note => Self::Note,
            MindMapNodeKindDto::Organization => Self::Organization,
            MindMapNodeKindDto::Timeline => Self::Timeline,
            MindMapNodeKindDto::Plot => Self::Plot,
            MindMapNodeKindDto::Foreshadowing => Self::Foreshadowing,
            MindMapNodeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapEdgeKindDto {
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
    Custom,
}

impl From<crate::mind_map::MindMapEdgeKind> for MindMapEdgeKindDto {
    fn from(k: crate::mind_map::MindMapEdgeKind) -> Self {
        match k {
            crate::mind_map::MindMapEdgeKind::Contains => Self::Contains,
            crate::mind_map::MindMapEdgeKind::References => Self::References,
            crate::mind_map::MindMapEdgeKind::AppearsIn => Self::AppearsIn,
            crate::mind_map::MindMapEdgeKind::Causes => Self::Causes,
            crate::mind_map::MindMapEdgeKind::RelatedTo => Self::RelatedTo,
            crate::mind_map::MindMapEdgeKind::LocatedAt => Self::LocatedAt,
            crate::mind_map::MindMapEdgeKind::CharacterRelation => Self::CharacterRelation,
            crate::mind_map::MindMapEdgeKind::Timeline => Self::Timeline,
            crate::mind_map::MindMapEdgeKind::Foreshadows => Self::Foreshadows,
            crate::mind_map::MindMapEdgeKind::Resolves => Self::Resolves,
            crate::mind_map::MindMapEdgeKind::DependsOn => Self::DependsOn,
            crate::mind_map::MindMapEdgeKind::ConflictsWith => Self::ConflictsWith,
            crate::mind_map::MindMapEdgeKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapEdgeKindDto> for crate::mind_map::MindMapEdgeKind {
    fn from(dto: MindMapEdgeKindDto) -> Self {
        match dto {
            MindMapEdgeKindDto::Contains => Self::Contains,
            MindMapEdgeKindDto::References => Self::References,
            MindMapEdgeKindDto::AppearsIn => Self::AppearsIn,
            MindMapEdgeKindDto::Causes => Self::Causes,
            MindMapEdgeKindDto::RelatedTo => Self::RelatedTo,
            MindMapEdgeKindDto::LocatedAt => Self::LocatedAt,
            MindMapEdgeKindDto::CharacterRelation => Self::CharacterRelation,
            MindMapEdgeKindDto::Timeline => Self::Timeline,
            MindMapEdgeKindDto::Foreshadows => Self::Foreshadows,
            MindMapEdgeKindDto::Resolves => Self::Resolves,
            MindMapEdgeKindDto::DependsOn => Self::DependsOn,
            MindMapEdgeKindDto::ConflictsWith => Self::ConflictsWith,
            MindMapEdgeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapLayoutKindDto {
    AutoRadial,
    HorizontalTree,
    Freeform,
    Timeline,
    Relationship,
    Custom,
}

impl From<crate::mind_map::LayoutKind> for MindMapLayoutKindDto {
    fn from(k: crate::mind_map::LayoutKind) -> Self {
        match k {
            crate::mind_map::LayoutKind::AutoRadial => Self::AutoRadial,
            crate::mind_map::LayoutKind::HorizontalTree => Self::HorizontalTree,
            crate::mind_map::LayoutKind::Freeform => Self::Freeform,
            crate::mind_map::LayoutKind::Timeline => Self::Timeline,
            crate::mind_map::LayoutKind::Relationship => Self::Relationship,
            crate::mind_map::LayoutKind::Custom => Self::Custom,
        }
    }
}

impl From<MindMapLayoutKindDto> for crate::mind_map::LayoutKind {
    fn from(dto: MindMapLayoutKindDto) -> Self {
        match dto {
            MindMapLayoutKindDto::AutoRadial => Self::AutoRadial,
            MindMapLayoutKindDto::HorizontalTree => Self::HorizontalTree,
            MindMapLayoutKindDto::Freeform => Self::Freeform,
            MindMapLayoutKindDto::Timeline => Self::Timeline,
            MindMapLayoutKindDto::Relationship => Self::Relationship,
            MindMapLayoutKindDto::Custom => Self::Custom,
        }
    }
}
