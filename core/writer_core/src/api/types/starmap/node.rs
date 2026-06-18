use super::*;

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