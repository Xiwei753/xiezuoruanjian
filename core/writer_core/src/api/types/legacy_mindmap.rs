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



#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutDto {
    pub kind: MindMapLayoutKindDto,
    pub nodes: Vec<MindMapLayoutNodeDto>,
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



#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphMetadataDto {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphsListDto {
    pub default_graph_id: Option<String>,
    pub graphs: Vec<MindMapGraphMetadataDto>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapNodePatchDto {
    pub title: Option<String>,
    pub kind: Option<MindMapNodeKindDto>,
    pub payload: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MindMapEdgePatchDto {
    pub kind: Option<MindMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
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


