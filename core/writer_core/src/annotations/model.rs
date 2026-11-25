//! 注解模型：标签、文本锚点、命名条目与对象链接。
//!
//! Issue #1060：标签与导航链接的唯一持久化所有者是 annotations 仓库。
//! 星图节点、节点素材与 `StarMapLink` 不再保存第二份标签/链接数据；
//! 显示层需要的列表通过关系查询组装只读投影。

use serde::{Deserialize, Serialize};

use crate::object_ref::{CanvasRoute, ObjectRef, ObjectScope};

/// The flat, user-editable tag names installed when a new project is created.
pub const DEFAULT_TAG_NAMES: [&str; 14] = [
    "角色", "主角", "配角", "反派", "势力", "地点", "装备", "道具", "能力", "生物", "事件", "设定",
    "伏笔", "线索",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub tag_id: String,
    /// 标签命名空间：作品 ID；空字符串表示全局素材域。
    pub project_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

impl Tag {
    /// 标签所属命名空间。项目标签只在项目标签目录可见。
    pub fn scope(&self) -> ObjectScope {
        ObjectScope::from_project_id(Some(self.project_id.as_str()))
    }

    /// 命名空间键，用于隔离不同作用域的同名标签。
    pub fn namespace_key(&self) -> String {
        match self.scope() {
            ObjectScope::Global => String::new(),
            ObjectScope::Project { project_id } => project_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagBinding {
    pub binding_id: String,
    /// 绑定所属命名空间（标签命名空间），空字符串表示全局素材域。
    #[serde(default)]
    pub project_id: String,
    pub tag_id: String,
    pub target: ObjectRef,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<i64>,
}

impl TagBinding {
    /// 唯一关系键：命名空间 + typed 目标 + 标签。
    ///
    /// 同一标签对同一对象只能有一条有效绑定；重复 attach 幂等命中它。
    pub fn relation_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.project_id,
            self.tag_id,
            self.target.stable_key()
        )
    }

    pub fn is_active(&self) -> bool {
        self.deleted_at_ms.is_none()
    }
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
    /// 普通对象引用（章节、命名条目、素材、实例…）。
    Object(ObjectRef),
    /// 画布导航：保留完整 [`CanvasRoute`]（起点、穿越段、终点）。
    CanvasNavigation(CanvasRoute),
}

impl LinkTarget {
    /// 链接目标解析出的对象身份；画布导航在解析后才知道终点对象。
    pub fn object_ref(&self) -> Option<&ObjectRef> {
        match self {
            Self::Object(reference) => Some(reference),
            Self::CanvasNavigation(_) => None,
        }
    }
}

/// 导航链接：唯一持久化在 annotations 仓库，按源所有权保存一次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectLink {
    pub link_id: String,
    pub source: ObjectRef,
    pub target: LinkTarget,
    pub origin: LinkOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 对象级创建时间（毫秒）。旧 `StarMapLink.created_at`（秒）迁移时换算。
    #[serde(default)]
    pub created_at_ms: i64,
    /// 对象级版本；同步与 MCP 以它做增量判断。
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
    /// 共享节点素材（旧记录中的 `starMapNode`）。
    #[serde(rename = "nodeAsset", alias = "starMapNode")]
    NodeAsset,
    /// 画布节点实例。
    NodeInstance,
    /// 画布嵌入实例（旧记录中的 `starMapEmbed`）。
    #[serde(rename = "embedInstance", alias = "starMapEmbed")]
    EmbedInstance,
    /// 星图节点连接端口。
    CanvasAnchor,
}

impl AnnotationKind {
    /// 旧记录中的种类名映射到新种类，供迁移与兼容读取使用。
    pub fn from_legacy_name(name: &str) -> Option<Self> {
        match name {
            "starMapNode" => Some(Self::NodeAsset),
            "starMapEmbed" => Some(Self::EmbedInstance),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Binding => "binding",
            Self::NamedEntry => "namedEntry",
            Self::TextAnchor => "textAnchor",
            Self::Link => "link",
            Self::Project => "project",
            Self::Volume => "volume",
            Self::Chapter => "chapter",
            Self::StarMap => "starMap",
            Self::NodeAsset => "nodeAsset",
            Self::NodeInstance => "nodeInstance",
            Self::EmbedInstance => "embedInstance",
            Self::CanvasAnchor => "canvasAnchor",
        }
    }
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
        /// 记录所在命名空间；全局素材域与作品域分开。
        scope: ObjectScope,
        object_id: String,
        deleted_at_ms: i64,
    },
}
