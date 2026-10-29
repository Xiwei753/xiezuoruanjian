use serde::{Deserialize, Serialize};

/// The flat, user-editable tag names installed when a new project is created.
pub const DEFAULT_TAG_NAMES: [&str; 14] = [
    "角色", "主角", "配角", "反派", "势力", "地点", "装备", "道具", "能力", "生物", "事件", "设定",
    "伏笔", "线索",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub tag_id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObjectRef {
    Project {
        project_id: String,
        object_id: String,
    },
    Volume {
        project_id: String,
        object_id: String,
    },
    Chapter {
        project_id: String,
        object_id: String,
    },
    TextAnchor {
        project_id: String,
        object_id: String,
    },
    StarMap {
        project_id: String,
        object_id: String,
    },
    StarMapNode {
        project_id: String,
        object_id: String,
    },
    StarMapEmbed {
        project_id: String,
        object_id: String,
    },
    NamedEntry {
        project_id: String,
        object_id: String,
    },
}

impl ObjectRef {
    pub fn project_id(&self) -> &str {
        match self {
            Self::Project { project_id, .. }
            | Self::Volume { project_id, .. }
            | Self::Chapter { project_id, .. }
            | Self::TextAnchor { project_id, .. }
            | Self::StarMap { project_id, .. }
            | Self::StarMapNode { project_id, .. }
            | Self::StarMapEmbed { project_id, .. }
            | Self::NamedEntry { project_id, .. } => project_id,
        }
    }

    pub fn object_id(&self) -> &str {
        match self {
            Self::Project { object_id, .. }
            | Self::Volume { object_id, .. }
            | Self::Chapter { object_id, .. }
            | Self::TextAnchor { object_id, .. }
            | Self::StarMap { object_id, .. }
            | Self::StarMapNode { object_id, .. }
            | Self::StarMapEmbed { object_id, .. }
            | Self::NamedEntry { object_id, .. } => object_id,
        }
    }

    pub fn kind(&self) -> AnnotationKind {
        match self {
            Self::Project { .. } => AnnotationKind::Project,
            Self::Volume { .. } => AnnotationKind::Volume,
            Self::Chapter { .. } => AnnotationKind::Chapter,
            Self::TextAnchor { .. } => AnnotationKind::TextAnchor,
            Self::StarMap { .. } => AnnotationKind::StarMap,
            Self::StarMapNode { .. } => AnnotationKind::StarMapNode,
            Self::StarMapEmbed { .. } => AnnotationKind::StarMapEmbed,
            Self::NamedEntry { .. } => AnnotationKind::NamedEntry,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagBinding {
    pub binding_id: String,
    pub project_id: String,
    pub tag_id: String,
    pub target: ObjectRef,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAnchor {
    pub anchor_id: String,
    pub project_id: String,
    pub chapter_id: String,
    pub start_byte: usize,
    pub end_byte: usize,
    /// Content identity for the exact chapter text against which this range
    /// was last verified. Editor-session revisions are never persisted here.
    #[serde(default)]
    pub text_hash: String,
    /// Selected text and nearby context let Core safely relocate an anchor
    /// after another device changes the chapter. Empty legacy snapshots are
    /// treated as unresolved rather than trusting an old byte offset.
    #[serde(default)]
    pub anchored_text: String,
    #[serde(default)]
    pub context_before: String,
    #[serde(default)]
    pub context_after: String,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

impl TextAnchor {
    pub fn object_ref(&self) -> ObjectRef {
        ObjectRef::TextAnchor {
            project_id: self.project_id.clone(),
            object_id: self.anchor_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedEntry {
    pub entry_id: String,
    pub project_id: String,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub confirmed_misspellings: Vec<String>,
    #[serde(default)]
    pub auto_link_enabled: bool,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

impl NamedEntry {
    pub fn object_ref(&self) -> ObjectRef {
        ObjectRef::NamedEntry {
            project_id: self.project_id.clone(),
            object_id: self.entry_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkOrigin {
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum LinkTarget {
    Object(ObjectRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectLink {
    pub link_id: String,
    pub source: ObjectRef,
    pub target: LinkTarget,
    pub origin: LinkOrigin,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationKind {
    Tag,
    Binding,
    NamedEntry,
    TextAnchor,
    Link,
    Project,
    Volume,
    Chapter,
    StarMap,
    StarMapNode,
    StarMapEmbed,
}

/// A mutation of one independently persisted annotation object.
///
/// Deletes name the record and timestamp; the store converts that request to a
/// tombstone while retaining the existing record's payload and stable ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationMutation {
    UpsertTag(Tag),
    UpsertBinding(TagBinding),
    UpsertNamedEntry(NamedEntry),
    UpsertTextAnchor(TextAnchor),
    UpsertLink(ObjectLink),
    Delete {
        kind: AnnotationKind,
        project_id: String,
        object_id: String,
        deleted_at_ms: i64,
    },
}
