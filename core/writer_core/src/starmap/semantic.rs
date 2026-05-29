use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapNodeContent {
    #[default]
    Empty,
    Inline {
        summary: Option<String>,
        body: Option<String>,
    },
    ChapterRef {
        project_id: String,
        volume_id: Option<String>,
        chapter_id: String,
        range_start: Option<u32>,
        range_end: Option<u32>,
    },
    EntityRef {
        entity_type: String,
        entity_id: String,
    },
    ExternalRef {
        uri: String,
        label: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAnchor {
    pub anchor_id: String,
    pub target: StarMapAnchorTarget,
    pub label: Option<String>,
    #[serde(default)]
    pub role: StarMapAnchorRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapAnchorTarget {
    ChapterRange {
        project_id: Option<String>,
        volume_id: Option<String>,
        chapter_id: String,
        range_start: Option<u32>,
        range_end: Option<u32>,
    },
    Project {
        project_id: String,
    },
    Volume {
        project_id: Option<String>,
        volume_id: String,
    },
    Chapter {
        project_id: Option<String>,
        volume_id: Option<String>,
        chapter_id: String,
    },
    Character {
        entity_id: String,
    },
    Item {
        entity_id: String,
    },
    Location {
        entity_id: String,
    },
    Event {
        entity_id: String,
    },
    Starmap {
        starmap_id: String,
    },
    External {
        uri: String,
    },
    Custom {
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapAnchorRole {
    Source,
    Destination,
    Reference,
    #[serde(other)]
    Custom,
}

impl Default for StarMapAnchorRole {
    fn default() -> Self {
        StarMapAnchorRole::Reference
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPortal {
    pub target_starmap_id: String,
    #[serde(default)]
    pub mode: StarMapPortalMode,
    #[serde(default)]
    pub preview_policy: StarMapPortalPreviewPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalMode {
    EnterChild,
    PreviewInline,
    ReferenceOnly,
}

impl Default for StarMapPortalMode {
    fn default() -> Self {
        StarMapPortalMode::ReferenceOnly
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StarMapPortalPreviewPolicy {
    Auto,
    Always,
    Never,
}

impl Default for StarMapPortalPreviewPolicy {
    fn default() -> Self {
        StarMapPortalPreviewPolicy::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDisplayPolicy {
    pub importance: f32,
    pub min_visible_scale: f32,
    pub title_scale: f32,
    pub summary_scale: f32,
    pub detail_scale: f32,
    pub max_preview_chars: u32,
    pub min_readable_px: f32,
}

impl Default for StarMapDisplayPolicy {
    fn default() -> Self {
        Self {
            importance: 1.0,
            min_visible_scale: 0.1,
            title_scale: 0.2,
            summary_scale: 0.5,
            detail_scale: 1.0,
            max_preview_chars: 100,
            min_readable_px: 12.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapOpenBehavior {
    #[default]
    Inspector,
    ExpandCard,
    WritingMode,
    JumpToAnchor,
    EnterPortal,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProvenance {
    #[serde(default)]
    pub source: StarMapSourceKind,
    pub source_id: Option<String>,
    pub generated_by: Option<String>,
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub review_status: StarMapReviewStatus,
    pub created_from_anchor: Option<String>,
}

impl Default for StarMapProvenance {
    fn default() -> Self {
        Self {
            source: StarMapSourceKind::Human,
            source_id: None,
            generated_by: None,
            prompt_id: None,
            review_status: StarMapReviewStatus::Accepted,
            created_from_anchor: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapSourceKind {
    #[default]
    Human,
    Import,
    Plugin,
    Ai,
    System,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StarMapReviewStatus {
    #[default]
    Accepted,
    Draft,
    NeedsReview,
    Rejected,
    #[serde(other)]
    Unknown,
}
