//! DTOs for the shared annotation model exposed to platform clients.
//!
//! Issue #1060：对象引用按 typed [`ObjectRef`] 的变体一一对应，不再把
//! "星图节点"混成作品对象。旧 JSON（`kind:"starMapNode"` + `projectId`）
//! 仍可反序列化，并归一化到 `nodeAsset`。

use serde::{Deserialize, Serialize};

use super::StarMapTargetPathDto;
use crate::error::Error;
use crate::object_ref::{ObjectRef, ObjectScope};

/// 注解/链接可以指向的对象种类。
///
/// 与 [`ObjectRef`] 的变体一一对应：共享素材（`NodeAsset`/`StarMap`）与画布
/// 实例（`NodeInstance`/`EmbedInstance`/`CanvasAnchor`）是不同身份。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationObjectKindDto {
    Project,
    Volume,
    Chapter,
    TextAnchor,
    NamedEntry,
    StarMap,
    /// 共享节点素材；旧 DTO 名称是 `starMapNode`。
    #[serde(rename = "nodeAsset", alias = "starMapNode")]
    NodeAsset,
    /// 画布节点实例（实例 ID 只在画布内唯一）。
    #[serde(rename = "nodeInstance")]
    NodeInstance,
    /// 画布嵌入实例；旧 DTO 名称是 `starMapEmbed`。
    #[serde(rename = "embedInstance", alias = "starMapEmbed")]
    EmbedInstance,
    /// 星图节点连接端口（与正文文本锚点不同）。
    #[serde(rename = "canvasAnchor")]
    CanvasAnchor,
}

/// 旧 DTO 种类名归一化。
///
/// 旧记录只有 `starMapNode`/`starMapEmbed`；新模型把共享素材和画布实例拆开。
pub fn normalize_legacy_kind(name: &str) -> Option<AnnotationObjectKindDto> {
    Some(match name {
        "project" => AnnotationObjectKindDto::Project,
        "volume" => AnnotationObjectKindDto::Volume,
        "chapter" => AnnotationObjectKindDto::Chapter,
        "textAnchor" => AnnotationObjectKindDto::TextAnchor,
        "namedEntry" => AnnotationObjectKindDto::NamedEntry,
        "starMap" => AnnotationObjectKindDto::StarMap,
        "nodeAsset" | "starMapNode" => AnnotationObjectKindDto::NodeAsset,
        "nodeInstance" => AnnotationObjectKindDto::NodeInstance,
        "embedInstance" | "starMapEmbed" => AnnotationObjectKindDto::EmbedInstance,
        "canvasAnchor" => AnnotationObjectKindDto::CanvasAnchor,
        _ => return None,
    })
}

/// 稳定对象引用。
///
/// `project_id` 为空表示全局素材域；`starmap_id` 用于画布实例与画布锚点；
/// `anchor_id` 只用于画布锚点。缺省字段由下面的 custom `Deserialize` 按空
/// 处理（旧 JSON 只有 `projectId` + `objectId`），转换成 [`ObjectRef`] 时再
/// 报缺字段的明确错误。
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationObjectRefDto {
    pub kind: AnnotationObjectKindDto,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub starmap_id: Option<String>,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub anchor_id: Option<String>,
}

impl<'de> Deserialize<'de> for AnnotationObjectRefDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawObjectRef {
            kind: String,
            #[serde(default)]
            project_id: Option<String>,
            #[serde(default)]
            starmap_id: Option<String>,
            #[serde(default)]
            object_id: String,
            #[serde(default)]
            anchor_id: Option<String>,
        }

        let raw = RawObjectRef::deserialize(deserializer)?;
        let kind = normalize_legacy_kind(&raw.kind).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown annotation object kind `{}`", raw.kind))
        })?;
        Ok(Self {
            kind,
            project_id: raw.project_id,
            starmap_id: raw.starmap_id,
            object_id: raw.object_id,
            anchor_id: raw.anchor_id,
        })
    }
}

impl From<ObjectRef> for AnnotationObjectRefDto {
    fn from(value: ObjectRef) -> Self {
        match value {
            ObjectRef::Project {
                project_id,
                object_id,
            } => Self::project_scoped(AnnotationObjectKindDto::Project, project_id, object_id),
            ObjectRef::Volume {
                project_id,
                object_id,
            } => Self::project_scoped(AnnotationObjectKindDto::Volume, project_id, object_id),
            ObjectRef::Chapter {
                project_id,
                object_id,
            } => Self::project_scoped(AnnotationObjectKindDto::Chapter, project_id, object_id),
            ObjectRef::TextAnchor {
                project_id,
                object_id,
            } => Self::project_scoped(AnnotationObjectKindDto::TextAnchor, project_id, object_id),
            ObjectRef::NamedEntry {
                project_id,
                object_id,
            } => Self::project_scoped(AnnotationObjectKindDto::NamedEntry, project_id, object_id),
            ObjectRef::StarMap { scope, object_id } => {
                Self::scoped(AnnotationObjectKindDto::StarMap, scope, object_id)
            }
            ObjectRef::NodeAsset { scope, object_id } => {
                Self::scoped(AnnotationObjectKindDto::NodeAsset, scope, object_id)
            }
            ObjectRef::NodeInstance {
                starmap_id,
                object_id,
            } => Self::instance(AnnotationObjectKindDto::NodeInstance, starmap_id, object_id),
            ObjectRef::EmbedInstance {
                starmap_id,
                object_id,
            } => Self::instance(
                AnnotationObjectKindDto::EmbedInstance,
                starmap_id,
                object_id,
            ),
            ObjectRef::CanvasAnchor {
                starmap_id,
                object_id,
                anchor_id,
            } => Self {
                kind: AnnotationObjectKindDto::CanvasAnchor,
                project_id: None,
                starmap_id: Some(starmap_id),
                object_id,
                anchor_id: Some(anchor_id),
            },
        }
    }
}

impl AnnotationObjectRefDto {
    fn project_scoped(
        kind: AnnotationObjectKindDto,
        project_id: String,
        object_id: String,
    ) -> Self {
        Self {
            kind,
            project_id: Some(project_id),
            starmap_id: None,
            object_id,
            anchor_id: None,
        }
    }

    fn scoped(kind: AnnotationObjectKindDto, scope: ObjectScope, object_id: String) -> Self {
        Self {
            kind,
            project_id: scope.project_id().map(str::to_string),
            starmap_id: None,
            object_id,
            anchor_id: None,
        }
    }

    fn instance(kind: AnnotationObjectKindDto, starmap_id: String, object_id: String) -> Self {
        Self {
            kind,
            project_id: None,
            starmap_id: Some(starmap_id),
            object_id,
            anchor_id: None,
        }
    }
}

impl TryFrom<AnnotationObjectRefDto> for ObjectRef {
    type Error = Error;

    fn try_from(value: AnnotationObjectRefDto) -> Result<Self, Self::Error> {
        let object_id = value.object_id;
        if object_id.is_empty() {
            return Err(Error::Other(
                "annotation object id must be non-empty".to_string(),
            ));
        }
        let project_id = value.project_id.filter(|id| !id.is_empty());
        if let Some(project_id) = project_id.as_deref() {
            validate_safe_segment(project_id, "projectId")?;
        }
        let starmap_id = value.starmap_id.filter(|id| !id.is_empty());
        if let Some(starmap_id) = starmap_id.as_deref() {
            validate_safe_segment(starmap_id, "starmapId")?;
        }
        Ok(match value.kind {
            AnnotationObjectKindDto::Project => ObjectRef::Project {
                project_id: require_project_id(project_id, "project")?,
                object_id,
            },
            AnnotationObjectKindDto::Volume => ObjectRef::Volume {
                project_id: require_project_id(project_id, "volume")?,
                object_id,
            },
            AnnotationObjectKindDto::Chapter => ObjectRef::Chapter {
                project_id: require_project_id(project_id, "chapter")?,
                object_id,
            },
            AnnotationObjectKindDto::TextAnchor => ObjectRef::TextAnchor {
                project_id: require_project_id(project_id, "text anchor")?,
                object_id,
            },
            AnnotationObjectKindDto::NamedEntry => ObjectRef::NamedEntry {
                project_id: require_project_id(project_id, "named entry")?,
                object_id,
            },
            AnnotationObjectKindDto::StarMap => ObjectRef::StarMap {
                scope: ObjectScope::from_project_id(project_id.as_deref()),
                object_id,
            },
            AnnotationObjectKindDto::NodeAsset => ObjectRef::NodeAsset {
                scope: ObjectScope::from_project_id(project_id.as_deref()),
                object_id,
            },
            AnnotationObjectKindDto::NodeInstance => ObjectRef::NodeInstance {
                starmap_id: require_starmap_id(starmap_id, "node instance")?,
                object_id,
            },
            AnnotationObjectKindDto::EmbedInstance => ObjectRef::EmbedInstance {
                starmap_id: require_starmap_id(starmap_id, "embed instance")?,
                object_id,
            },
            AnnotationObjectKindDto::CanvasAnchor => ObjectRef::CanvasAnchor {
                starmap_id: require_starmap_id(starmap_id, "canvas anchor")?,
                object_id,
                anchor_id: require_anchor_id(value.anchor_id)?,
            },
        })
    }
}

fn require_project_id(project_id: Option<String>, kind: &str) -> Result<String, Error> {
    project_id.ok_or_else(|| Error::Other(format!("{kind} reference requires projectId")))
}

fn require_starmap_id(starmap_id: Option<String>, kind: &str) -> Result<String, Error> {
    starmap_id.ok_or_else(|| Error::Other(format!("{kind} reference requires starmapId")))
}

fn require_anchor_id(anchor_id: Option<String>) -> Result<String, Error> {
    anchor_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::Other("canvas anchor reference requires anchorId".to_string()))
}

fn validate_safe_segment(value: &str, field: &str) -> Result<(), Error> {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{field} is not a safe path component"
        )))
    }
}

/// Reference to an internal object or a canvas navigation route that an
/// annotation link points at.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTargetDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<AnnotationObjectRefDto>,
    /// 画布导航：保留完整路径（起点、穿越段、终点）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_route: Option<StarMapTargetPathDto>,
}

impl From<crate::annotations::LinkTarget> for AnnotationTargetDto {
    fn from(value: crate::annotations::LinkTarget) -> Self {
        match value {
            crate::annotations::LinkTarget::Object(object_ref) => Self {
                object_ref: Some(object_ref.into()),
                canvas_route: None,
            },
            crate::annotations::LinkTarget::CanvasNavigation(route) => Self {
                object_ref: None,
                canvas_route: Some(route.into()),
            },
        }
    }
}

impl TryFrom<AnnotationTargetDto> for crate::annotations::LinkTarget {
    type Error = Error;

    fn try_from(value: AnnotationTargetDto) -> Result<Self, Self::Error> {
        match (value.object_ref, value.canvas_route) {
            (Some(object_ref), None) => Ok(Self::Object(object_ref.try_into()?)),
            (None, Some(route)) => Ok(Self::CanvasNavigation(route.try_into()?)),
            (Some(_), Some(_)) => Err(Error::Other(
                "annotation target cannot be both an object and a canvas route".to_string(),
            )),
            (None, None) => Err(Error::Other(
                "annotation target requires objectRef or canvasRoute".to_string(),
            )),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationObjectLinkDto {
    pub link_id: String,
    pub source: AnnotationObjectRefDto,
    pub target: AnnotationTargetDto,
    pub origin: AnnotationLinkOriginDto,
    #[serde(default)]
    pub label: Option<String>,
    /// 对象级创建时间（毫秒）；旧记录迁移时由秒换算。
    #[serde(default)]
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationLinkResolutionStateDto {
    Exists,
    Missing,
    ProjectNotDownloaded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAnnotationsDto {
    pub project_id: String,
    pub chapter_id: String,
    /// Opaque in-memory token binding these ranges to this loaded text
    /// snapshot. The token is echoed when applying the session's edit maps.
    pub baseline_id: String,
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
