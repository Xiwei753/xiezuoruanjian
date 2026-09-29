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
    pub position: StarMapPointDto,
    #[serde(default)]
    pub style: StarMapNodeStyleDto,
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
            position: n.position.into(),
            style: n.style.into(),
            provenance: n.provenance.into(),
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

impl TryFrom<StarMapNodeDto> for crate::starmap::types::StarMapNode {
    type Error = crate::error::Error;

    fn try_from(d: StarMapNodeDto) -> Result<Self, Self::Error> {
        let payload = d
            .payload
            .map(|s| serde_json::from_str(&s).map_err(crate::error::Error::from))
            .transpose()?;
        Ok(Self {
            id: d.id,
            title: d.title,
            kind: d.kind.into(),
            payload,
            tags: d.tags,
            content: d.content.try_into()?,
            anchors: d
                .anchors
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            portal: d.portal.map(|p| p.try_into()).transpose()?,
            position: d.position.into(),
            style: d.style.into(),
            provenance: d.provenance.into(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

// Issue #791 评论 5883783849: 手写 Default 返回合法的 kind:"empty"，
// 避免派生 Default 产生 kind:"" 导致 TryFrom 报 "unknown node content kind:"。
impl Default for StarMapNodeContentDto {
    fn default() -> Self {
        Self {
            kind: "empty".to_string(),
            summary: None,
            body: None,
            project_id: None,
            volume_id: None,
            chapter_id: None,
            range_start: None,
            range_end: None,
            entity_type: None,
            entity_id: None,
            uri: None,
            label: None,
        }
    }
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

impl TryFrom<StarMapNodeContentDto> for crate::starmap::semantic::StarMapNodeContent {
    type Error = crate::error::Error;

    fn try_from(d: StarMapNodeContentDto) -> Result<Self, Self::Error> {
        match d.kind.as_str() {
            "empty" => Ok(Self::Empty),
            "inline" => Ok(Self::Inline {
                summary: d.summary,
                body: d.body,
            }),
            "chapterRef" => {
                let project_id = d.project_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field project_id for kind 'chapterRef'".into(),
                    )
                })?;
                let chapter_id = d.chapter_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field chapter_id for kind 'chapterRef'".into(),
                    )
                })?;
                Ok(Self::ChapterRef {
                    project_id,
                    volume_id: d.volume_id,
                    chapter_id,
                    range_start: d.range_start,
                    range_end: d.range_end,
                })
            }
            "entityRef" => {
                let entity_type = d.entity_type.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_type for kind 'entityRef'".into(),
                    )
                })?;
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'entityRef'".into(),
                    )
                })?;
                Ok(Self::EntityRef {
                    entity_type,
                    entity_id,
                })
            }
            "externalRef" => {
                let uri = d.uri.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field uri for kind 'externalRef'".into(),
                    )
                })?;
                Ok(Self::ExternalRef {
                    uri,
                    label: d.label,
                })
            }
            unknown => Err(crate::error::Error::Other(format!(
                "unknown node content kind: {}",
                unknown
            ))),
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
    pub position: Option<StarMapPointDto>,
    pub style: Option<StarMapNodeStyleDto>,
    pub provenance: Option<StarMapProvenanceDto>,
}

impl TryFrom<StarMapNodePatchDto> for crate::starmap::types::StarMapNodePatch {
    type Error = crate::error::Error;

    fn try_from(d: StarMapNodePatchDto) -> Result<Self, Self::Error> {
        let payload = d
            .payload
            .map(|opt| {
                opt.map(|s| serde_json::from_str(&s).map_err(crate::error::Error::from))
                    .transpose()
            })
            .transpose()?;
        Ok(Self {
            title: d.title,
            kind: d.kind.map(Into::into),
            payload,
            tags: d.tags,
            content: d.content.map(|c| c.try_into()).transpose()?,
            anchors: d
                .anchors
                .map(|v| {
                    v.into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?,
            portal: d
                .portal
                .map(|p| p.map(|inner| inner.try_into()).transpose())
                .transpose()?,
            position: d.position.map(Into::into),
            style: d.style.map(Into::into),
            provenance: d.provenance.map(Into::into),
        })
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatchInputDto {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKindDto>,
    pub payload: Option<String>,
    /// JSON C-ABI（Harmony）只发要改的字段，省略 `clearPayload` 即"不清空"。
    #[serde(default)]
    pub clear_payload: bool,
    pub tags: Option<Vec<String>>,
    pub content: Option<StarMapNodeContentDto>,
    pub anchors: Option<Vec<StarMapAnchorDto>>,
    pub portal: Option<StarMapPortalDto>,
    /// 同 `clear_payload`：省略即"不清空 portal"。
    #[serde(default)]
    pub clear_portal: bool,
    pub position: Option<StarMapPointDto>,
    pub style: Option<StarMapNodeStyleDto>,
    pub provenance: Option<StarMapProvenanceDto>,
}

impl From<StarMapNodePatchInputDto> for StarMapNodePatchDto {
    fn from(d: StarMapNodePatchInputDto) -> Self {
        Self {
            title: d.title,
            kind: d.kind,
            payload: if d.clear_payload {
                Some(None)
            } else {
                d.payload.map(Some)
            },
            tags: d.tags,
            content: d.content,
            anchors: d.anchors,
            portal: if d.clear_portal {
                Some(None)
            } else {
                d.portal.map(Some)
            },
            position: d.position,
            style: d.style,
            provenance: d.provenance,
        }
    }
}
