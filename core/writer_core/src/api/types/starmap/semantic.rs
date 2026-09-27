//! # 星图语义 DTO — 锚点、目标、Portal 的跨语言类型
//!
//! `StarMapAnchorTargetDto` 使用 `kind` 字符串判别 + 扁平 Optional 字段模式
//! （而非 Rust 枚举），因为 JSON 线格式需要跨语言可解析。
//! `range_start`/`range_end` 为 UTF-8 byte offset（半开区间）。

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

impl TryFrom<StarMapAnchorDto> for crate::starmap::semantic::StarMapAnchor {
    type Error = crate::error::Error;

    fn try_from(d: StarMapAnchorDto) -> Result<Self, Self::Error> {
        Ok(Self {
            anchor_id: d.anchor_id,
            target: d.target.try_into()?,
            label: d.label,
            role: d.role.into(),
        })
    }
}

/// 锚点目标 DTO — 使用 `kind` 字符串判别 + 扁平 Optional 字段。
/// `range_start`/`range_end` 为 UTF-8 byte offset（半开区间 `[start, end)`），
/// 仅在 `kind == "chapterRange"` 时有值。
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

impl TryFrom<StarMapAnchorTargetDto> for crate::starmap::semantic::StarMapAnchorTarget {
    type Error = crate::error::Error;

    #[allow(clippy::too_many_lines)]
    fn try_from(d: StarMapAnchorTargetDto) -> Result<Self, Self::Error> {
        match d.kind.as_str() {
            "project" => {
                let project_id = d.project_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field project_id for kind 'project'".into(),
                    )
                })?;
                Ok(Self::Project { project_id })
            }
            "volume" => {
                let volume_id = d.volume_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field volume_id for kind 'volume'".into(),
                    )
                })?;
                Ok(Self::Volume {
                    project_id: d.project_id,
                    volume_id,
                })
            }
            "chapter" => {
                let chapter_id = d.chapter_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field chapter_id for kind 'chapter'".into(),
                    )
                })?;
                Ok(Self::Chapter {
                    project_id: d.project_id,
                    volume_id: d.volume_id,
                    chapter_id,
                })
            }
            "chapterRange" => {
                let chapter_id = d.chapter_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field chapter_id for kind 'chapterRange'".into(),
                    )
                })?;
                Ok(Self::ChapterRange {
                    project_id: d.project_id,
                    volume_id: d.volume_id,
                    chapter_id,
                    range_start: d.range_start,
                    range_end: d.range_end,
                })
            }
            "character" => {
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'character'".into(),
                    )
                })?;
                Ok(Self::Character { entity_id })
            }
            "item" => {
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'item'".into(),
                    )
                })?;
                Ok(Self::Item { entity_id })
            }
            "location" => {
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'location'".into(),
                    )
                })?;
                Ok(Self::Location { entity_id })
            }
            "event" => {
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'event'".into(),
                    )
                })?;
                Ok(Self::Event { entity_id })
            }
            "starmap" => {
                let starmap_id = d.starmap_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field starmap_id for kind 'starmap'".into(),
                    )
                })?;
                Ok(Self::Starmap { starmap_id })
            }
            "external" => {
                let uri = d.uri.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field uri for kind 'external'".into(),
                    )
                })?;
                Ok(Self::External { uri })
            }
            "custom" => {
                let payload_str = d.payload.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field payload for kind 'custom'".into(),
                    )
                })?;
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_str).map_err(crate::error::Error::from)?;
                Ok(Self::Custom { payload })
            }
            unknown => Err(crate::error::Error::Other(format!(
                "unknown anchor target kind: {}",
                unknown
            ))),
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

/// Portal DTO — 跳转定义，只保留目标星图 ID 和可选落点。
/// 显示/交互策略（mode、preview_policy）已退出 Core。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortalDto {
    pub destination_starmap_id: String,
    #[serde(default)]
    pub destination_target: Option<StarMapTargetDetailDto>,
}

impl From<crate::starmap::semantic::StarMapPortal> for StarMapPortalDto {
    fn from(p: crate::starmap::semantic::StarMapPortal) -> Self {
        Self {
            destination_starmap_id: p.destination_starmap_id,
            destination_target: p.destination_target.map(Into::into),
        }
    }
}

impl TryFrom<StarMapPortalDto> for crate::starmap::semantic::StarMapPortal {
    type Error = crate::error::Error;

    fn try_from(d: StarMapPortalDto) -> Result<Self, Self::Error> {
        Ok(Self {
            destination_starmap_id: d.destination_starmap_id,
            destination_target: d.destination_target.map(|t| t.try_into()).transpose()?,
        })
    }
}

/// 节点位置 DTO — 星图文档坐标系下的 (x, y)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPointDto {
    pub x: f32,
    pub y: f32,
}

impl From<crate::starmap::types::StarMapPoint> for StarMapPointDto {
    fn from(p: crate::starmap::types::StarMapPoint) -> Self {
        Self { x: p.x, y: p.y }
    }
}

impl From<StarMapPointDto> for crate::starmap::types::StarMapPoint {
    fn from(d: StarMapPointDto) -> Self {
        Self { x: d.x, y: d.y }
    }
}

/// 节点样式 DTO — 纯数据层的外观属性。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeStyleDto {
    #[serde(default)]
    pub fill_color: Option<String>,
}

impl From<crate::starmap::types::StarMapNodeStyle> for StarMapNodeStyleDto {
    fn from(s: crate::starmap::types::StarMapNodeStyle) -> Self {
        Self {
            fill_color: s.fill_color,
        }
    }
}

impl From<StarMapNodeStyleDto> for crate::starmap::types::StarMapNodeStyle {
    fn from(d: StarMapNodeStyleDto) -> Self {
        Self {
            fill_color: d.fill_color,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StarMapTargetPathDto {
    pub starmap_id: String,
    #[serde(default)]
    pub segments: Vec<StarMapPathSegmentDto>,
    pub target: StarMapTargetDetailDto,
}

impl From<crate::starmap::types::reference::StarMapTargetPath> for StarMapTargetPathDto {
    fn from(p: crate::starmap::types::reference::StarMapTargetPath) -> Self {
        Self {
            starmap_id: p.starmap_id,
            segments: p.segments.into_iter().map(Into::into).collect(),
            target: p.target.into(),
        }
    }
}

impl TryFrom<StarMapTargetPathDto> for crate::starmap::types::reference::StarMapTargetPath {
    type Error = crate::error::Error;

    fn try_from(d: StarMapTargetPathDto) -> Result<Self, Self::Error> {
        Ok(Self {
            starmap_id: d.starmap_id,
            segments: d
                .segments
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            target: d.target.try_into()?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPathSegmentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub instance_id: Option<String>,
    pub node_id: Option<String>,
}

impl From<crate::starmap::types::reference::StarMapPathSegment> for StarMapPathSegmentDto {
    fn from(s: crate::starmap::types::reference::StarMapPathSegment) -> Self {
        match s {
            crate::starmap::types::reference::StarMapPathSegment::EnterEmbed { instance_id } => {
                Self {
                    kind: "enterEmbed".to_string(),
                    instance_id: Some(instance_id),
                    node_id: None,
                }
            }
            crate::starmap::types::reference::StarMapPathSegment::EnterPortal { node_id } => Self {
                kind: "enterPortal".to_string(),
                instance_id: None,
                node_id: Some(node_id),
            },
        }
    }
}

impl TryFrom<StarMapPathSegmentDto> for crate::starmap::types::reference::StarMapPathSegment {
    type Error = crate::error::Error;

    fn try_from(d: StarMapPathSegmentDto) -> Result<Self, Self::Error> {
        match d.kind.as_str() {
            "enterEmbed" => {
                let instance_id = d.instance_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field instance_id for kind 'enterEmbed'".into(),
                    )
                })?;
                Ok(Self::EnterEmbed { instance_id })
            }
            "enterPortal" => {
                let node_id = d.node_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field node_id for kind 'enterPortal'".into(),
                    )
                })?;
                Ok(Self::EnterPortal { node_id })
            }
            unknown => Err(crate::error::Error::Other(format!(
                "unknown path segment kind: {}",
                unknown
            ))),
        }
    }
}

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

impl TryFrom<StarMapTargetDetailDto> for crate::starmap::semantic::StarMapTargetDetail {
    type Error = crate::error::Error;

    fn try_from(d: StarMapTargetDetailDto) -> Result<Self, Self::Error> {
        match d.kind.as_str() {
            "starmap" => Ok(Self::Starmap),
            "node" => {
                let node_id = d.node_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field node_id for kind 'node'".into(),
                    )
                })?;
                Ok(Self::Node { node_id })
            }
            "anchor" => {
                let node_id = d.node_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field node_id for kind 'anchor'".into(),
                    )
                })?;
                let anchor_id = d.anchor_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field anchor_id for kind 'anchor'".into(),
                    )
                })?;
                Ok(Self::Anchor { node_id, anchor_id })
            }
            "chapterRange" => {
                let chapter_id = d.chapter_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field chapter_id for kind 'chapterRange'".into(),
                    )
                })?;
                Ok(Self::ChapterRange {
                    project_id: d.project_id,
                    volume_id: d.volume_id,
                    chapter_id,
                    range_start: d.range_start,
                    range_end: d.range_end,
                })
            }
            "entity" => {
                let entity_type = d.entity_type.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_type for kind 'entity'".into(),
                    )
                })?;
                let entity_id = d.entity_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field entity_id for kind 'entity'".into(),
                    )
                })?;
                Ok(Self::Entity {
                    entity_type,
                    entity_id,
                })
            }
            "external" => {
                let uri = d.uri.filter(|s| !s.is_empty()).ok_or_else(|| {
                    crate::error::Error::Other(
                        "missing or empty required field uri for kind 'external'".into(),
                    )
                })?;
                Ok(Self::External { uri })
            }
            unknown => Err(crate::error::Error::Other(format!(
                "unknown target detail kind: {}",
                unknown
            ))),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

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
