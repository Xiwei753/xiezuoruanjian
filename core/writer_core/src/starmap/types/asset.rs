//! 素材库：共享对象本体、画布引用实例与库目录归属。
//!
//! 三层语义：
//! - 素材本体（[`StarMapNodeDefinition`] / 星图 meta）拥有稳定全局 ID；
//! - 画布只保存 [`StarMapNodeInstance`] 引用，位置与连线属于当前画布；
//! - [`StarMapLibraryEntry`] 只保存库目录归属与分类，可跨全局/作品库移动。

use serde::{Deserialize, Serialize};

use super::{
    StarMapNodeKind, StarMapNodeStyle, StarMapPoint,
};
use crate::starmap::semantic::{
    StarMapAnchor, StarMapNodeContent, StarMapPortal, StarMapProvenance,
};

pub const CURRENT_LIBRARY_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapAssetKind {
    Node,
    StarMap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StarMapAssetScope {
    Global,
    Project { project_id: String },
}

impl StarMapAssetScope {
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Project { project_id } => Some(project_id),
        }
    }

    pub fn from_project_id(project_id: Option<&str>) -> Self {
        match project_id.filter(|id| !id.is_empty()) {
            Some(project_id) => Self::Project {
                project_id: project_id.to_string(),
            },
            None => Self::Global,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAssetRef {
    pub kind: StarMapAssetKind,
    pub asset_id: String,
}

impl StarMapAssetRef {
    pub fn node(asset_id: impl Into<String>) -> Self {
        Self {
            kind: StarMapAssetKind::Node,
            asset_id: asset_id.into(),
        }
    }

    pub fn starmap(asset_id: impl Into<String>) -> Self {
        Self {
            kind: StarMapAssetKind::StarMap,
            asset_id: asset_id.into(),
        }
    }
}

/// 画布中的一个节点引用实例。
///
/// `instance_id` 是画布内身份，边与 portal 路径都指向它；
/// `target_node_id` 是共享素材本体 ID。`starmap_id` 记录实例所属画布，
/// 让库索引可以在不加载全部星图的情况下定位引用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeInstance {
    pub instance_id: String,
    pub target_node_id: String,
    #[serde(default)]
    pub position: StarMapPoint,
    #[serde(default)]
    pub starmap_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

/// 库目录中的一条归属记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLibraryEntry {
    pub entry_id: String,
    #[serde(default)]
    pub parent_folder_id: Option<String>,
    pub scope: StarMapAssetScope,
    pub target: StarMapAssetRef,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLibraryFolder {
    pub folder_id: String,
    #[serde(default)]
    pub parent_folder_id: Option<String>,
    pub scope: StarMapAssetScope,
    pub name: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapLibraryIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub entries: Vec<StarMapLibraryEntry>,
    #[serde(default)]
    pub folders: Vec<StarMapLibraryFolder>,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for StarMapLibraryIndex {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
            entries: Vec::new(),
            folders: Vec::new(),
            updated_at: 0,
        }
    }
}

/// 画布实例的反向引用索引项。只保存身份，不保存位置等画布事实。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeReference {
    pub target_node_id: String,
    pub starmap_id: String,
    pub instance_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReferenceIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub nodes: Vec<StarMapNodeReference>,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for StarMapReferenceIndex {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
            nodes: Vec::new(),
            updated_at: 0,
        }
    }
}

/// 素材在某个画布中的引用位置。`instance_id` 为节点实例 ID；
/// 星图引用对应 Embed 实例 ID。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapAssetReference {
    pub target: StarMapAssetRef,
    pub project_id: Option<String>,
    pub starmap_id: String,
    pub instance_id: String,
}

/// 节点素材本体：可被多个画布实例共享的可编辑内容。
///
/// 画布实例只额外保存 `id`/`position`；标题、正文、锚点、portal、样式、
/// 来源与时间戳都以本类型为准。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodeDefinition {
    pub asset_id: String,
    pub title: String,
    pub kind: StarMapNodeKind,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content: StarMapNodeContent,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchor>,
    #[serde(default)]
    pub portal: Option<StarMapPortal>,
    #[serde(default)]
    pub style: StarMapNodeStyle,
    #[serde(default)]
    pub provenance: StarMapProvenance,
    pub created_at: u64,
    pub updated_at: u64,
}

impl StarMapNodeDefinition {
    pub fn from_node(asset_id: impl Into<String>, node: &super::StarMapNode) -> Self {
        Self {
            asset_id: asset_id.into(),
            title: node.title.clone(),
            kind: node.kind.clone(),
            payload: node.payload.clone(),
            tags: node.tags.clone(),
            content: node.content.clone(),
            anchors: node.anchors.clone(),
            portal: node.portal.clone(),
            style: node.style.clone(),
            provenance: node.provenance.clone(),
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }

    /// 用共享本体组装某个画布实例的完整展示节点。
    pub fn instantiate(&self, instance_id: &str, position: StarMapPoint) -> super::StarMapNode {
        super::StarMapNode {
            id: instance_id.to_string(),
            title: self.title.clone(),
            kind: self.kind.clone(),
            payload: self.payload.clone(),
            tags: self.tags.clone(),
            content: self.content.clone(),
            anchors: self.anchors.clone(),
            portal: self.portal.clone(),
            position,
            style: self.style.clone(),
            provenance: self.provenance.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    /// 把共享内容写回画布实例，保留实例 ID、位置和实例本身的时间戳更新。
    pub fn apply_to_node(&self, node: &mut super::StarMapNode) {
        node.title = self.title.clone();
        node.kind = self.kind.clone();
        node.payload = self.payload.clone();
        node.tags = self.tags.clone();
        node.content = self.content.clone();
        node.anchors = self.anchors.clone();
        node.portal = self.portal.clone();
        node.style = self.style.clone();
        node.provenance = self.provenance.clone();
        node.created_at = self.created_at;
        node.updated_at = node.updated_at.max(self.updated_at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_round_trips_through_json() {
        let global = serde_json::to_string(&StarMapAssetScope::Global).unwrap();
        assert_eq!(global, "{\"kind\":\"global\"}");
        let project = StarMapAssetScope::Project {
            project_id: "p1".to_string(),
        };
        let value: StarMapAssetScope =
            serde_json::from_str("{\"kind\":\"project\",\"projectId\":\"p1\"}").unwrap();
        assert_eq!(value, project);
    }

    #[test]
    fn entry_keeps_asset_id_when_scope_moves() {
        let entry = StarMapLibraryEntry {
            entry_id: "le_1".to_string(),
            parent_folder_id: None,
            scope: StarMapAssetScope::Global,
            target: StarMapAssetRef::node("n1"),
            created_at: 1,
            updated_at: 1,
        };
        let mut moved = entry.clone();
        moved.scope = StarMapAssetScope::Project {
            project_id: "p1".to_string(),
        };
        assert_eq!(moved.target.asset_id, entry.target.asset_id);
    }
}
