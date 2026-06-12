use super::*;
// StarMap DTOs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMetaDto {
    pub starmap_id: String,
    pub title: String,
    pub description: String,
    pub project_id: Option<String>,
    pub parent_starmap_id: Option<String>,
    pub is_main_for_project: bool,
    pub accent_color: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub linked_chapter_count: u32,
    pub child_starmap_count: u32,
}

impl From<crate::starmap::StarMapMeta> for StarMapMetaDto {
    fn from(m: crate::starmap::StarMapMeta) -> Self {
        Self {
            starmap_id: m.starmap_id,
            title: m.title,
            description: m.description,
            project_id: m.project_id,
            parent_starmap_id: m.parent_starmap_id,
            is_main_for_project: m.is_main_for_project,
            accent_color: m.accent_color,
            created_at: m.created_at,
            updated_at: m.updated_at,
            node_count: m.node_count,
            edge_count: m.edge_count,
            linked_chapter_count: m.linked_chapter_count,
            child_starmap_count: m.child_starmap_count,
        }
    }
}

impl From<StarMapMetaDto> for crate::starmap::StarMapMeta {
    fn from(d: StarMapMetaDto) -> Self {
        Self {
            starmap_id: d.starmap_id,
            title: d.title,
            description: d.description,
            project_id: d.project_id,
            parent_starmap_id: d.parent_starmap_id,
            is_main_for_project: d.is_main_for_project,
            accent_color: d.accent_color,
            created_at: d.created_at,
            updated_at: d.updated_at,
            node_count: d.node_count,
            edge_count: d.edge_count,
            linked_chapter_count: d.linked_chapter_count,
            child_starmap_count: d.child_starmap_count,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraphDto {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNodeDto>,
    pub edges: Vec<StarMapEdgeDto>,
    #[serde(default)]
    pub embeds: Vec<StarMapEmbedDto>,
    #[serde(default)]
    pub links: Vec<StarMapLinkDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapGraph> for StarMapGraphDto {
    fn from(g: crate::starmap::types::StarMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id,
            starmap_id: g.starmap_id,
            title: g.title,
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            embeds: g.embeds.into_iter().map(Into::into).collect(),
            links: g.links.into_iter().map(Into::into).collect(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

impl From<StarMapGraphDto> for crate::starmap::types::StarMapGraph {
    fn from(d: StarMapGraphDto) -> Self {
        Self {
            schema_version: d.schema_version,
            id: d.id,
            starmap_id: d.starmap_id,
            title: d.title,
            nodes: d.nodes.into_iter().map(Into::into).collect(),
            edges: d.edges.into_iter().map(Into::into).collect(),
            embeds: d.embeds.into_iter().map(Into::into).collect(),
            links: d.links.into_iter().map(Into::into).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeDto {
    pub id: String,
    pub title: String,
    pub kind: StarMapNodeKindDto,
    pub payload: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content: StarMapNodeContentDto,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchorDto>,
    #[serde(default)]
    pub portal: Option<StarMapPortalDto>,
    #[serde(default)]
    pub display_policy: StarMapDisplayPolicyDto,
    #[serde(default)]
    pub open_behavior: StarMapOpenBehaviorDto,
    #[serde(default)]
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapNode> for StarMapNodeDto {
    fn from(n: crate::starmap::types::StarMapNode) -> Self {
        Self {
            id: n.id,
            title: n.title,
            kind: n.kind.into(),
            payload: n
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            tags: n.tags,
            content: n.content.into(),
            anchors: n.anchors.into_iter().map(Into::into).collect(),
            portal: n.portal.map(Into::into),
            display_policy: n.display_policy.into(),
            open_behavior: n.open_behavior.into(),
            provenance: n.provenance.into(),
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

impl From<StarMapNodeDto> for crate::starmap::types::StarMapNode {
    fn from(d: StarMapNodeDto) -> Self {
        Self {
            id: d.id,
            title: d.title,
            kind: d.kind.into(),
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            tags: d.tags,
            content: d.content.into(),
            anchors: d.anchors.into_iter().map(Into::into).collect(),
            portal: d.portal.map(Into::into),
            display_policy: d.display_policy.into(),
            open_behavior: d.open_behavior.into(),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

// Flattened struct (was tagged enum StarMapNodeContentDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeContentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub uri: Option<String>,
    pub label: Option<String>,
}

impl From<crate::starmap::semantic::StarMapNodeContent> for StarMapNodeContentDto {
    fn from(c: crate::starmap::semantic::StarMapNodeContent) -> Self {
        match c {
            crate::starmap::semantic::StarMapNodeContent::Empty => Self {
                kind: "empty".to_string(),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::Inline { summary, body } => Self {
                kind: "inline".to_string(),
                summary,
                body,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::ChapterRef {
                project_id,
                volume_id,
                chapter_id,
                range_start,
                range_end,
            } => Self {
                kind: "chapterRef".to_string(),
                project_id: Some(project_id),
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::EntityRef {
                entity_type,
                entity_id,
            } => Self {
                kind: "entityRef".to_string(),
                entity_type: Some(entity_type),
                entity_id: Some(entity_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapNodeContent::ExternalRef { uri, label } => Self {
                kind: "externalRef".to_string(),
                uri: Some(uri),
                label,
                ..Default::default()
            },
        }
    }
}

impl From<StarMapNodeContentDto> for crate::starmap::semantic::StarMapNodeContent {
    fn from(d: StarMapNodeContentDto) -> Self {
        match d.kind.as_str() {
            "inline" => Self::Inline {
                summary: d.summary,
                body: d.body,
            },
            "chapterRef" => Self::ChapterRef {
                project_id: d.project_id.unwrap_or_default(),
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
            "entityRef" => Self::EntityRef {
                entity_type: d.entity_type.unwrap_or_default(),
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "externalRef" => Self::ExternalRef {
                uri: d.uri.unwrap_or_default(),
                label: d.label,
            },
            _ => Self::Empty,
        }
    }
}

// Since StarMapAnchor depends on semantic enums, we copy its structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchorDto {
    pub anchor_id: String,
    pub target: StarMapAnchorTargetDto,
    pub label: Option<String>,
    #[serde(default)]
    pub role: StarMapAnchorRoleDto,
}

impl From<crate::starmap::semantic::StarMapAnchor> for StarMapAnchorDto {
    fn from(a: crate::starmap::semantic::StarMapAnchor) -> Self {
        Self {
            anchor_id: a.anchor_id,
            target: a.target.into(),
            label: a.label,
            role: a.role.into(),
        }
    }
}

impl From<StarMapAnchorDto> for crate::starmap::semantic::StarMapAnchor {
    fn from(d: StarMapAnchorDto) -> Self {
        Self {
            anchor_id: d.anchor_id,
            target: d.target.into(),
            label: d.label,
            role: d.role.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortalDto {
    pub target_starmap_id: String,
    #[serde(default)]
    pub deep_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub mode: StarMapPortalModeDto,
    #[serde(default)]
    pub preview_policy: StarMapPortalPreviewPolicyDto,
}

impl From<crate::starmap::semantic::StarMapPortal> for StarMapPortalDto {
    fn from(p: crate::starmap::semantic::StarMapPortal) -> Self {
        Self {
            target_starmap_id: p.target_starmap_id,
            deep_target: p.deep_target.map(Into::into),
            mode: p.mode.into(),
            preview_policy: p.preview_policy.into(),
        }
    }
}

impl From<StarMapPortalDto> for crate::starmap::semantic::StarMapPortal {
    fn from(d: StarMapPortalDto) -> Self {
        Self {
            target_starmap_id: d.target_starmap_id,
            deep_target: d.deep_target.map(Into::into),
            mode: d.mode.into(),
            preview_policy: d.preview_policy.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDisplayPolicyDto {
    pub importance: f32,
    pub min_visible_scale: f32,
    pub title_scale: f32,
    pub summary_scale: f32,
    pub detail_scale: f32,
    pub max_preview_chars: u32,
    pub min_readable_px: f32,
}

impl Default for StarMapDisplayPolicyDto {
    fn default() -> Self {
        crate::starmap::semantic::StarMapDisplayPolicy::default().into()
    }
}

impl From<crate::starmap::semantic::StarMapDisplayPolicy> for StarMapDisplayPolicyDto {
    fn from(p: crate::starmap::semantic::StarMapDisplayPolicy) -> Self {
        Self {
            importance: p.importance,
            min_visible_scale: p.min_visible_scale,
            title_scale: p.title_scale,
            summary_scale: p.summary_scale,
            detail_scale: p.detail_scale,
            max_preview_chars: p.max_preview_chars,
            min_readable_px: p.min_readable_px,
        }
    }
}

impl From<StarMapDisplayPolicyDto> for crate::starmap::semantic::StarMapDisplayPolicy {
    fn from(d: StarMapDisplayPolicyDto) -> Self {
        Self {
            importance: d.importance,
            min_visible_scale: d.min_visible_scale,
            title_scale: d.title_scale,
            summary_scale: d.summary_scale,
            detail_scale: d.detail_scale,
            max_preview_chars: d.max_preview_chars,
            min_readable_px: d.min_readable_px,
        }
    }
}

// We map OpenBehavior to string simply, or enum wrapper. Since it's enum in crate, we wrap it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapOpenBehaviorDto {
    #[default]
    Inspector,
    ExpandCard,
    WritingMode,
    JumpToAnchor,
    EnterPortal,
    Custom,
}

impl From<crate::starmap::semantic::StarMapOpenBehavior> for StarMapOpenBehaviorDto {
    fn from(b: crate::starmap::semantic::StarMapOpenBehavior) -> Self {
        match b {
            crate::starmap::semantic::StarMapOpenBehavior::Inspector => Self::Inspector,
            crate::starmap::semantic::StarMapOpenBehavior::ExpandCard => Self::ExpandCard,
            crate::starmap::semantic::StarMapOpenBehavior::WritingMode => Self::WritingMode,
            crate::starmap::semantic::StarMapOpenBehavior::JumpToAnchor => Self::JumpToAnchor,
            crate::starmap::semantic::StarMapOpenBehavior::EnterPortal => Self::EnterPortal,
            crate::starmap::semantic::StarMapOpenBehavior::Custom => Self::Custom,
        }
    }
}

impl From<StarMapOpenBehaviorDto> for crate::starmap::semantic::StarMapOpenBehavior {
    fn from(d: StarMapOpenBehaviorDto) -> Self {
        match d {
            StarMapOpenBehaviorDto::Inspector => Self::Inspector,
            StarMapOpenBehaviorDto::ExpandCard => Self::ExpandCard,
            StarMapOpenBehaviorDto::WritingMode => Self::WritingMode,
            StarMapOpenBehaviorDto::JumpToAnchor => Self::JumpToAnchor,
            StarMapOpenBehaviorDto::EnterPortal => Self::EnterPortal,
            StarMapOpenBehaviorDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProvenanceDto {
    #[serde(default)]
    pub source: StarMapSourceKindDto,
    pub source_id: Option<String>,
    pub generated_by: Option<String>,
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub review_status: StarMapReviewStatusDto,
    pub created_from_anchor: Option<String>,
}

impl From<crate::starmap::semantic::StarMapProvenance> for StarMapProvenanceDto {
    fn from(p: crate::starmap::semantic::StarMapProvenance) -> Self {
        Self {
            source: p.source.into(),
            source_id: p.source_id,
            generated_by: p.generated_by,
            prompt_id: p.prompt_id,
            review_status: p.review_status.into(),
            created_from_anchor: p.created_from_anchor,
        }
    }
}

impl From<StarMapProvenanceDto> for crate::starmap::semantic::StarMapProvenance {
    fn from(d: StarMapProvenanceDto) -> Self {
        Self {
            source: d.source.into(),
            source_id: d.source_id,
            generated_by: d.generated_by,
            prompt_id: d.prompt_id,
            review_status: d.review_status.into(),
            created_from_anchor: d.created_from_anchor,
        }
    }
}

impl Default for StarMapProvenanceDto {
    fn default() -> Self {
        crate::starmap::semantic::StarMapProvenance::default().into()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDeepTargetDto {
    pub starmap_id: String,
    #[serde(default)]
    pub path: Vec<StarMapPathSegmentDto>,
    pub target: StarMapTargetDetailDto,
}

impl From<crate::starmap::semantic::StarMapDeepTarget> for StarMapDeepTargetDto {
    fn from(t: crate::starmap::semantic::StarMapDeepTarget) -> Self {
        Self {
            starmap_id: t.starmap_id,
            path: t.path.into_iter().map(Into::into).collect(),
            target: t.target.into(),
        }
    }
}

impl From<StarMapDeepTargetDto> for crate::starmap::semantic::StarMapDeepTarget {
    fn from(d: StarMapDeepTargetDto) -> Self {
        Self {
            starmap_id: d.starmap_id,
            path: d.path.into_iter().map(Into::into).collect(),
            target: d.target.into(),
        }
    }
}

// Flattened struct (was tagged enum StarMapPathSegmentDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPathSegmentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub starmap_id: Option<String>,
    pub node_id: Option<String>,
}

impl From<crate::starmap::semantic::StarMapPathSegment> for StarMapPathSegmentDto {
    fn from(s: crate::starmap::semantic::StarMapPathSegment) -> Self {
        match s {
            crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id } => Self {
                kind: "enterChild".to_string(),
                starmap_id: Some(starmap_id),
                node_id: None,
            },
            crate::starmap::semantic::StarMapPathSegment::EnterNode { node_id } => Self {
                kind: "enterNode".to_string(),
                starmap_id: None,
                node_id: Some(node_id),
            },
        }
    }
}

impl From<StarMapPathSegmentDto> for crate::starmap::semantic::StarMapPathSegment {
    fn from(d: StarMapPathSegmentDto) -> Self {
        match d.kind.as_str() {
            "enterChild" => Self::EnterChild {
                starmap_id: d.starmap_id.unwrap_or_default(),
            },
            _ => Self::EnterNode {
                node_id: d.node_id.unwrap_or_default(),
            },
        }
    }
}

// Flattened struct (was tagged enum StarMapEdgeEndpointDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeEndpointDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
    pub target: Option<StarMapDeepTargetDto>,
}

impl From<crate::starmap::types::StarMapEdgeEndpoint> for StarMapEdgeEndpointDto {
    fn from(e: crate::starmap::types::StarMapEdgeEndpoint) -> Self {
        match e {
            crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::Starmap => Self {
                kind: "starmap".to_string(),
                node_id: None,
                anchor_id: None,
                target: None,
            },
            crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { target } => Self {
                kind: "deepTarget".to_string(),
                node_id: None,
                anchor_id: None,
                target: Some(target.into()),
            },
        }
    }
}

impl From<StarMapEdgeEndpointDto> for crate::starmap::types::StarMapEdgeEndpoint {
    fn from(d: StarMapEdgeEndpointDto) -> Self {
        match d.kind.as_str() {
            "anchor" => Self::Anchor {
                node_id: d.node_id.unwrap_or_default(),
                anchor_id: d.anchor_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap,
            "deepTarget" => Self::DeepTarget {
                target: d.target.map(Into::into).unwrap_or_else(|| {
                    crate::starmap::semantic::StarMapDeepTarget {
                        starmap_id: String::new(),
                        path: vec![],
                        target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                    }
                }),
            },
            _ => Self::Node {
                node_id: d.node_id.unwrap_or_default(),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeDto {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub kind: StarMapEdgeKindDto,
    pub label: Option<String>,
    pub payload: Option<String>,
    #[serde(default)]
    pub from_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub to_target: Option<StarMapDeepTargetDto>,
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpointDto>,
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpointDto>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEdge> for StarMapEdgeDto {
    fn from(e: crate::starmap::types::StarMapEdge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind.into(),
            label: e.label,
            payload: e
                .payload
                .map(|v| serde_json::to_string(&v).unwrap_or_default()),
            from_target: e.from_target.map(Into::into),
            to_target: e.to_target.map(Into::into),
            from_endpoint: e.from_endpoint.map(Into::into),
            to_endpoint: e.to_endpoint.map(Into::into),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEdgeDto> for crate::starmap::types::StarMapEdge {
    fn from(d: StarMapEdgeDto) -> Self {
        Self {
            id: d.id,
            from: d.from,
            to: d.to,
            kind: d.kind.into(),
            label: d.label,
            payload: d
                .payload
                .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
            from_target: d.from_target.map(Into::into),
            to_target: d.to_target.map(Into::into),
            from_endpoint: d.from_endpoint.map(Into::into),
            to_endpoint: d.to_endpoint.map(Into::into),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPlacementDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub z_index: i32,
    pub collapsed: bool,
}

impl From<crate::starmap::types::StarMapEmbedPlacement> for StarMapEmbedPlacementDto {
    fn from(p: crate::starmap::types::StarMapEmbedPlacement) -> Self {
        Self {
            x: p.x,
            y: p.y,
            width: p.width,
            height: p.height,
            scale: p.scale,
            z_index: p.z_index,
            collapsed: p.collapsed,
        }
    }
}

impl From<StarMapEmbedPlacementDto> for crate::starmap::types::StarMapEmbedPlacement {
    fn from(d: StarMapEmbedPlacementDto) -> Self {
        Self {
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            scale: d.scale,
            z_index: d.z_index,
            collapsed: d.collapsed,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl From<crate::starmap::types::StarMapEmbedViewport> for StarMapEmbedViewportDto {
    fn from(v: crate::starmap::types::StarMapEmbedViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
        }
    }
}

impl From<StarMapEmbedViewportDto> for crate::starmap::types::StarMapEmbedViewport {
    fn from(d: StarMapEmbedViewportDto) -> Self {
        Self {
            scale: d.scale,
            offset_x: d.offset_x,
            offset_y: d.offset_y,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedDto {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: StarMapDisplayPolicyDto,
    pub open_behavior: StarMapOpenBehaviorDto,
    pub placement: StarMapEmbedPlacementDto,
    pub target_viewport: StarMapEmbedViewportDto,
    pub source_node_id: Option<String>,
    pub host_endpoint: Option<StarMapEndpointDto>,
    pub provenance: StarMapProvenanceDto,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapEmbed> for StarMapEmbedDto {
    fn from(e: crate::starmap::types::StarMapEmbed) -> Self {
        Self {
            instance_id: e.instance_id,
            target_starmap_id: e.target_starmap_id,
            label: e.label,
            display_policy: e.display_policy.into(),
            open_behavior: e.open_behavior.into(),
            placement: e.placement.into(),
            target_viewport: e.target_viewport.into(),
            source_node_id: e.source_node_id,
            host_endpoint: e.host_endpoint.map(Into::into),
            provenance: e.provenance.into(),
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

impl From<StarMapEmbedDto> for crate::starmap::types::StarMapEmbed {
    fn from(d: StarMapEmbedDto) -> Self {
        Self {
            instance_id: d.instance_id,
            target_starmap_id: d.target_starmap_id,
            label: d.label,
            display_policy: d.display_policy.into(),
            open_behavior: d.open_behavior.into(),
            placement: d.placement.into(),
            target_viewport: d.target_viewport.into(),
            source_node_id: d.source_node_id,
            host_endpoint: d.host_endpoint.map(Into::into),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkDto {
    pub link_id: String,
    pub source: StarMapEndpointDto,
    pub target: StarMapDeepTargetDto,
    pub label: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<crate::starmap::types::StarMapLink> for StarMapLinkDto {
    fn from(l: crate::starmap::types::StarMapLink) -> Self {
        Self {
            link_id: l.link_id,
            source: l.source.into(),
            target: l.target.into(),
            label: l.label,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

impl From<StarMapLinkDto> for crate::starmap::types::StarMapLink {
    fn from(d: StarMapLinkDto) -> Self {
        Self {
            link_id: d.link_id,
            source: d.source.into(),
            target: d.target.into(),
            label: d.label,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutDto {
    pub kind: StarMapLayoutKindDto,
    pub nodes: Vec<StarMapLayoutNodeDto>,
}

impl From<crate::starmap::types::StarMapLayout> for StarMapLayoutDto {
    fn from(l: crate::starmap::types::StarMapLayout) -> Self {
        Self {
            kind: l.kind.into(),
            nodes: l.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<StarMapLayoutDto> for crate::starmap::types::StarMapLayout {
    fn from(d: StarMapLayoutDto) -> Self {
        Self {
            kind: d.kind.into(),
            nodes: d.nodes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLayoutNodeDto {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
    pub scale: f32,
    pub depth: f32,
    pub focus_weight: f32,
    pub orbit_group: Option<String>,
}

impl From<crate::starmap::types::StarMapLayoutNode> for StarMapLayoutNodeDto {
    fn from(n: crate::starmap::types::StarMapLayoutNode) -> Self {
        Self {
            node_id: n.node_id,
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            radius: n.radius,
            collapsed: n.collapsed,
            z_index: n.z_index,
            scale: n.scale,
            depth: n.depth,
            focus_weight: n.focus_weight,
            orbit_group: n.orbit_group,
        }
    }
}

impl From<StarMapLayoutNodeDto> for crate::starmap::types::StarMapLayoutNode {
    fn from(d: StarMapLayoutNodeDto) -> Self {
        Self {
            node_id: d.node_id,
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            radius: d.radius,
            collapsed: d.collapsed,
            z_index: d.z_index,
            scale: d.scale,
            depth: d.depth,
            focus_weight: d.focus_weight,
            orbit_group: d.orbit_group,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgeRenderDto {
    pub edge_id: String,
    pub from_cx: f32,
    pub from_cy: f32,
    pub to_cx: f32,
    pub to_cy: f32,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub arrow_tip_x: f32,
    pub arrow_tip_y: f32,
    pub arrow_left_x: f32,
    pub arrow_left_y: f32,
    pub arrow_right_x: f32,
    pub arrow_right_y: f32,
    pub label_x: f32,
    pub label_y: f32,
    pub has_bidirectional: bool,
}

impl From<crate::starmap::render::EdgeRender> for StarMapEdgeRenderDto {
    fn from(r: crate::starmap::render::EdgeRender) -> Self {
        Self {
            edge_id: r.edge_id,
            from_cx: r.from_cx,
            from_cy: r.from_cy,
            to_cx: r.to_cx,
            to_cy: r.to_cy,
            start_x: r.start_x,
            start_y: r.start_y,
            end_x: r.end_x,
            end_y: r.end_y,
            offset_x: r.offset_x,
            offset_y: r.offset_y,
            arrow_tip_x: r.arrow_tip_x,
            arrow_tip_y: r.arrow_tip_y,
            arrow_left_x: r.arrow_left_x,
            arrow_left_y: r.arrow_left_y,
            arrow_right_x: r.arrow_right_x,
            arrow_right_y: r.arrow_right_y,
            label_x: r.label_x,
            label_y: r.label_y,
            has_bidirectional: r.has_bidirectional,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReferenceDto {
    pub host_starmap_id: String,
    pub host_title: String,
    pub ref_type: String,
    pub ref_id: String,
    pub target_starmap_id: String,
}

impl From<crate::starmap::StarMapReference> for StarMapReferenceDto {
    fn from(r: crate::starmap::StarMapReference) -> Self {
        Self {
            host_starmap_id: r.host_starmap_id,
            host_title: r.host_title,
            ref_type: r.ref_type,
            ref_id: r.ref_id,
            target_starmap_id: r.target_starmap_id,
        }
    }
}

impl From<StarMapReferenceDto> for crate::starmap::StarMapReference {
    fn from(d: StarMapReferenceDto) -> Self {
        Self {
            host_starmap_id: d.host_starmap_id,
            host_title: d.host_title,
            ref_type: d.ref_type,
            ref_id: d.ref_id,
            target_starmap_id: d.target_starmap_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatchDto {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKindDto>,
    pub payload: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub content: Option<StarMapNodeContentDto>,
    pub anchors: Option<Vec<StarMapAnchorDto>>,
    pub portal: Option<Option<StarMapPortalDto>>,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub provenance: Option<StarMapProvenanceDto>,
}

impl From<StarMapNodePatchDto> for crate::starmap::types::StarMapNodePatch {
    fn from(d: StarMapNodePatchDto) -> Self {
        Self {
            title: d.title,
            kind: d.kind.map(Into::into),
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
            tags: d.tags,
            content: d.content.map(Into::into),
            anchors: d.anchors.map(|v| v.into_iter().map(Into::into).collect()),
            portal: d.portal.map(|p| p.map(Into::into)),
            display_policy: d.display_policy.map(Into::into),
            open_behavior: d.open_behavior.map(Into::into),
            provenance: d.provenance.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatchDto {
    pub kind: Option<StarMapEdgeKindDto>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<String>>,
    pub from_target: Option<Option<StarMapDeepTargetDto>>,
    pub to_target: Option<Option<StarMapDeepTargetDto>>,
    pub from_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
    pub to_endpoint: Option<Option<StarMapEdgeEndpointDto>>,
}

impl From<StarMapEdgePatchDto> for crate::starmap::types::StarMapEdgePatch {
    fn from(d: StarMapEdgePatchDto) -> Self {
        Self {
            kind: d.kind.map(Into::into),
            label: d.label,
            payload: d.payload.map(|opt| {
                opt.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
            }),
            from_target: d.from_target.map(|v| v.map(Into::into)),
            to_target: d.to_target.map(|v| v.map(Into::into)),
            from_endpoint: d.from_endpoint.map(|v| v.map(Into::into)),
            to_endpoint: d.to_endpoint.map(|v| v.map(Into::into)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchDto {
    pub label: Option<Option<String>>,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub viewport: Option<Option<StarMapViewportDto>>,
    pub placement: Option<Option<StarMapEmbedPlacementDto>>,
    pub target_viewport: Option<Option<StarMapEmbedViewportDto>>,
    pub source_node_id: Option<Option<String>>,
    pub host_anchor: Option<Option<String>>,
    pub host_endpoint: Option<Option<StarMapEndpointDto>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatchInputDto {
    pub label: Option<String>,
    pub clear_label: bool,
    pub display_policy: Option<StarMapDisplayPolicyDto>,
    pub open_behavior: Option<StarMapOpenBehaviorDto>,
    pub viewport: Option<StarMapViewportDto>,
    pub clear_viewport: bool,
    pub placement: Option<StarMapEmbedPlacementDto>,
    pub clear_placement: bool,
    pub target_viewport: Option<StarMapEmbedViewportDto>,
    pub clear_target_viewport: bool,
    pub source_node_id: Option<String>,
    pub clear_source_node_id: bool,
    pub host_anchor: Option<String>,
    pub clear_host_anchor: bool,
    pub host_endpoint: Option<StarMapEndpointDto>,
    pub clear_host_endpoint: bool,
}

impl From<StarMapEmbedPatchInputDto> for StarMapEmbedPatchDto {
    fn from(d: StarMapEmbedPatchInputDto) -> Self {
        Self {
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
            display_policy: d.display_policy,
            open_behavior: d.open_behavior,
            viewport: if d.clear_viewport {
                Some(None)
            } else {
                d.viewport.map(Some)
            },
            placement: if d.clear_placement {
                Some(None)
            } else {
                d.placement.map(Some)
            },
            target_viewport: if d.clear_target_viewport {
                Some(None)
            } else {
                d.target_viewport.map(Some)
            },
            source_node_id: if d.clear_source_node_id {
                Some(None)
            } else {
                d.source_node_id.map(Some)
            },
            host_anchor: if d.clear_host_anchor {
                Some(None)
            } else {
                d.host_anchor.map(Some)
            },
            host_endpoint: if d.clear_host_endpoint {
                Some(None)
            } else {
                d.host_endpoint.map(Some)
            },
        }
    }
}

impl From<StarMapEmbedPatchDto> for crate::starmap::types::StarMapEmbedPatch {
    fn from(d: StarMapEmbedPatchDto) -> Self {
        Self {
            label: d.label,
            display_policy: d.display_policy.map(Into::into),
            open_behavior: d.open_behavior.map(Into::into),
            viewport: d.viewport.map(|v| v.map(Into::into)),
            placement: d.placement.map(|p| p.map(Into::into)),
            target_viewport: d.target_viewport.map(|v| v.map(Into::into)),
            source_node_id: d.source_node_id,
            host_anchor: d.host_anchor,
            host_endpoint: d.host_endpoint.map(|v| v.map(Into::into)),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchDto {
    pub source: Option<StarMapEndpointDto>,
    pub target: Option<StarMapDeepTargetDto>,
    pub label: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLinkPatchInputDto {
    pub source: Option<StarMapEndpointDto>,
    pub target: Option<StarMapDeepTargetDto>,
    pub label: Option<String>,
    pub clear_label: bool,
}

impl From<StarMapLinkPatchInputDto> for StarMapLinkPatchDto {
    fn from(d: StarMapLinkPatchInputDto) -> Self {
        Self {
            source: d.source,
            target: d.target,
            label: if d.clear_label {
                Some(None)
            } else {
                d.label.map(Some)
            },
        }
    }
}

impl From<StarMapLinkPatchDto> for crate::starmap::types::StarMapLinkPatch {
    fn from(d: StarMapLinkPatchDto) -> Self {
        Self {
            source: d.source.map(Into::into),
            target: d.target.map(Into::into),
            label: d.label,
        }
    }
}

// StarMap enums and additional types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapNodeKindDto {
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
    Custom,
}

impl From<crate::starmap::types::StarMapNodeKind> for StarMapNodeKindDto {
    fn from(k: crate::starmap::types::StarMapNodeKind) -> Self {
        match k {
            crate::starmap::types::StarMapNodeKind::Character => Self::Character,
            crate::starmap::types::StarMapNodeKind::Event => Self::Event,
            crate::starmap::types::StarMapNodeKind::Location => Self::Location,
            crate::starmap::types::StarMapNodeKind::Item => Self::Item,
            crate::starmap::types::StarMapNodeKind::Concept => Self::Concept,
            crate::starmap::types::StarMapNodeKind::Theme => Self::Theme,
            crate::starmap::types::StarMapNodeKind::Note => Self::Note,
            crate::starmap::types::StarMapNodeKind::Organization => Self::Organization,
            crate::starmap::types::StarMapNodeKind::Timeline => Self::Timeline,
            crate::starmap::types::StarMapNodeKind::Plot => Self::Plot,
            crate::starmap::types::StarMapNodeKind::Foreshadowing => Self::Foreshadowing,
            crate::starmap::types::StarMapNodeKind::Chapter => Self::Chapter,
            crate::starmap::types::StarMapNodeKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapNodeKindDto> for crate::starmap::types::StarMapNodeKind {
    fn from(dto: StarMapNodeKindDto) -> Self {
        match dto {
            StarMapNodeKindDto::Character => Self::Character,
            StarMapNodeKindDto::Event => Self::Event,
            StarMapNodeKindDto::Location => Self::Location,
            StarMapNodeKindDto::Item => Self::Item,
            StarMapNodeKindDto::Concept => Self::Concept,
            StarMapNodeKindDto::Theme => Self::Theme,
            StarMapNodeKindDto::Note => Self::Note,
            StarMapNodeKindDto::Organization => Self::Organization,
            StarMapNodeKindDto::Timeline => Self::Timeline,
            StarMapNodeKindDto::Plot => Self::Plot,
            StarMapNodeKindDto::Foreshadowing => Self::Foreshadowing,
            StarMapNodeKindDto::Chapter => Self::Chapter,
            StarMapNodeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapEdgeKindDto {
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

impl From<crate::starmap::types::StarMapEdgeKind> for StarMapEdgeKindDto {
    fn from(k: crate::starmap::types::StarMapEdgeKind) -> Self {
        match k {
            crate::starmap::types::StarMapEdgeKind::Contains => Self::Contains,
            crate::starmap::types::StarMapEdgeKind::References => Self::References,
            crate::starmap::types::StarMapEdgeKind::AppearsIn => Self::AppearsIn,
            crate::starmap::types::StarMapEdgeKind::Causes => Self::Causes,
            crate::starmap::types::StarMapEdgeKind::RelatedTo => Self::RelatedTo,
            crate::starmap::types::StarMapEdgeKind::LocatedAt => Self::LocatedAt,
            crate::starmap::types::StarMapEdgeKind::CharacterRelation => Self::CharacterRelation,
            crate::starmap::types::StarMapEdgeKind::Timeline => Self::Timeline,
            crate::starmap::types::StarMapEdgeKind::Foreshadows => Self::Foreshadows,
            crate::starmap::types::StarMapEdgeKind::Resolves => Self::Resolves,
            crate::starmap::types::StarMapEdgeKind::DependsOn => Self::DependsOn,
            crate::starmap::types::StarMapEdgeKind::ConflictsWith => Self::ConflictsWith,
            crate::starmap::types::StarMapEdgeKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapEdgeKindDto> for crate::starmap::types::StarMapEdgeKind {
    fn from(dto: StarMapEdgeKindDto) -> Self {
        match dto {
            StarMapEdgeKindDto::Contains => Self::Contains,
            StarMapEdgeKindDto::References => Self::References,
            StarMapEdgeKindDto::AppearsIn => Self::AppearsIn,
            StarMapEdgeKindDto::Causes => Self::Causes,
            StarMapEdgeKindDto::RelatedTo => Self::RelatedTo,
            StarMapEdgeKindDto::LocatedAt => Self::LocatedAt,
            StarMapEdgeKindDto::CharacterRelation => Self::CharacterRelation,
            StarMapEdgeKindDto::Timeline => Self::Timeline,
            StarMapEdgeKindDto::Foreshadows => Self::Foreshadows,
            StarMapEdgeKindDto::Resolves => Self::Resolves,
            StarMapEdgeKindDto::DependsOn => Self::DependsOn,
            StarMapEdgeKindDto::ConflictsWith => Self::ConflictsWith,
            StarMapEdgeKindDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapLayoutKindDto {
    Freeform,
    AutoRadial,
    Custom,
}

impl From<crate::starmap::types::StarMapLayoutKind> for StarMapLayoutKindDto {
    fn from(k: crate::starmap::types::StarMapLayoutKind) -> Self {
        match k {
            crate::starmap::types::StarMapLayoutKind::Freeform => Self::Freeform,
            crate::starmap::types::StarMapLayoutKind::AutoRadial => Self::AutoRadial,
            crate::starmap::types::StarMapLayoutKind::Custom => Self::Custom,
        }
    }
}

impl From<StarMapLayoutKindDto> for crate::starmap::types::StarMapLayoutKind {
    fn from(dto: StarMapLayoutKindDto) -> Self {
        match dto {
            StarMapLayoutKindDto::Freeform => Self::Freeform,
            StarMapLayoutKindDto::AutoRadial => Self::AutoRadial,
            StarMapLayoutKindDto::Custom => Self::Custom,
        }
    }
}

// Flattened struct (was tagged enum StarMapAnchorTargetDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchorTargetDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_id: Option<String>,
    pub entity_type: Option<String>,
    pub starmap_id: Option<String>,
    pub uri: Option<String>,
    pub payload: Option<String>,
}

impl From<crate::starmap::semantic::StarMapAnchorTarget> for StarMapAnchorTargetDto {
    fn from(t: crate::starmap::semantic::StarMapAnchorTarget) -> Self {
        match t {
            crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                project_id,
                volume_id,
                chapter_id,
                range_start,
                range_end,
            } => Self {
                kind: "chapterRange".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Project { project_id } => Self {
                kind: "project".to_string(),
                project_id: Some(project_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Volume {
                project_id,
                volume_id,
            } => Self {
                kind: "volume".to_string(),
                project_id,
                volume_id: Some(volume_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Chapter {
                project_id,
                volume_id,
                chapter_id,
            } => Self {
                kind: "chapter".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Character { entity_id } => Self {
                kind: "character".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("character".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Item { entity_id } => Self {
                kind: "item".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("item".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Location { entity_id } => Self {
                kind: "location".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("location".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Event { entity_id } => Self {
                kind: "event".to_string(),
                entity_id: Some(entity_id),
                entity_type: Some("event".to_string()),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Starmap { starmap_id } => Self {
                kind: "starmap".to_string(),
                starmap_id: Some(starmap_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::External { uri } => Self {
                kind: "external".to_string(),
                uri: Some(uri),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapAnchorTarget::Custom { payload } => Self {
                kind: "custom".to_string(),
                payload: Some(serde_json::to_string(&payload).unwrap_or_default()),
                ..Default::default()
            },
        }
    }
}

impl From<StarMapAnchorTargetDto> for crate::starmap::semantic::StarMapAnchorTarget {
    fn from(d: StarMapAnchorTargetDto) -> Self {
        match d.kind.as_str() {
            "project" => Self::Project {
                project_id: d.project_id.unwrap_or_default(),
            },
            "volume" => Self::Volume {
                project_id: d.project_id,
                volume_id: d.volume_id.unwrap_or_default(),
            },
            "chapter" => Self::Chapter {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
            },
            "character" => Self::Character {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "item" => Self::Item {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "location" => Self::Location {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "event" => Self::Event {
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap {
                starmap_id: d.starmap_id.unwrap_or_default(),
            },
            "external" => Self::External {
                uri: d.uri.unwrap_or_default(),
            },
            "custom" => Self::Custom {
                payload: d
                    .payload
                    .map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null),
            },
            _ => Self::ChapterRange {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
        }
    }
}

impl Default for StarMapAnchorTargetDto {
    fn default() -> Self {
        Self {
            kind: "chapterRange".to_string(),
            project_id: None,
            volume_id: None,
            chapter_id: None,
            range_start: None,
            range_end: None,
            entity_id: None,
            entity_type: None,
            starmap_id: None,
            uri: None,
            payload: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapAnchorRoleDto {
    #[default]
    Source,
    Destination,
    Reference,
    Custom,
}

impl From<crate::starmap::semantic::StarMapAnchorRole> for StarMapAnchorRoleDto {
    fn from(r: crate::starmap::semantic::StarMapAnchorRole) -> Self {
        match r {
            crate::starmap::semantic::StarMapAnchorRole::Source => Self::Source,
            crate::starmap::semantic::StarMapAnchorRole::Destination => Self::Destination,
            crate::starmap::semantic::StarMapAnchorRole::Reference => Self::Reference,
            crate::starmap::semantic::StarMapAnchorRole::Custom => Self::Custom,
        }
    }
}

impl From<StarMapAnchorRoleDto> for crate::starmap::semantic::StarMapAnchorRole {
    fn from(dto: StarMapAnchorRoleDto) -> Self {
        match dto {
            StarMapAnchorRoleDto::Source => Self::Source,
            StarMapAnchorRoleDto::Destination => Self::Destination,
            StarMapAnchorRoleDto::Reference => Self::Reference,
            StarMapAnchorRoleDto::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalModeDto {
    #[default]
    EnterChild,
    PreviewInline,
    ReferenceOnly,
}

impl From<crate::starmap::semantic::StarMapPortalMode> for StarMapPortalModeDto {
    fn from(m: crate::starmap::semantic::StarMapPortalMode) -> Self {
        match m {
            crate::starmap::semantic::StarMapPortalMode::EnterChild => Self::EnterChild,
            crate::starmap::semantic::StarMapPortalMode::PreviewInline => Self::PreviewInline,
            crate::starmap::semantic::StarMapPortalMode::ReferenceOnly => Self::ReferenceOnly,
        }
    }
}

impl From<StarMapPortalModeDto> for crate::starmap::semantic::StarMapPortalMode {
    fn from(dto: StarMapPortalModeDto) -> Self {
        match dto {
            StarMapPortalModeDto::EnterChild => Self::EnterChild,
            StarMapPortalModeDto::PreviewInline => Self::PreviewInline,
            StarMapPortalModeDto::ReferenceOnly => Self::ReferenceOnly,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalPreviewPolicyDto {
    #[default]
    Auto,
    Always,
    Never,
}

impl From<crate::starmap::semantic::StarMapPortalPreviewPolicy> for StarMapPortalPreviewPolicyDto {
    fn from(p: crate::starmap::semantic::StarMapPortalPreviewPolicy) -> Self {
        match p {
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Auto => Self::Auto,
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Always => Self::Always,
            crate::starmap::semantic::StarMapPortalPreviewPolicy::Never => Self::Never,
        }
    }
}

impl From<StarMapPortalPreviewPolicyDto> for crate::starmap::semantic::StarMapPortalPreviewPolicy {
    fn from(dto: StarMapPortalPreviewPolicyDto) -> Self {
        match dto {
            StarMapPortalPreviewPolicyDto::Auto => Self::Auto,
            StarMapPortalPreviewPolicyDto::Always => Self::Always,
            StarMapPortalPreviewPolicyDto::Never => Self::Never,
        }
    }
}

// Flattened struct (was tagged enum StarMapTargetDetailDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapTargetDetailDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
    pub project_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub range_start: Option<u32>,
    pub range_end: Option<u32>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub uri: Option<String>,
}

impl From<crate::starmap::semantic::StarMapTargetDetail> for StarMapTargetDetailDto {
    fn from(d: crate::starmap::semantic::StarMapTargetDetail) -> Self {
        match d {
            crate::starmap::semantic::StarMapTargetDetail::Starmap => Self {
                kind: "starmap".to_string(),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::ChapterRange {
                project_id,
                volume_id,
                chapter_id,
                range_start,
                range_end,
            } => Self {
                kind: "chapterRange".to_string(),
                project_id,
                volume_id,
                chapter_id: Some(chapter_id),
                range_start,
                range_end,
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::Entity {
                entity_type,
                entity_id,
            } => Self {
                kind: "entity".to_string(),
                entity_type: Some(entity_type),
                entity_id: Some(entity_id),
                ..Default::default()
            },
            crate::starmap::semantic::StarMapTargetDetail::External { uri } => Self {
                kind: "external".to_string(),
                uri: Some(uri),
                ..Default::default()
            },
        }
    }
}

impl From<StarMapTargetDetailDto> for crate::starmap::semantic::StarMapTargetDetail {
    fn from(d: StarMapTargetDetailDto) -> Self {
        match d.kind.as_str() {
            "node" => Self::Node {
                node_id: d.node_id.unwrap_or_default(),
            },
            "anchor" => Self::Anchor {
                node_id: d.node_id.unwrap_or_default(),
                anchor_id: d.anchor_id.unwrap_or_default(),
            },
            "chapterRange" => Self::ChapterRange {
                project_id: d.project_id,
                volume_id: d.volume_id,
                chapter_id: d.chapter_id.unwrap_or_default(),
                range_start: d.range_start,
                range_end: d.range_end,
            },
            "entity" => Self::Entity {
                entity_type: d.entity_type.unwrap_or_default(),
                entity_id: d.entity_id.unwrap_or_default(),
            },
            "external" => Self::External {
                uri: d.uri.unwrap_or_default(),
            },
            _ => Self::Starmap,
        }
    }
}

impl Default for StarMapTargetDetailDto {
    fn default() -> Self {
        Self {
            kind: "starmap".to_string(),
            node_id: None,
            anchor_id: None,
            project_id: None,
            volume_id: None,
            chapter_id: None,
            range_start: None,
            range_end: None,
            entity_type: None,
            entity_id: None,
            uri: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum StarMapSourceKindDto {
    Human,
    Import,
    Plugin,
    Ai,
    System,
    #[default]
    Unknown,
}

impl From<crate::starmap::semantic::StarMapSourceKind> for StarMapSourceKindDto {
    fn from(k: crate::starmap::semantic::StarMapSourceKind) -> Self {
        match k {
            crate::starmap::semantic::StarMapSourceKind::Human => Self::Human,
            crate::starmap::semantic::StarMapSourceKind::Import => Self::Import,
            crate::starmap::semantic::StarMapSourceKind::Plugin => Self::Plugin,
            crate::starmap::semantic::StarMapSourceKind::Ai => Self::Ai,
            crate::starmap::semantic::StarMapSourceKind::System => Self::System,
            crate::starmap::semantic::StarMapSourceKind::Unknown => Self::Unknown,
        }
    }
}

impl From<StarMapSourceKindDto> for crate::starmap::semantic::StarMapSourceKind {
    fn from(dto: StarMapSourceKindDto) -> Self {
        match dto {
            StarMapSourceKindDto::Human => Self::Human,
            StarMapSourceKindDto::Import => Self::Import,
            StarMapSourceKindDto::Plugin => Self::Plugin,
            StarMapSourceKindDto::Ai => Self::Ai,
            StarMapSourceKindDto::System => Self::System,
            StarMapSourceKindDto::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum StarMapReviewStatusDto {
    Accepted,
    Draft,
    NeedsReview,
    Rejected,
    #[default]
    Unknown,
}

impl From<crate::starmap::semantic::StarMapReviewStatus> for StarMapReviewStatusDto {
    fn from(s: crate::starmap::semantic::StarMapReviewStatus) -> Self {
        match s {
            crate::starmap::semantic::StarMapReviewStatus::Accepted => Self::Accepted,
            crate::starmap::semantic::StarMapReviewStatus::Draft => Self::Draft,
            crate::starmap::semantic::StarMapReviewStatus::NeedsReview => Self::NeedsReview,
            crate::starmap::semantic::StarMapReviewStatus::Rejected => Self::Rejected,
            crate::starmap::semantic::StarMapReviewStatus::Unknown => Self::Unknown,
        }
    }
}

impl From<StarMapReviewStatusDto> for crate::starmap::semantic::StarMapReviewStatus {
    fn from(dto: StarMapReviewStatusDto) -> Self {
        match dto {
            StarMapReviewStatusDto::Accepted => Self::Accepted,
            StarMapReviewStatusDto::Draft => Self::Draft,
            StarMapReviewStatusDto::NeedsReview => Self::NeedsReview,
            StarMapReviewStatusDto::Rejected => Self::Rejected,
            StarMapReviewStatusDto::Unknown => Self::Unknown,
        }
    }
}

// Flattened struct (was tagged enum StarMapEndpointDto)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: Option<String>,
    pub anchor_id: Option<String>,
}

impl From<crate::starmap::types::StarMapEndpoint> for StarMapEndpointDto {
    fn from(e: crate::starmap::types::StarMapEndpoint) -> Self {
        match e {
            crate::starmap::types::StarMapEndpoint::Node { node_id } => Self {
                kind: "node".to_string(),
                node_id: Some(node_id),
                anchor_id: None,
            },
            crate::starmap::types::StarMapEndpoint::Anchor { node_id, anchor_id } => Self {
                kind: "anchor".to_string(),
                node_id: Some(node_id),
                anchor_id: Some(anchor_id),
            },
            crate::starmap::types::StarMapEndpoint::Starmap => Self {
                kind: "starmap".to_string(),
                node_id: None,
                anchor_id: None,
            },
        }
    }
}

impl From<StarMapEndpointDto> for crate::starmap::types::StarMapEndpoint {
    fn from(dto: StarMapEndpointDto) -> Self {
        match dto.kind.as_str() {
            "anchor" => Self::Anchor {
                node_id: dto.node_id.unwrap_or_default(),
                anchor_id: dto.anchor_id.unwrap_or_default(),
            },
            "starmap" => Self::Starmap,
            _ => Self::Node {
                node_id: dto.node_id.unwrap_or_default(),
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapViewportDto {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<crate::starmap::types::StarMapViewport> for StarMapViewportDto {
    fn from(v: crate::starmap::types::StarMapViewport) -> Self {
        Self {
            scale: v.scale,
            offset_x: v.offset_x,
            offset_y: v.offset_y,
            width: v.width,
            height: v.height,
        }
    }
}

impl From<StarMapViewportDto> for crate::starmap::types::StarMapViewport {
    fn from(dto: StarMapViewportDto) -> Self {
        Self {
            scale: dto.scale,
            offset_x: dto.offset_x,
            offset_y: dto.offset_y,
            width: dto.width,
            height: dto.height,
        }
    }
}
