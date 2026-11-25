//! # 跨业务稳定对象引用（Issue #1060）
//!
//! 本模块是"谁拥有哪个对象"的唯一身份来源：每类对象有且只有一个持久化
//! 所有者，其它地方只保存稳定 ID，不复制实体内容。
//!
//! ## 作用域
//!
//! - [`ObjectScope::Project`]：作品内对象（章节、文本锚点、命名条目…）。
//! - [`ObjectScope::Global`]：全局素材域（未归属作品的独立星图、共享素材）。
//!
//! 项目归属只用于授权与标签命名空间，不参与素材身份。素材移动到另一个库
//! 不改变 ID；同一共享素材被多个作品引用时仍然是同一个 ID。
//!
//! ## 身份分类
//!
//! - 共享素材：[`NodeAssetId`] / [`ObjectRef::NodeAsset`]、[`StarMapAssetId`]。
//!   内容只保存在素材本体记录里，可被任意数量画布投影。
//! - 画布实例：[`NodeInstanceId`] / [`ObjectRef::NodeInstance`]、
//!   [`EmbedInstanceId`] / [`ObjectRef::EmbedInstance`]。实例从属一张星图，
//!   实例 ID 与素材 ID 不混用。
//! - 画布锚点：[`CanvasAnchorId`] / [`ObjectRef::CanvasAnchor`]。这是星图节点
//!   上的连接端口，与正文 [`ObjectRef::TextAnchor`] 是不同语义，两者不互转。
//! - 画布路由：[`CanvasRoute`]（即 [`crate::starmap::types::StarMapTargetPath`]）。
//!   跨层导航路径保留起点、穿越段与终点，用于导航与端点校验，
//!   不是通用对象 ID，不参与标签绑定。

use serde::{Deserialize, Serialize};

use crate::starmap::types::StarMapTargetPath;

/// 跨层画布路由。
///
/// 只用于"从哪张图、经过哪些嵌入/portal、到达哪个终点"的导航语义。
/// 它不是对象身份：解析出实际对象后必须转换成分类型的 [`ObjectRef`]。
pub type CanvasRoute = StarMapTargetPath;

/// 对象所属命名空间。
///
/// 标签按此命名空间隔离：项目标签只在项目标签目录中可见，全局素材域的
/// 标签不自动进入任何项目。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObjectScope {
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "project")]
    Project { project_id: String },
}

impl ObjectScope {
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Project { project_id } => Some(project_id),
        }
    }

    /// 空 project_id 视为全局素材域。
    pub fn from_project_id(project_id: Option<&str>) -> Self {
        match project_id.filter(|id| !id.is_empty()) {
            Some(project_id) => Self::Project {
                project_id: project_id.to_string(),
            },
            None => Self::Global,
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }
}

/// 共享节点素材的稳定 ID（`starmaps/library/nodes/<bucket>/<asset_id>.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeAssetId(String);

impl NodeAssetId {
    pub fn new(asset_id: impl Into<String>) -> Self {
        Self(asset_id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for NodeAssetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for NodeAssetId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for NodeAssetId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for NodeAssetId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// 星图素材（整张星图作为可复用素材）的稳定 ID。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StarMapAssetId(String);

impl StarMapAssetId {
    pub fn new(asset_id: impl Into<String>) -> Self {
        Self(asset_id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for StarMapAssetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for StarMapAssetId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for StarMapAssetId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// 画布节点实例身份：实例从属一张星图，`instance_id` 只在画布内唯一。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInstanceId {
    pub starmap_id: String,
    pub instance_id: String,
}

impl NodeInstanceId {
    pub fn new(starmap_id: impl Into<String>, instance_id: impl Into<String>) -> Self {
        Self {
            starmap_id: starmap_id.into(),
            instance_id: instance_id.into(),
        }
    }
}

/// 画布嵌入实例身份（子星图挂在宿主画布上的实例）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedInstanceId {
    pub starmap_id: String,
    pub instance_id: String,
}

impl EmbedInstanceId {
    pub fn new(starmap_id: impl Into<String>, instance_id: impl Into<String>) -> Self {
        Self {
            starmap_id: starmap_id.into(),
            instance_id: instance_id.into(),
        }
    }
}

/// 星图节点上的连接端口身份。
///
/// 端口属于共享素材内容（`StarMapAnchor`），因此以素材 ID + 端口 ID 定位；
/// 同一素材在多个画布上的实例共享同一端口定义。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasAnchorId {
    pub asset_id: NodeAssetId,
    pub anchor_id: String,
}

impl CanvasAnchorId {
    pub fn new(asset_id: impl Into<NodeAssetId>, anchor_id: impl Into<String>) -> Self {
        Self {
            asset_id: asset_id.into(),
            anchor_id: anchor_id.into(),
        }
    }
}

/// 稳定对象引用。
///
/// 每个变体只携带身份，不携带实体内容。序列化 tag 与历史记录
/// （`kind` 字段）保持一致的部分继续沿用，迁移期间新增的合法值是
/// `nodeAsset` / `nodeInstance` / `embedInstance` / `canvasAnchor`。
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
    /// 正文文本锚点：由章节与稳定 `anchor_id` 关联，随正文事务提交。
    TextAnchor {
        project_id: String,
        object_id: String,
    },
    NamedEntry {
        project_id: String,
        object_id: String,
    },
    /// 星图本体（可以是不属于任何作品的独立星图）。
    StarMap {
        scope: ObjectScope,
        object_id: String,
    },
    /// 共享节点素材：可被多个画布投影，内容只存一份。
    #[serde(rename = "nodeAsset")]
    NodeAsset {
        scope: ObjectScope,
        object_id: String,
    },
    /// 画布节点实例：位置与实例版本属于该画布。
    #[serde(rename = "nodeInstance")]
    NodeInstance {
        starmap_id: String,
        object_id: String,
    },
    /// 画布嵌入实例。
    #[serde(rename = "embedInstance")]
    EmbedInstance {
        starmap_id: String,
        object_id: String,
    },
    /// 星图节点连接端口（与正文文本锚点语义不同）。
    #[serde(rename = "canvasAnchor")]
    CanvasAnchor {
        starmap_id: String,
        object_id: String,
        anchor_id: String,
    },
}

impl ObjectRef {
    /// 授权与标签命名空间所属作用域。
    ///
    /// 画布实例继承其所在星图的作用域；共享素材的作用域来自素材归属。
    pub fn scope(&self) -> ObjectScope {
        match self {
            Self::Project { project_id, .. }
            | Self::Volume { project_id, .. }
            | Self::Chapter { project_id, .. }
            | Self::TextAnchor { project_id, .. }
            | Self::NamedEntry { project_id, .. } => ObjectScope::Project {
                project_id: project_id.clone(),
            },
            Self::StarMap { scope, .. } | Self::NodeAsset { scope, .. } => scope.clone(),
            Self::NodeInstance { .. } | Self::EmbedInstance { .. } | Self::CanvasAnchor { .. } => {
                ObjectScope::Global
            }
        }
    }

    /// 项目 ID；全局素材域返回 `None`。
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Self::Project { project_id, .. }
            | Self::Volume { project_id, .. }
            | Self::Chapter { project_id, .. }
            | Self::TextAnchor { project_id, .. }
            | Self::NamedEntry { project_id, .. } => Some(project_id),
            Self::StarMap { scope, .. } | Self::NodeAsset { scope, .. } => scope.project_id(),
            _ => None,
        }
    }

    /// 对象自身的稳定 ID（实例类返回画布内实例 ID）。
    pub fn object_id(&self) -> &str {
        match self {
            Self::Project { object_id, .. }
            | Self::Volume { object_id, .. }
            | Self::Chapter { object_id, .. }
            | Self::TextAnchor { object_id, .. }
            | Self::NamedEntry { object_id, .. }
            | Self::StarMap { object_id, .. }
            | Self::NodeAsset { object_id, .. }
            | Self::NodeInstance { object_id, .. }
            | Self::EmbedInstance { object_id, .. }
            | Self::CanvasAnchor { object_id, .. } => object_id,
        }
    }

    /// 实例/路由所在的画布 ID。
    pub fn starmap_id(&self) -> Option<&str> {
        match self {
            Self::NodeInstance { starmap_id, .. }
            | Self::EmbedInstance { starmap_id, .. }
            | Self::CanvasAnchor { starmap_id, .. } => Some(starmap_id),
            Self::StarMap { object_id, .. } => Some(object_id),
            _ => None,
        }
    }

    pub fn node_asset_id(&self) -> Option<NodeAssetId> {
        match self {
            Self::NodeAsset { object_id, .. } => Some(NodeAssetId::new(object_id.clone())),
            _ => None,
        }
    }

    pub fn node_instance_id(&self) -> Option<NodeInstanceId> {
        match self {
            Self::NodeInstance {
                starmap_id,
                object_id,
            } => Some(NodeInstanceId::new(starmap_id.clone(), object_id.clone())),
            _ => None,
        }
    }

    pub fn embed_instance_id(&self) -> Option<EmbedInstanceId> {
        match self {
            Self::EmbedInstance {
                starmap_id,
                object_id,
            } => Some(EmbedInstanceId::new(starmap_id.clone(), object_id.clone())),
            _ => None,
        }
    }

    pub fn canvas_anchor_id(&self) -> Option<CanvasAnchorId> {
        match self {
            Self::CanvasAnchor {
                object_id,
                anchor_id,
                ..
            } => Some(CanvasAnchorId::new(object_id.clone(), anchor_id.clone())),
            _ => None,
        }
    }

    /// 是否指向共享素材（而不是某个画布实例）。
    pub fn is_shared_asset(&self) -> bool {
        matches!(self, Self::NodeAsset { .. } | Self::StarMap { .. })
    }

    /// 稳定唯一键，用于唯一关系（标签绑定、索引、去重）而不依赖存储路径。
    pub fn stable_key(&self) -> String {
        let scope = match self.scope() {
            ObjectScope::Global => "global".to_string(),
            ObjectScope::Project { project_id } => format!("project:{project_id}"),
        };
        match self {
            Self::CanvasAnchor { anchor_id, .. } => format!(
                "{}|{}|{}|{}",
                self.kind_key(),
                scope,
                self.object_id(),
                anchor_id
            ),
            _ => format!(
                "{}|{}|{}|{}",
                self.kind_key(),
                scope,
                self.starmap_id().unwrap_or(""),
                self.object_id()
            ),
        }
    }

    fn kind_key(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Volume { .. } => "volume",
            Self::Chapter { .. } => "chapter",
            Self::TextAnchor { .. } => "textAnchor",
            Self::NamedEntry { .. } => "namedEntry",
            Self::StarMap { .. } => "starMap",
            Self::NodeAsset { .. } => "nodeAsset",
            Self::NodeInstance { .. } => "nodeInstance",
            Self::EmbedInstance { .. } => "embedInstance",
            Self::CanvasAnchor { .. } => "canvasAnchor",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_asset_identity_survives_scope_change() {
        let asset = ObjectRef::NodeAsset {
            scope: ObjectScope::Global,
            object_id: "asset_1".to_string(),
        };
        let moved = ObjectRef::NodeAsset {
            scope: ObjectScope::Project {
                project_id: "p1".to_string(),
            },
            object_id: "asset_1".to_string(),
        };
        assert_eq!(asset.object_id(), moved.object_id());
        assert_ne!(asset.stable_key(), moved.stable_key());
    }

    #[test]
    fn instance_and_asset_ids_do_not_collide() {
        let asset = ObjectRef::NodeAsset {
            scope: ObjectScope::Global,
            object_id: "n1".to_string(),
        };
        let instance = ObjectRef::NodeInstance {
            starmap_id: "sm1".to_string(),
            object_id: "n1".to_string(),
        };
        assert_ne!(asset.stable_key(), instance.stable_key());
    }

    #[test]
    fn canvas_anchor_is_not_a_text_anchor() {
        let canvas = ObjectRef::CanvasAnchor {
            starmap_id: "sm1".to_string(),
            object_id: "asset_1".to_string(),
            anchor_id: "a1".to_string(),
        };
        let text = ObjectRef::TextAnchor {
            project_id: "p1".to_string(),
            object_id: "a1".to_string(),
        };
        assert_ne!(canvas.stable_key(), text.stable_key());
    }

    #[test]
    fn global_scope_round_trips() {
        let scope = ObjectScope::Global;
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, "{\"kind\":\"global\"}");
        let decoded: ObjectScope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, scope);
    }

    #[test]
    fn node_asset_wire_kind_is_camel_case() {
        let reference = ObjectRef::NodeAsset {
            scope: ObjectScope::Project {
                project_id: "p1".to_string(),
            },
            object_id: "asset_1".to_string(),
        };
        let value = serde_json::to_value(&reference).unwrap();
        assert_eq!(value["kind"], "nodeAsset");
        assert_eq!(value["scope"]["kind"], "project");
    }
}
