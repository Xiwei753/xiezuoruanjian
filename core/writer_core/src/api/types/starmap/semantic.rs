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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPathSegmentDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub starmap_id: Option<String>,
}

impl From<crate::starmap::semantic::StarMapPathSegment> for StarMapPathSegmentDto {
    fn from(s: crate::starmap::semantic::StarMapPathSegment) -> Self {
        match s {
            crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id } => Self {
                kind: "enterChild".to_string(),
                starmap_id: Some(starmap_id),
            },
        }
    }
}

impl From<StarMapPathSegmentDto> for crate::starmap::semantic::StarMapPathSegment {
    fn from(d: StarMapPathSegmentDto) -> Self {
        Self::EnterChild {
            starmap_id: d.starmap_id.unwrap_or_default(),
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
