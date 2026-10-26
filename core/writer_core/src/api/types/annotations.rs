//! DTOs for the shared annotation model exposed to platform clients.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationObjectKindDto {
    Project,
    Volume,
    Chapter,
    TextAnchor,
    StarMap,
    StarMapNode,
    StarMapEmbed,
    NamedEntry,
}

/// Stable reference to an object in its owning project. For text anchors,
/// `object_id` is the anchor id; the anchor record contains its chapter id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationObjectRefDto {
    pub kind: AnnotationObjectKindDto,
    pub project_id: String,
    pub object_id: String,
}

/// Exactly one of `object_ref` and `external_url` is populated for link targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTargetDto {
    pub object_ref: Option<AnnotationObjectRefDto>,
    pub external_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTagDto {
    pub tag_id: String,
    pub project_id: String,
    pub name: String,
    pub color: Option<String>,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTagBindingDto {
    pub binding_id: String,
    pub project_id: String,
    pub tag_id: String,
    pub target: AnnotationObjectRefDto,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTextAnchorDto {
    pub anchor_id: String,
    pub project_id: String,
    pub chapter_id: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub text_revision: u64,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationNamedEntryDto {
    pub entry_id: String,
    pub project_id: String,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub confirmed_misspellings: Vec<String>,
    pub auto_link_enabled: bool,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationLinkOriginDto {
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationObjectLinkDto {
    pub link_id: String,
    pub source: AnnotationObjectRefDto,
    pub target: AnnotationTargetDto,
    pub origin: AnnotationLinkOriginDto,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationLinkResolutionStateDto {
    Exists,
    Missing,
    ProjectNotDownloaded,
    ExternalUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationLinkResolutionDto {
    pub state: AnnotationLinkResolutionStateDto,
    pub target: AnnotationTargetDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationAutoMatchDto {
    pub start_byte: u32,
    pub end_byte: u32,
    pub matched_text: String,
    pub entry_ids: Vec<String>,
    pub candidates: Vec<AnnotationObjectRefDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAnnotationsDto {
    pub project_id: String,
    pub chapter_id: String,
    pub text_revision: u64,
    pub anchors: Vec<AnnotationTextAnchorDto>,
    pub bindings: Vec<AnnotationTagBindingDto>,
    pub links: Vec<AnnotationObjectLinkDto>,
    pub auto_matches: Vec<AnnotationAutoMatchDto>,
}

/// One committed editor transaction's byte-range mapping. Both intervals use
/// UTF-8 byte offsets and half-open bounds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationEditDto {
    pub old_start_byte: u32,
    pub old_end_byte: u32,
    pub new_start_byte: u32,
    pub new_end_byte: u32,
}
